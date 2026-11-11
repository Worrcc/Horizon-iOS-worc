//! Touch controls in the game (`touch_controls.rs` turns fingers into [`Output`]s): applying
//! them as the keys, clicks and look movement a keyboard and mouse would give, and drawing
//! the buttons over the HUD. Enabled on iOS, or with HORIZON_TOUCH=1 for desktop testing.
mod editor;
use super::{Game, Screen, event_loop::EventLoopCtl};
use horizon_client::touch_layout::{self, Entry, Overrides, Rect, Show};
use horizon_client::{
    hud::{HudAtlas, HudBatch},
    lang,
    touch_controls::{Button, Layout, Output, TouchControls},
    ui,
};
use std::time::Instant;
use winit::{
    event::{
        DeviceId, ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
    },
    keyboard::KeyCode,
};

/// Touch turns the view this many window pixels' worth of mouse movement per pixel dragged.
const LOOK_SCALE: f64 = 0.6;

/// Whether this system's touches drive the touch controls.
pub(super) fn enabled() -> bool {
    cfg!(target_os = "ios") || forced()
}
fn forced() -> bool {
    static FORCED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FORCED.get_or_init(|| std::env::var("HORIZON_TOUCH").is_ok_and(|v| v == "1"))
}

/// The menus' own two buttons, top left and top right: the on-screen keyboard and Back (Esc).
#[derive(Clone, Copy, PartialEq, Eq)]
enum MenuButton {
    Keyboard,
    Back,
}

fn menu_button(layout: &Layout, button: MenuButton) -> [f32; 4] {
    match button {
        MenuButton::Keyboard => [4.0, 4.0, 22.0, 16.0],
        MenuButton::Back => [layout.width - 30.0, 4.0, 26.0, 16.0],
    }
}

fn inside([x, y, w, h]: [f32; 4], (px, py): (f32, f32)) -> bool {
    px >= x - 2.0 && px < x + w + 2.0 && py >= y - 2.0 && py < y + h + 2.0
}

/// The GUI's size and scale for a window of `size` pixels.
fn layout_of(size: [u32; 2]) -> (Layout, f32) {
    let batch = HudBatch::new(size);
    (
        Layout {
            width: batch.width,
            height: batch.height,
            main_hand_left: false,
        },
        batch.scale,
    )
}

/// The window events a menu pointer output stands for, on `device`. Empty for the world's
/// outputs.
fn pointer_event(device: DeviceId, output: Output, scale: f32) -> Vec<WindowEvent> {
    vec![match output {
        Output::Pointer(x, y) => WindowEvent::CursorMoved {
            device_id: device,
            position: winit::dpi::PhysicalPosition::new(f64::from(x * scale), f64::from(y * scale)),
        },
        Output::PointerButton(pressed) => WindowEvent::MouseInput {
            device_id: device,
            state: if pressed {
                ElementState::Pressed
            } else {
                ElementState::Released
            },
            button: MouseButton::Left,
        },
        Output::PointerRight => {
            return vec![
                WindowEvent::MouseInput {
                    device_id: device,
                    state: ElementState::Pressed,
                    button: MouseButton::Right,
                },
                WindowEvent::MouseInput {
                    device_id: device,
                    state: ElementState::Released,
                    button: MouseButton::Right,
                },
            ];
        }
        Output::Scroll(lines) => WindowEvent::MouseWheel {
            device_id: device,
            delta: MouseScrollDelta::LineDelta(0.0, lines),
            phase: TouchPhase::Moved,
        },
        _ => return Vec::new(),
    }]
}

/// Shared artwork for built-in, menu and mod touch buttons.
#[derive(Clone, Copy)]
enum TouchFrame {
    Slot(u32),
    #[cfg(feature = "mods")]
    SmallPlate,
    Offhand(bool),
}
const SLOT_FRAME: TouchFrame = TouchFrame::Slot(0xB3FFFFFF);

fn small_plate(frame: TouchFrame) -> bool {
    match frame {
        TouchFrame::Slot(_) | TouchFrame::Offhand(_) => false,
        #[cfg(feature = "mods")]
        TouchFrame::SmallPlate => true,
    }
}

fn draw_button_frame(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    frame: TouchFrame,
    down: bool,
) {
    let [x, y, w, h] = rect;
    match frame {
        TouchFrame::Slot(tint) => ui::nine_slice(
            batch,
            atlas,
            "gui/sprites/gamemode_switcher/slot",
            x,
            y,
            w,
            h,
            tint,
        ),
        #[cfg(feature = "mods")]
        TouchFrame::SmallPlate => batch.fill(atlas, x, y, w, h, 0x90000000),
        TouchFrame::Offhand(left) => batch.sprite(
            atlas,
            if left {
                "gui/sprites/hud/hotbar_offhand_left"
            } else {
                "gui/sprites/hud/hotbar_offhand_right"
            },
            x,
            y,
        ),
    }
    if down {
        let inset = if small_plate(frame) { 0.0 } else { 3.0 };
        batch.fill(
            atlas,
            x + inset,
            y + inset,
            w - 2.0 * inset,
            h - 2.0 * inset,
            0x60FFFFFF,
        );
    }
}

fn draw_button_selection(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    frame: TouchFrame,
    lit: bool,
) {
    if lit {
        let [x, y, w, h] = rect;
        let outset = if small_plate(frame) { 3.0 } else { 0.0 };
        ui::nine_slice(
            batch,
            atlas,
            "gui/sprites/gamemode_switcher/selection",
            x - outset,
            y - outset,
            w + 2.0 * outset,
            h + 2.0 * outset,
            0xFFFFFFFF,
        );
    }
}

fn draw_button_label(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, h]: [f32; 4], label: &str) {
    let tw = HudBatch::text_width(atlas, label);
    batch.text(
        atlas,
        label,
        (x + (w - tw) / 2.0).round(),
        (y + (h - 8.0) / 2.0).round(),
        0xFFFFFFFF,
        true,
    );
}

/// A slot-style menu button, retaining its existing active/pressed state.
pub(super) fn draw_button(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    label: &str,
    down: bool,
) {
    draw_button_frame(batch, atlas, rect, SLOT_FRAME, down);
    draw_button_label(batch, atlas, rect, label);
}

#[cfg(feature = "mods")]
pub(super) fn draw_container_frame(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    lit: bool,
) {
    draw_button_frame(batch, atlas, rect, SLOT_FRAME, false);
    draw_button_selection(batch, atlas, rect, SLOT_FRAME, lit);
}

fn draw_pause(batch: &mut HudBatch, atlas: &HudAtlas, [x, y, w, h]: [f32; 4]) {
    let (x, y) = (x + (w - 6.0) / 2.0, y + (h - 8.0) / 2.0);
    for dx in [0.0, 4.0] {
        batch.fill(atlas, x + dx + 1.0, y + 1.0, 2.0, 8.0, 0xFF3F3F3F);
    }
    for dx in [0.0, 4.0] {
        batch.fill(atlas, x + dx, y, 2.0, 8.0, 0xFFFFFFFF);
    }
}

/// Touch on the startup menu (the title screen shown while the world's resources load, before
/// `Game` exists): the first finger is the pointer, and the keyboard button shows the keyboard
/// for its text fields.
#[derive(Default)]
pub(super) struct MenuTouch {
    controls: TouchControls,
    device: Option<DeviceId>,
    /// The on-screen keyboard is up (`Window::set_ime_allowed`).
    pub keyboard: bool,
}

/// What a touch on the startup menu does.
pub(super) enum MenuTouchAction {
    /// Pointer events for the menu.
    Events(Vec<WindowEvent>),
    /// The keyboard button: show or hide the keyboard.
    ToggleKeyboard,
    /// The Microsoft account button (`account.rs`).
    Account,
}

impl MenuTouch {
    /// A touch on a window of `size` pixels; `atlas` once the menu has its artwork.
    pub(super) fn touch(
        &mut self,
        touch: Touch,
        size: [u32; 2],
        atlas: Option<&HudAtlas>,
    ) -> MenuTouchAction {
        let (layout, scale) = layout_of(size);
        let at = (
            touch.location.x as f32 / scale,
            touch.location.y as f32 / scale,
        );
        if touch.phase == TouchPhase::Started {
            if inside(menu_button(&layout, MenuButton::Keyboard), at) {
                return MenuTouchAction::ToggleKeyboard;
            }
            if atlas.is_some_and(|atlas| super::account::hit(&layout, atlas, at)) {
                return MenuTouchAction::Account;
            }
        }
        self.device = Some(touch.device_id);
        let outputs =
            self.controls
                .touch(touch.id, touch.phase, at, &layout, false, Instant::now());
        MenuTouchAction::Events(self.events(outputs, scale))
    }

    /// Once a frame: a finger held still right clicks.
    pub(super) fn update(&mut self, size: [u32; 2]) -> Vec<WindowEvent> {
        if !self.controls.used {
            return Vec::new();
        }
        let (_, scale) = layout_of(size);
        let outputs = self.controls.update(Instant::now());
        self.events(outputs, scale)
    }

    fn events(&self, outputs: Vec<Output>, scale: f32) -> Vec<WindowEvent> {
        let Some(device) = self.device else {
            return Vec::new();
        };
        outputs
            .into_iter()
            .flat_map(|o| pointer_event(device, o, scale))
            .collect()
    }

    /// The account button, and the keyboard button once a finger has touched the screen.
    pub(super) fn draw(&self, batch: &mut HudBatch, atlas: &HudAtlas) {
        if !enabled() {
            return;
        }
        let layout = Layout {
            width: batch.width,
            height: batch.height,
            main_hand_left: false,
        };
        super::account::draw(batch, atlas, &layout);
        if !self.controls.used {
            return;
        }
        draw_button(
            batch,
            atlas,
            menu_button(&layout, MenuButton::Keyboard),
            &lang::get_or("horizon.touch.keyboard", "Aa"),
            self.keyboard,
        );
    }
}

#[derive(Default)]
pub(super) struct TouchInput {
    overrides: Overrides,
    editor: Option<editor::Editor>,
    controls: TouchControls,
    /// The touchscreen, for the pointer events the menus get.
    device: Option<DeviceId>,
    /// The on-screen keyboard is up (`Window::set_ime_allowed`).
    keyboard: bool,
    #[cfg(feature = "mods")]
    mod_fingers: std::collections::HashMap<u64, (usize, String)>,
    /// Menu buttons own their finger through lift, including across screen changes.
    menu_fingers: std::collections::HashSet<u64>,
}

impl TouchInput {
    /// Whether a mod key's on-screen button shows: the player's choice, else
    /// its default (a mod's own button shows; an automatic one is hidden
    /// unless the mod asked for it shown).
    #[cfg(feature = "mods")]
    pub(super) fn mod_key_shown(
        &self,
        mods: &horizon_mods::Mods,
        owner: usize,
        mod_id: &str,
        key: &horizon_mods::ModKey,
    ) -> bool {
        let own = mods
            .touch_buttons()
            .any(|(i, name, _, _)| i == owner && name == key.name);
        let hidden_by_default = !own && !key.auto_shown;
        let id = format!("mod.{mod_id}.{}", key.name);
        !self
            .overrides
            .0
            .get(&id)
            .map_or(hidden_by_default, |v| v.hidden)
    }
    /// Shows or hides a mod key's on-screen button where it is.
    #[cfg(feature = "mods")]
    pub(super) fn set_mod_key_shown(&mut self, mod_id: &str, key: &str, shown: bool) {
        self.overrides
            .set_hidden(&format!("mod.{mod_id}.{key}"), !shown);
    }
    /// Touch Sneak (toggle) and Sprint (button or joystick) hold their mappings
    /// independently of the keyboard's modifier state.
    pub(super) fn holds_mapping(&self, mapping: &str) -> bool {
        match mapping {
            "key.sneak" => self.controls.pressed(Button::Sneak),
            "key.sprint" => self.controls.holds_sprint(),
            _ => false,
        }
    }
    pub(super) fn editor_closed_by_options(&mut self) {
        self.editor = None;
    }
    pub(super) fn load(options: &std::path::Path) -> Self {
        let text = std::fs::read_to_string(options.with_file_name("horizon-touch.txt"))
            .unwrap_or_default();
        Self {
            overrides: Overrides::parse(&text),
            ..Default::default()
        }
    }
}

struct TouchItem {
    id: String,
    name: String,
    label: String,
    rect: Option<Rect>,
    hidden: bool,
    /// The effective default condition, for the editor and live visibility.
    default_visible: bool,
    condition: String,
    kind: TouchKind,
}
enum TouchKind {
    Game(Button),
    Stick,
    #[cfg(feature = "mods")]
    Mod {
        owner: usize,
        key: String,
        button: Option<horizon_mods::TouchButton>,
        down: bool,
    },
}

const HOTBAR_KEYS: [KeyCode; 9] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
];

impl Game<'_> {
    fn touch_items(&self, layout: &Layout) -> Vec<TouchItem> {
        let riding = self.riding.map(|id| self.vehicle_kind(id));
        let mount_labels = riding
            .as_ref()
            .and_then(|id| self.pack.mount_controls.0.get(id));
        let mut items: Vec<_> = Button::ALL
            .into_iter()
            .map(|b| TouchItem {
                id: b.id().into(),
                name: lang::get(b.name_key()),
                label: mount_labels.map_or_else(
                    || lang::get(b.label_key()),
                    |(sprint, attack)| match b {
                        Button::Sprint => sprint.clone(),
                        Button::Attack => attack.clone(),
                        _ => lang::get(b.label_key()),
                    },
                ),
                rect: Some(layout.button(b)),
                hidden: false,
                default_visible: !b.riding_default() || riding.is_some(),
                condition: if b.riding_default() {
                    lang::get("horizon.touch.condition.riding")
                } else {
                    String::new()
                },
                kind: TouchKind::Game(b),
            })
            .collect();
        let r = horizon_client::touch_controls::STICK_RADIUS;
        let (x, y) = layout.stick_rest();
        items.push(TouchItem {
            id: "game.stick".into(),
            name: lang::get("horizon.touch.stick"),
            label: String::new(),
            rect: Some([x - r, y - r, 2.0 * r, 2.0 * r]),
            hidden: false,
            default_visible: true,
            condition: String::new(),
            kind: TouchKind::Stick,
        });
        #[cfg(feature = "mods")]
        for (owner, entry) in self.mods.mods.entries().iter().enumerate() {
            let Some(manifest) = &entry.manifest else {
                continue;
            };
            let keys = self.mods.mods.keys_of(owner);
            let buttons: Vec<_> = self
                .mods
                .mods
                .touch_buttons()
                .filter(|(i, _, _, _)| *i == owner)
                .collect();
            let auto: Vec<_> = keys
                .iter()
                .filter(|k| !buttons.iter().any(|(_, n, _, _)| *n == k.name))
                .collect();
            let labels = touch_layout::labels(
                &auto
                    .iter()
                    .map(|k| {
                        if k.label.is_empty() {
                            k.name.as_str()
                        } else {
                            k.label.as_str()
                        }
                    })
                    .collect::<Vec<_>>(),
            );
            for key in keys {
                let button = buttons
                    .iter()
                    .find(|(_, name, _, _)| *name == key.name)
                    .map(|(_, _, b, _)| (*b).clone());
                let rect = button.as_ref().map(|b| {
                    let [x, y, w, h] = touch_rect(b, layout);
                    let inset = if w < 20.0 { 3.0 } else { 0.0 };
                    [x - inset, y - inset, w + 2.0 * inset, h + 2.0 * inset]
                });
                let label = button.as_ref().map_or_else(
                    || {
                        labels[auto
                            .iter()
                            .position(|k| k.name == key.name)
                            .expect("automatic key")]
                        .clone()
                    },
                    |b| b.label.clone(),
                );
                items.push(TouchItem {
                    id: format!("mod.{}.{}", manifest.id, key.name),
                    name: horizon_client::text::Text::tr(
                        "horizon.touch.mod_name",
                        &[&manifest.name, &key.label],
                    )
                    .plain(),
                    label,
                    rect,
                    // Until the layout resolves: whether it starts hidden. An
                    // automatic button does, unless its mod asked otherwise.
                    hidden: button.is_none() && !key.auto_shown,
                    default_visible: key.touch_context.as_ref().is_none_or(|c| {
                        c.matches(
                            riding.as_deref(),
                            self.inventory
                                .get(self.selected)
                                .and_then(|s| s.as_ref())
                                .map(|s| s.item.as_ref()),
                        )
                    }),
                    condition: key.touch_context.as_ref().map_or_else(String::new, |c| {
                        match (!c.riding.is_empty(), !c.holding.is_empty()) {
                            (true, true) => lang::get("horizon.touch.condition.riding_holding"),
                            (true, false) => lang::get("horizon.touch.condition.riding"),
                            (false, true) => lang::get("horizon.touch.condition.holding"),
                            _ => String::new(),
                        }
                    }),
                    kind: TouchKind::Mod {
                        owner,
                        key: key.name.clone(),
                        button,
                        down: self.mods.mods.touch_key_down(owner, &key.name),
                    },
                });
            }
        }
        let entries: Vec<_> = items
            .iter()
            .map(|item| Entry {
                id: item.id.clone(),
                default: item.rect,
                hidden_by_default: item.hidden,
                visible_now: self
                    .touch
                    .overrides
                    .show(&item.id)
                    .visible(item.default_visible, riding.is_some()),
            })
            .collect();
        for (item, placed) in items.iter_mut().zip(touch_layout::resolve(
            (layout.width, layout.height),
            &entries,
            &self.touch.overrides,
        )) {
            item.rect = placed.rect;
            item.hidden = placed.hidden;
            let show = self.touch.overrides.show(&item.id);
            let condition = match show {
                Show::Default => item.condition.clone(),
                Show::Always => lang::get("horizon.touch.condition.always"),
                Show::Riding => lang::get("horizon.touch.condition.riding"),
                Show::Never => lang::get("horizon.touch.condition.never"),
            };
            if !condition.is_empty() {
                item.name = horizon_client::text::Text::tr(
                    "horizon.touch.condition_name",
                    &[&item.name, &condition],
                )
                .plain();
            }
        }
        items
    }
    fn touch_geometry(&self, items: &[TouchItem]) -> horizon_client::touch_controls::Geometry {
        horizon_client::touch_controls::Geometry {
            buttons: Button::ALL.map(|b| {
                items
                    .iter()
                    .find(|i| i.id == b.id() && !i.hidden)
                    .and_then(|i| i.rect)
            }),
            stick: items
                .iter()
                .find(|i| i.id == "game.stick" && !i.hidden)
                .and_then(|i| i.rect),
            moved_stick: self.touch.overrides.0.contains_key("game.stick"),
        }
    }
    /// The world is on screen with nothing over it: the controls act on it.
    #[cfg(feature = "mods")]
    /// `input.touch-active`: the player is using the touch screen. True on the game's screens
    /// too (a mod's own screen, the layout editor), where a mod still has to know to offer its
    /// touch buttons; the buttons themselves only draw while playing.
    pub(super) fn mod_touch_active(&self) -> bool {
        enabled() && (self.touch.controls.used || forced())
    }
    fn touch_playing(&self) -> bool {
        self.title.is_none()
            && self.screen == Screen::Playing
            && self.player.is_some()
            && self.server_packs.prompt.is_none()
            && self.link_prompt.is_none()
            && self.book.is_none()
            && self.sign_edit.is_none()
            && !self.dialog_open()
    }

    /// Back (Esc) is shown on screens over a world; the title screens have their own buttons.
    fn touch_back_shown(&self) -> bool {
        self.title.is_none() || self.screen != Screen::Playing
    }

    fn touch_layout(&self) -> Option<(Layout, f32)> {
        let size = self
            .renderer
            .as_ref()
            .map(|r| [r.config.width, r.config.height])?;
        let (mut layout, scale) = layout_of(size);
        layout.main_hand_left = self.settings.main_hand_left;
        Some((layout, scale))
    }

    /// `WindowEvent::Touch`.
    pub(super) fn touch_event(&mut self, event_loop: &dyn EventLoopCtl, touch: Touch) {
        if self.pending_save.is_some() {
            return;
        }
        let Some((layout, scale)) = self.touch_layout() else {
            return;
        };
        self.touch.device = Some(touch.device_id);
        self.last_input = Instant::now();
        let at = (
            touch.location.x as f32 / scale,
            touch.location.y as f32 / scale,
        );
        if self.touch_editor_open() {
            self.touch_editor_touch(touch.id, touch.phase, at);
            return;
        }
        let items = self.touch_items(&layout);
        for output in self.touch.controls.configure(self.touch_geometry(&items)) {
            self.apply_touch(event_loop, output, scale);
        }
        let playing = self.touch_playing();
        if self.touch.menu_fingers.contains(&touch.id) {
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                self.touch.menu_fingers.remove(&touch.id);
            }
            return;
        }
        #[cfg(feature = "mods")]
        if self.mod_touch_event(&touch, at, &layout, playing, &items) {
            return;
        }
        if !playing && touch.phase == TouchPhase::Started {
            if self.title.is_some() && super::account::hit(&layout, &self.gui.atlas, at) {
                super::account::tapped();
                return;
            }
            if inside(menu_button(&layout, MenuButton::Keyboard), at) {
                self.touch.controls.used = true;
                self.touch.menu_fingers.insert(touch.id);
                let shown = !self.touch.keyboard;
                self.show_keyboard(shown);
                return;
            }
            if self.touch_back_shown() && inside(menu_button(&layout, MenuButton::Back), at) {
                self.touch.menu_fingers.insert(touch.id);
                self.apply_touch(event_loop, Output::Key(KeyCode::Escape, true), scale);
                self.apply_touch(event_loop, Output::Key(KeyCode::Escape, false), scale);
                return;
            }
        }
        #[cfg(feature = "mods")]
        if self.screen == Screen::Mod && self.mods.canvas_screen.is_some() {
            self.touch.controls.used = true;
            for output in self.touch.controls.release_all() {
                self.apply_touch(event_loop, output, scale);
            }
            self.mod_canvas_touch(touch.id, touch.phase, at);
            return;
        }
        #[cfg(feature = "mods")]
        if self.screen == Screen::Inventory && self.mods.mods.wants_container() {
            // Like canvas screens, this surface owns its gesture recognizer.
            // Release fingers left on the world's controls when it opened.
            for output in self.touch.controls.release_all() {
                self.apply_touch(event_loop, output, scale);
            }
        }
        #[cfg(feature = "mods")]
        if self.mod_container_touch(event_loop, &touch, at, scale) {
            self.touch.controls.used = true;
            return;
        }
        let outputs =
            self.touch
                .controls
                .touch(touch.id, touch.phase, at, &layout, playing, Instant::now());
        for output in outputs {
            self.apply_touch(event_loop, output, scale);
        }
        // Back in the world: the keyboard goes away with the menu that wanted it.
        if playing && self.touch.keyboard {
            self.show_keyboard(false);
        }
    }

    /// Once a frame: held fingers start breaking or right click.
    pub(super) fn touch_update(&mut self, event_loop: &dyn EventLoopCtl) {
        #[cfg(feature = "mods")]
        if !self.mod_touch_active() || !self.touch_playing() {
            self.release_mod_touches();
        }
        if !enabled() || !self.touch.controls.used {
            return;
        }
        let Some((layout, scale)) = self.touch_layout() else {
            return;
        };
        let items = self.touch_items(&layout);
        for output in self.touch.controls.configure(self.touch_geometry(&items)) {
            self.apply_touch(event_loop, output, scale);
        }
        #[cfg(feature = "mods")]
        for (owner, key) in self.touch.mod_fingers.values() {
            if items.iter().any(|i| i.hidden && matches!(&i.kind, TouchKind::Mod { owner: o, key: k, .. } if o == owner && k == key)) {
                self.mods.mods.registered_touch_key(*owner, key, false);
            }
        }
        for output in self.touch.controls.update(Instant::now()) {
            self.apply_touch(event_loop, output, scale);
        }
        // The chat and text screens bring the keyboard up by themselves.
        let wants_keyboard = self.screen == Screen::Chat
            || self.sign_edit.is_some()
            || self.screen == Screen::CommandBlock;
        if wants_keyboard && !self.touch.keyboard {
            self.show_keyboard(true);
        }
    }

    /// Lets go of every finger: the app went to the background.
    pub(super) fn touch_release_all(&mut self, event_loop: &dyn EventLoopCtl) {
        self.cancel_touch_editor_pointer();
        #[cfg(feature = "mods")]
        {
            self.release_mod_touches();
            self.release_canvas_touch();
        }
        self.touch.menu_fingers.clear();
        let Some((_, scale)) = self.touch_layout() else {
            return;
        };
        for output in self.touch.controls.release_all() {
            self.apply_touch(event_loop, output, scale);
        }
    }

    /// A controller is being used: the on-screen controls go away (letting go of every finger)
    /// until the next touch brings them back.
    pub(super) fn touch_hide(&mut self, event_loop: &dyn EventLoopCtl) {
        if !self.touch.controls.used {
            return;
        }
        self.touch_release_all(event_loop);
        self.touch.controls.used = false;
    }

    fn show_keyboard(&mut self, shown: bool) {
        self.touch.keyboard = shown;
        if let Some(window) = &self.window {
            window.set_ime_allowed(shown);
        }
    }

    pub(super) fn apply_touch(
        &mut self,
        event_loop: &dyn EventLoopCtl,
        output: Output,
        scale: f32,
    ) {
        let result = match output {
            Output::Key(key, true) => self.control_key_down(key),
            Output::Key(key, false) => {
                self.keys.remove(&key);
                self.physical_keys.remove(&key);
                Ok(())
            }
            Output::Mouse(button, pressed) => self.touch_mouse(button, pressed),
            Output::Use => {
                // Bedrock's tap: attack what is in reach under the crosshair, else use.
                let button = if self.pick_entity_hit().is_some() {
                    MouseButton::Left
                } else {
                    MouseButton::Right
                };
                self.touch_mouse(button, true)
                    .and_then(|()| self.touch_mouse(button, false))
            }
            Output::Look(dx, dy) => {
                let s = f64::from(scale) * LOOK_SCALE;
                self.mouse
                    .on_move(0.0, 0.0, f64::from(dx) * s, f64::from(dy) * s, true, true);
                Ok(())
            }
            Output::Hotbar(slot) => {
                let key = HOTBAR_KEYS[slot];
                let pressed = self.control_key_down(key);
                self.keys.remove(&key);
                self.physical_keys.remove(&key);
                pressed
            }
            Output::Pointer(..)
            | Output::PointerButton(_)
            | Output::PointerRight
            | Output::Scroll(_) => {
                let (Some(device), Some(id)) =
                    (self.touch.device, self.window.as_ref().map(|w| w.id()))
                else {
                    return;
                };
                for event in pointer_event(device, output, scale) {
                    self.handle_window_event(
                        event_loop,
                        id,
                        event,
                        super::input_trace::InputOrigin::User,
                    );
                }
                Ok(())
            }
        };
        if let Err(e) = result {
            log::warn!("Horizon: touch input: {e}");
        }
    }

    /// The world's mouse button, as `horizon-ctl mouse` presses it.
    fn touch_mouse(&mut self, button: MouseButton, pressed: bool) -> horizon_client::Result<()> {
        if self.screen != Screen::Playing {
            return Ok(());
        }
        self.extras_mouse(button, pressed);
        self.mod_mouse_key(button, pressed);
        self.play_mouse(button, pressed)
    }

    /// The buttons over the HUD, once a finger has touched the screen.
    pub(super) fn draw_touch_controls(&self, batch: &mut HudBatch) {
        if !enabled() || self.touch_editor_open() {
            return;
        }
        let layout = Layout {
            width: batch.width,
            height: batch.height,
            main_hand_left: self.settings.main_hand_left,
        };
        let atlas = &self.gui.atlas;
        if self.title.is_some() {
            super::account::draw(batch, atlas, &layout);
        }
        if !self.touch.controls.used && !forced() {
            return;
        }
        let button = |batch: &mut HudBatch, rect: [f32; 4], label: &str, down: bool| {
            draw_button(batch, atlas, rect, label, down)
        };
        if !self.touch_playing() {
            button(
                batch,
                menu_button(&layout, MenuButton::Keyboard),
                &lang::get_or("horizon.touch.keyboard", "Aa"),
                self.touch.keyboard,
            );
            if self.touch_back_shown() {
                button(
                    batch,
                    menu_button(&layout, MenuButton::Back),
                    &lang::get_or("horizon.touch.back", "Back"),
                    false,
                );
            }
            return;
        }
        for item in self.touch_items(&layout).iter().filter(|item| !item.hidden) {
            self.draw_touch_item(batch, item, &layout, false);
        }
    }
    fn draw_touch_item(
        &self,
        batch: &mut HudBatch,
        item: &TouchItem,
        layout: &Layout,
        editing: bool,
    ) {
        let Some(rect) = item.rect else { return };
        let atlas = &self.gui.atlas;
        match &item.kind {
            TouchKind::Game(b) => {
                let frame = if *b == Button::Inventory {
                    TouchFrame::Offhand(layout.main_hand_left)
                } else {
                    SLOT_FRAME
                };
                draw_button_frame(
                    batch,
                    atlas,
                    rect,
                    frame,
                    !editing && self.touch.controls.touching(*b),
                );
                if *b == Button::Pause {
                    draw_pause(batch, atlas, rect);
                } else {
                    let label_rect = if *b == Button::Inventory {
                        [
                            rect[0] + if layout.main_hand_left { -1.0 } else { 6.0 },
                            rect[1],
                            24.0,
                            24.0,
                        ]
                    } else {
                        rect
                    };
                    draw_button_label(batch, atlas, label_rect, &item.label);
                }
                draw_button_selection(
                    batch,
                    atlas,
                    rect,
                    frame,
                    !editing && self.touch.controls.latched(*b),
                );
            }
            TouchKind::Stick => {
                let centre = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
                let (origin, at) = if editing {
                    (centre, centre)
                } else {
                    self.touch.controls.stick().unwrap_or((centre, centre))
                };
                let r = rect[2] / 2.0;
                draw_button_frame(
                    batch,
                    atlas,
                    [origin.0 - r, origin.1 - r, 2.0 * r, 2.0 * r],
                    TouchFrame::Slot(0x80FFFFFF),
                    false,
                );
                let (dx, dy) = (at.0 - origin.0, at.1 - origin.1);
                let length = dx.hypot(dy).max(1.0);
                let k = length.min(r) / length;
                draw_button_frame(
                    batch,
                    atlas,
                    [origin.0 + dx * k - 7.0, origin.1 + dy * k - 7.0, 14.0, 14.0],
                    TouchFrame::Slot(0xCCFFFFFF),
                    false,
                );
            }
            #[cfg(feature = "mods")]
            TouchKind::Mod { button, down, .. } => {
                if let Some(b) = button {
                    self.draw_mod_touch_button(batch, b, !editing && *down, rect, editing);
                } else {
                    draw_button(batch, atlas, rect, &item.label, !editing && *down);
                }
            }
        }
    }
    #[cfg(feature = "mods")]
    fn draw_mod_touch_button(
        &self,
        batch: &mut HudBatch,
        b: &horizon_mods::TouchButton,
        down: bool,
        rect: Rect,
        editing: bool,
    ) {
        use horizon_client::items::ItemStack;
        let atlas = &self.gui.atlas;
        let [mut x, mut y, mut w, mut h] = rect;
        let small = b.size < 20.0;
        if small {
            x += 3.0;
            y += 3.0;
            w -= 6.0;
            h -= 6.0;
        }
        let frame = if small {
            TouchFrame::SmallPlate
        } else {
            SLOT_FRAME
        };
        draw_button_frame(batch, atlas, [x, y, w, h], frame, down);
        if let Some(id) = b
            .icon
            .as_deref()
            .filter(|id| self.res.items().get(id).is_some())
        {
            let (ix, iy) = (x + (w - 16.0) / 2.0, y + (h - 16.0) / 2.0);
            self.draw_item_model(batch, &ItemStack::new(id, 1), ix, iy);
            if !small && !b.lit {
                batch.fill(atlas, ix, iy, 16.0, 16.0, 0x60000000);
            }
        } else {
            draw_button_label(batch, atlas, [x, y, w, h], &b.label);
        }
        draw_button_selection(batch, atlas, [x, y, w, h], frame, b.lit && !editing);
    }
    #[cfg(feature = "mods")]
    fn mod_touch_event(
        &mut self,
        touch: &Touch,
        at: (f32, f32),
        layout: &Layout,
        playing: bool,
        items: &[TouchItem],
    ) -> bool {
        if self.touch.mod_fingers.contains_key(&touch.id) {
            if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
                let (owner, name) = self.touch.mod_fingers.remove(&touch.id).unwrap();
                if !self
                    .touch
                    .mod_fingers
                    .values()
                    .any(|f| f == &(owner, name.clone()))
                {
                    self.mods.mods.registered_touch_key(owner, &name, false);
                }
            }
            return true;
        }
        if playing && touch.phase == TouchPhase::Started {
            let button = items
                .iter()
                .rev()
                .filter(|item| !item.hidden)
                .find_map(|item| {
                    let TouchKind::Mod { owner, key, .. } = &item.kind else {
                        return None;
                    };
                    let rect = mod_hit_rect(item)?;
                    touch_layout::contains(rect, at).then(|| (*owner, key.clone()))
                });
            let hit = button.or_else(|| {
                let geometry = self.touch_geometry(items);
                if geometry.button_at(at).is_some()
                    || layout.hotbar_slot(at.0, at.1).is_some()
                    || geometry
                        .stick
                        .is_some_and(|r| touch_layout::contains(r, at))
                {
                    return None;
                }
                self.mod_canvas_box_hit(at, (layout.width, layout.height))
            });
            if let Some((owner, name)) = hit {
                self.touch.controls.used = true;
                self.mods.mods.registered_touch_key(owner, &name, true);
                self.touch.mod_fingers.insert(touch.id, (owner, name));
                return true;
            }
        }
        false
    }
    #[cfg(feature = "mods")]
    fn release_mod_touches(&mut self) {
        for (_, (owner, name)) in self.touch.mod_fingers.drain() {
            self.mods.mods.registered_touch_key(owner, &name, false);
        }
    }
}

#[cfg(feature = "mods")]
fn touch_rect(b: &horizon_mods::TouchButton, layout: &Layout) -> [f32; 4] {
    use horizon_mods::RenderCorner as C;
    let size = b.size.min(layout.width).min(layout.height).max(0.0);
    // Reserve the small variant's outer ring even while off, so toggling never moves it.
    let inset = if size < 20.0 {
        3.0_f32.min(layout.width / 2.0).min(layout.height / 2.0)
    } else {
        0.0
    };
    let size = size
        .min((layout.width - 2.0 * inset).max(0.0))
        .min((layout.height - 2.0 * inset).max(0.0));
    let x = match b.anchor {
        C::TopRight | C::BottomRight => layout.width - b.dx - size,
        _ => b.dx,
    };
    let y = match b.anchor {
        C::BottomLeft | C::BottomRight => layout.height - b.dy - size,
        _ => b.dy,
    };
    [
        x.clamp(inset, (layout.width - size - inset).max(inset)),
        y.clamp(inset, (layout.height - size - inset).max(inset)),
        size,
        size,
    ]
}

#[cfg(feature = "mods")]
fn mod_hit_rect(item: &TouchItem) -> Option<Rect> {
    let [x, y, w, h] = item.rect?;
    let (hw, hh) = (w.max(24.0), h.max(24.0));
    Some([x - (hw - w) / 2.0, y - (hh - h) / 2.0, hw, hh])
}

#[cfg(all(test, feature = "mods"))]
fn touch_hit_rect(b: &horizon_mods::TouchButton, layout: &Layout) -> [f32; 4] {
    let [x, y, w, h] = touch_rect(b, layout);
    let (hw, hh) = (w.max(24.0), h.max(24.0));
    [x - (hw - w) / 2.0, y - (hh - h) / 2.0, hw, hh]
}

#[cfg(all(test, feature = "mods"))]
mod mod_button_tests {
    use super::*;
    use horizon_mods::{RenderCorner, TouchButton};
    #[test]
    fn mod_touch_buttons_anchor_inward_and_stay_fully_on_screen() {
        let layout = Layout {
            width: 320.0,
            height: 180.0,
            main_hand_left: false,
        };
        for (anchor, expected) in [
            (RenderCorner::TopLeft, [10.0, 20.0, 24.0, 24.0]),
            (RenderCorner::TopRight, [286.0, 20.0, 24.0, 24.0]),
            (RenderCorner::BottomLeft, [10.0, 136.0, 24.0, 24.0]),
            (RenderCorner::BottomRight, [286.0, 136.0, 24.0, 24.0]),
        ] {
            let mut b = TouchButton {
                label: "Mine".into(),
                icon: None,
                anchor,
                dx: 10.0,
                dy: 20.0,
                size: 24.0,
                lit: false,
            };
            assert_eq!(touch_rect(&b, &layout), expected);
            b.dx = 1000.0;
            b.dy = -1000.0;
            let [x, y, w, h] = touch_rect(&b, &layout);
            assert!(x >= 0.0 && y >= 0.0 && x + w <= layout.width && y + h <= layout.height);
        }
    }
    #[test]
    fn mod_touch_small_ring_stays_on_screen_and_hit_boxes_stay_centered() {
        let layout = Layout {
            width: 320.0,
            height: 180.0,
            main_hand_left: false,
        };
        let mut b = TouchButton {
            label: "VM".into(),
            icon: Some("minecraft:diamond_pickaxe".into()),
            anchor: RenderCorner::TopLeft,
            dx: 0.0,
            dy: 0.0,
            size: 16.0,
            lit: true,
        };
        assert_eq!(touch_rect(&b, &layout), [3.0, 3.0, 16.0, 16.0]);
        assert_eq!(touch_hit_rect(&b, &layout), [-1.0, -1.0, 24.0, 24.0]);
        b.anchor = RenderCorner::BottomRight;
        for (size, visual, hit) in [
            (16.0, [301.0, 161.0, 16.0, 16.0], [297.0, 157.0, 24.0, 24.0]),
            (20.0, [300.0, 160.0, 20.0, 20.0], [298.0, 158.0, 24.0, 24.0]),
            (28.0, [292.0, 152.0, 28.0, 28.0], [292.0, 152.0, 28.0, 28.0]),
        ] {
            b.size = size;
            assert_eq!(touch_rect(&b, &layout), visual);
            assert_eq!(touch_hit_rect(&b, &layout), hit);
            b.lit = false;
            assert_eq!(touch_rect(&b, &layout), visual);
            b.lit = true;
        }
        b.size = 16.0;
        b.dx = 1000.0;
        b.dy = -1000.0;
        for anchor in [
            RenderCorner::TopLeft,
            RenderCorner::TopRight,
            RenderCorner::BottomLeft,
            RenderCorner::BottomRight,
        ] {
            b.anchor = anchor;
            let [x, y, w, h] = touch_rect(&b, &layout);
            assert!(x >= 3.0 && y >= 3.0);
            assert!(x + w + 3.0 <= layout.width && y + h + 3.0 <= layout.height);
        }
    }
}

#[cfg(test)]
mod hold_mapping_tests {
    use super::*;

    #[test]
    fn touch_sprint_reports_its_mapping_held_until_lift() {
        let mut input = TouchInput::default();
        let layout = Layout {
            width: 633.0,
            height: 292.0,
            main_hand_left: false,
        };
        let r = layout.button(Button::Sprint);
        let at = (r[0] + 10.0, r[1] + 10.0);
        let now = Instant::now();
        input
            .controls
            .touch(1, TouchPhase::Started, at, &layout, true, now);
        assert!(input.holds_mapping("key.sprint"));
        assert!(!input.holds_mapping("key.sneak"));
        input
            .controls
            .touch(1, TouchPhase::Ended, at, &layout, true, now);
        assert!(!input.holds_mapping("key.sprint"));
    }
}
