//! Touch controls in the game (`touch_controls.rs` turns fingers into [`Output`]s): applying
//! them as the keys, clicks and look movement a keyboard and mouse would give, and drawing
//! the buttons over the HUD. On iOS only: desktop touchscreens keep winit's own behaviour.
use super::{Game, Screen};
use horizon_client::{
    hud::{HudAtlas, HudBatch, rgba},
    osk,
    touch_controls::{Button, Layout, Output, TouchControls},
    ui_theme::Ui,
    widgets::{self, Kind, Target},
};
use std::{collections::HashSet, time::Instant};
use winit::{
    application::ApplicationHandler,
    event::{
        DeviceId, ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
    },
    event_loop::ActiveEventLoop,
    keyboard::KeyCode,
};

/// Touch turns the view this many window pixels' worth of mouse movement per pixel dragged.
const LOOK_SCALE: f64 = 0.6;

/// Whether this system's touches drive the touch controls.
pub(super) const ENABLED: bool = cfg!(target_os = "ios");

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
        },
        batch.scale,
    )
}

/// The window event a menu pointer output stands for, on `device`. `None` for the world's
/// outputs.
fn pointer_event(device: DeviceId, output: Output, scale: f32) -> Option<WindowEvent> {
    Some(match output {
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
        Output::Scroll(lines) => WindowEvent::MouseWheel {
            device_id: device,
            delta: MouseScrollDelta::LineDelta(0.0, lines),
            phase: TouchPhase::Moved,
        },
        _ => return None,
    })
}

/// A touch button: a dark box (light while pressed) with a thin edge and its label.
pub(super) fn draw_button(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    rect: [f32; 4],
    label: &str,
    down: bool,
) {
    let [x, y, w, h] = rect;
    let fill = if down {
        rgba(255, 255, 255, 0x70)
    } else {
        rgba(0, 0, 0, 0x60)
    };
    batch.fill(atlas, x, y, w, h, fill);
    let edge = rgba(255, 255, 255, 0x50);
    batch.fill(atlas, x, y, w, 1.0, edge);
    batch.fill(atlas, x, y + h - 1.0, w, 1.0, edge);
    batch.fill(atlas, x, y, 1.0, h, edge);
    batch.fill(atlas, x + w - 1.0, y, 1.0, h, edge);
    let tw = HudBatch::text_width(atlas, label);
    batch.text(
        atlas,
        label,
        (x + (w - tw) / 2.0).round(),
        (y + (h - 8.0) / 2.0).round(),
        u32::MAX,
        true,
    );
}

/// Text fields registered by the same drawers controller focus uses.
pub(super) fn field_at(at: (f32, f32)) -> Option<Target> {
    widgets::last_targets()
        .into_iter()
        .rev()
        .find(|t| t.kind == Kind::Field && inside(t.rect, at))
}

/// Touch owns its visibility so controller polling/settings cannot reset it.
/// Captured fingers stay captured through their release, even if Done closed the screen.
#[derive(Default)]
struct TouchKeyboard {
    keyboard: Option<osk::Keyboard>,
    at_top: bool,
    captured: HashSet<u64>,
}

impl TouchKeyboard {
    fn open(&mut self, at_top: bool) {
        self.keyboard = Some(osk::Keyboard::default());
        self.at_top = at_top;
    }

    fn captured(&mut self, touch: &Touch) -> bool {
        if matches!(touch.phase, TouchPhase::Ended | TouchPhase::Cancelled) {
            self.captured.remove(&touch.id)
        } else {
            self.captured.contains(&touch.id)
        }
    }

    fn tap(&mut self, touch: &Touch, size: [u32; 2], atlas: &HudAtlas) -> Option<osk::Output> {
        let keyboard = self.keyboard.as_mut()?;
        if touch.phase != TouchPhase::Started {
            return None;
        }
        self.captured.insert(touch.id);
        let mut batch = HudBatch::new(size);
        let at = (
            touch.location.x as f32 / batch.scale,
            touch.location.y as f32 / batch.scale,
        );
        let ui = Ui::new(atlas, &mut batch, at);
        keyboard.tap([ui.w, ui.h], self.at_top, ui.cursor)
    }

    fn draw(&self, batch: &mut HudBatch, atlas: &HudAtlas) {
        if let Some(keyboard) = &self.keyboard {
            let mut ui = Ui::new(atlas, batch, (0.0, 0.0));
            keyboard.draw_touch(&mut ui, self.at_top, Default::default());
            ui.finish();
        }
    }
}

/// Touch on the startup menu (the title screen shown while the world's resources load, before
/// `Game` exists): the first finger is the pointer, and the keyboard button shows the keyboard
/// for its text fields.
#[derive(Default)]
pub(super) struct MenuTouch {
    controls: TouchControls,
    device: Option<DeviceId>,
    keyboard: TouchKeyboard,
    wanted_keyboard: bool,
    cursor: (f32, f32),
}

/// What a touch on the startup menu does.
pub(super) enum MenuTouchAction {
    /// Pointer events for the menu.
    Events(Vec<WindowEvent>),
    /// The keyboard button: show or hide the keyboard.
    ToggleKeyboard,
    /// A key on Horizon's keyboard (or an absorbed touch).
    Keyboard(Option<osk::Output>),
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
        self.controls.used = true;
        self.device = Some(touch.device_id);
        if self.keyboard.captured(&touch) {
            return MenuTouchAction::Keyboard(None);
        }
        let at = (
            touch.location.x as f32 / scale,
            touch.location.y as f32 / scale,
        );
        if touch.phase == TouchPhase::Started
            && inside(menu_button(&layout, MenuButton::Keyboard), at)
        {
            self.keyboard.captured.insert(touch.id);
            return MenuTouchAction::ToggleKeyboard;
        }
        if self.keyboard.keyboard.is_some() {
            return MenuTouchAction::Keyboard(
                atlas.and_then(|a| self.keyboard.tap(&touch, size, a)),
            );
        }
        if touch.phase == TouchPhase::Started
            && atlas.is_some_and(|atlas| super::account::hit(&layout, atlas, at))
        {
            self.keyboard.captured.insert(touch.id);
            return MenuTouchAction::Account;
        }
        self.cursor = at;
        let mut outputs =
            self.controls
                .touch(touch.id, touch.phase, at, &layout, false, Instant::now());
        self.open_touched_field(&mut outputs, layout.height);
        MenuTouchAction::Events(self.events(outputs, scale))
    }

    /// Once a frame: a finger held still starts a drag.
    pub(super) fn update(&mut self, size: [u32; 2]) -> Vec<WindowEvent> {
        if !self.controls.used || self.keyboard.keyboard.is_some() {
            return Vec::new();
        }
        let (layout, scale) = layout_of(size);
        let mut outputs = self.controls.update(Instant::now());
        self.open_touched_field(&mut outputs, layout.height);
        self.events(outputs, scale)
    }

    fn open_touched_field(&mut self, outputs: &mut Vec<Output>, height: f32) {
        if outputs
            .iter()
            .any(|o| matches!(o, Output::PointerButton(true)))
            && let Some(field) = field_at(self.cursor)
        {
            self.keyboard.open(field.center().1 > height / 2.0);
            self.wanted_keyboard = true;
            outputs.extend(self.controls.release_all());
        }
    }

    fn events(&self, outputs: Vec<Output>, scale: f32) -> Vec<WindowEvent> {
        let Some(device) = self.device else {
            return Vec::new();
        };
        outputs
            .into_iter()
            .filter_map(|o| pointer_event(device, o, scale))
            .collect()
    }

    pub(super) fn auto_keyboard(
        &mut self,
        wants: bool,
        at_top: bool,
        size: [u32; 2],
    ) -> Vec<WindowEvent> {
        if !self.controls.used || self.wanted_keyboard == wants {
            return Vec::new();
        }
        self.wanted_keyboard = wants;
        if wants {
            self.keyboard.at_top = at_top;
            self.open_keyboard(size)
        } else {
            self.close_keyboard();
            Vec::new()
        }
    }

    pub(super) fn close_keyboard(&mut self) {
        self.keyboard.keyboard = None;
    }

    pub(super) fn toggle_keyboard(&mut self, size: [u32; 2], wants: bool) -> Vec<WindowEvent> {
        self.wanted_keyboard = wants;
        if self.keyboard.keyboard.is_some() {
            self.close_keyboard();
            Vec::new()
        } else {
            self.open_keyboard(size)
        }
    }

    fn open_keyboard(&mut self, size: [u32; 2]) -> Vec<WindowEvent> {
        let (_, scale) = layout_of(size);
        self.keyboard.open(self.keyboard.at_top);
        let outputs = self.controls.release_all();
        self.events(outputs, scale)
    }

    /// The account button, and the keyboard button once a finger has touched the screen.
    pub(super) fn draw(&self, batch: &mut HudBatch, atlas: &HudAtlas) {
        if !ENABLED {
            return;
        }
        let layout = Layout {
            width: batch.width,
            height: batch.height,
        };
        super::account::draw(batch, atlas, &layout);
        if !self.controls.used {
            return;
        }
        self.keyboard.draw(batch, atlas);
        draw_button(
            batch,
            atlas,
            menu_button(&layout, MenuButton::Keyboard),
            "Aa",
            self.keyboard.keyboard.is_some(),
        );
    }
}

#[derive(Default)]
pub(super) struct TouchInput {
    controls: TouchControls,
    /// The touchscreen, for the pointer events the menus get.
    device: Option<DeviceId>,
    keyboard: TouchKeyboard,
    /// Opening a text screen shows the keyboard once. Closing it keeps it closed
    /// until another field/screen is selected or Aa is tapped.
    keyboard_context: Option<(Screen, bool, bool, bool, bool)>,
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
    /// The world is on screen with nothing over it: the controls act on it.
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
        Some(layout_of(size))
    }

    /// Keep typing, dismissal and captured fingers when startup resources finish loading.
    pub(super) fn take_menu_touch(&mut self, menu: &mut MenuTouch) {
        self.touch = TouchInput {
            controls: std::mem::take(&mut menu.controls),
            device: menu.device,
            keyboard: std::mem::take(&mut menu.keyboard),
            keyboard_context: Some(self.touch_keyboard_context()),
        };
    }

    /// `WindowEvent::Touch`.
    pub(super) fn touch_event(&mut self, event_loop: &ActiveEventLoop, touch: Touch) {
        if self.pending_save.is_some() {
            return;
        }
        let Some((layout, scale)) = self.touch_layout() else {
            return;
        };
        self.touch.device = Some(touch.device_id);
        self.touch.controls.used = true;
        self.last_input = Instant::now();
        if self.touch.keyboard.captured(&touch) {
            return;
        }
        let at = (
            touch.location.x as f32 / scale,
            touch.location.y as f32 / scale,
        );
        self.pad.in_use = false;
        if self.touch.keyboard.keyboard.is_none()
            && let Some(keyboard) = self.pad.menu.osk.take()
        {
            self.touch.keyboard.keyboard = Some(keyboard);
            self.touch.keyboard.at_top =
                self.screen == Screen::Chat || self.cursor.1 > layout.height / 2.0;
            self.touch.keyboard_context = Some(self.touch_keyboard_context());
        }
        let playing = self.touch_playing();
        if !playing
            && touch.phase == TouchPhase::Started
            && inside(menu_button(&layout, MenuButton::Keyboard), at)
        {
            self.touch.keyboard.captured.insert(touch.id);
            let shown = self.touch.keyboard.keyboard.is_none();
            self.show_keyboard(
                event_loop,
                shown,
                self.screen == Screen::Chat || self.touch.keyboard.at_top,
            );
            return;
        }
        if self.touch.keyboard.keyboard.is_some() {
            let size = [
                (layout.width * scale) as u32,
                (layout.height * scale) as u32,
            ];
            if let Some(out) = self.touch.keyboard.tap(&touch, size, &self.gui.atlas)
                && self.osk_output(out)
            {
                self.touch.keyboard.keyboard = None;
            }
            return;
        }
        if !playing && touch.phase == TouchPhase::Started {
            if self.title.is_some() && super::account::hit(&layout, &self.gui.atlas, at) {
                super::account::tapped();
                return;
            }
            if self.touch_back_shown() && inside(menu_button(&layout, MenuButton::Back), at) {
                self.touch.keyboard.captured.insert(touch.id);
                self.apply_touch(event_loop, Output::Key(KeyCode::Escape, true), scale);
                self.apply_touch(event_loop, Output::Key(KeyCode::Escape, false), scale);
                return;
            }
        }
        let outputs =
            self.touch
                .controls
                .touch(touch.id, touch.phase, at, &layout, playing, Instant::now());
        let clicked = outputs
            .iter()
            .any(|o| matches!(o, Output::PointerButton(true)));
        let field = clicked.then(|| field_at(at)).flatten();
        for output in outputs {
            self.apply_touch(event_loop, output, scale);
        }
        self.touch_auto_keyboard(event_loop);
        if let Some(field) = field {
            self.show_keyboard(event_loop, true, field.center().1 > layout.height / 2.0);
        }
    }

    /// Once a frame: held fingers start breaking or dragging.
    pub(super) fn touch_update(&mut self, event_loop: &ActiveEventLoop) {
        if !ENABLED || !self.touch.controls.used {
            return;
        }
        let Some((_, scale)) = self.touch_layout() else {
            return;
        };
        if self.touch.keyboard.keyboard.is_none() {
            let outputs = self.touch.controls.update(Instant::now());
            let clicked = outputs
                .iter()
                .any(|o| matches!(o, Output::PointerButton(true)));
            for output in outputs {
                self.apply_touch(event_loop, output, scale);
            }
            if clicked && let Some(field) = field_at(self.cursor) {
                let height = self.touch_layout().map_or(0.0, |(l, _)| l.height);
                self.show_keyboard(event_loop, true, field.center().1 > height / 2.0);
            }
        }
        self.touch_auto_keyboard(event_loop);
    }

    fn touch_auto_keyboard(&mut self, event_loop: &ActiveEventLoop) {
        let context = self.touch_keyboard_context();
        let wants = context.4;
        if self.touch.keyboard_context != Some(context) {
            self.touch.keyboard_context = Some(context);
            self.show_keyboard(event_loop, wants, self.screen == Screen::Chat);
        }
    }

    fn touch_keyboard_context(&mut self) -> (Screen, bool, bool, bool, bool) {
        let wants = self.pad_typing()
            || if self.screen == Screen::Options {
                self.ui.options.typing()
            } else {
                self.title.as_mut().is_some_and(|t| t.has_caret_field())
            };
        (
            self.screen,
            self.sign_edit.is_some(),
            self.book.is_some(),
            self.dialog_open(),
            wants,
        )
    }

    /// Lets go of every finger: the app went to the background.
    #[cfg(target_os = "ios")]
    pub(super) fn touch_release_all(&mut self, event_loop: &ActiveEventLoop) {
        let Some((_, scale)) = self.touch_layout() else {
            return;
        };
        self.touch.keyboard.captured.clear();
        for output in self.touch.controls.release_all() {
            self.apply_touch(event_loop, output, scale);
        }
    }

    /// A controller is being used: the on-screen controls go away (letting go of every finger)
    /// until the next touch brings them back.
    #[cfg(target_os = "ios")]
    pub(super) fn touch_hide(&mut self, event_loop: &ActiveEventLoop) {
        if !self.touch.controls.used {
            return;
        }
        self.touch_release_all(event_loop);
        self.touch.controls.used = false;
        if let Some(keyboard) = self.touch.keyboard.keyboard.take() {
            self.pad.menu.osk = Some(keyboard);
        }
    }

    fn show_keyboard(&mut self, event_loop: &ActiveEventLoop, shown: bool, at_top: bool) {
        self.touch.keyboard_context = Some(self.touch_keyboard_context());
        if let Some(window) = &self.window {
            window.set_ime_allowed(false);
        }
        if shown {
            let at_top = at_top
                || self.touch_layout().is_some_and(|(layout, _)| {
                    self.screen != Screen::Options
                        && self
                            .title
                            .as_ref()
                            .and_then(|t| t.focused_rect((layout.width, layout.height)))
                            .is_some_and(|[_, y, _, h]| y + h / 2.0 > layout.height / 2.0)
                });
            self.touch.keyboard.open(at_top);
            if let Some((_, scale)) = self.touch_layout() {
                for output in self.touch.controls.release_all() {
                    self.apply_touch(event_loop, output, scale);
                }
            }
        } else {
            self.touch.keyboard.keyboard = None;
        }
    }

    fn apply_touch(&mut self, event_loop: &ActiveEventLoop, output: Output, scale: f32) {
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
            Output::Pointer(..) | Output::PointerButton(_) | Output::Scroll(_) => {
                let (Some(device), Some(id)) =
                    (self.touch.device, self.window.as_ref().map(|w| w.id()))
                else {
                    return;
                };
                if let Some(event) = pointer_event(device, output, scale) {
                    self.window_event(event_loop, id, event);
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
        if !ENABLED {
            return;
        }
        let layout = Layout {
            width: batch.width,
            height: batch.height,
        };
        let atlas = &self.gui.atlas;
        if self.title.is_some() {
            super::account::draw(batch, atlas, &layout);
        }
        if !self.touch.controls.used {
            return;
        }
        let button = |batch: &mut HudBatch, rect: [f32; 4], label: &str, down: bool| {
            draw_button(batch, atlas, rect, label, down)
        };
        if !self.touch_playing() {
            self.touch.keyboard.draw(batch, atlas);
            button(
                batch,
                menu_button(&layout, MenuButton::Keyboard),
                "Aa",
                self.touch.keyboard.keyboard.is_some(),
            );
            if self.touch_back_shown() {
                button(batch, menu_button(&layout, MenuButton::Back), "Back", false);
            }
            return;
        }
        for b in Button::ALL {
            button(
                batch,
                layout.button(b),
                b.label(),
                self.touch.controls.pressed(b),
            );
        }
        // The joystick: its ring where the finger landed, or at rest.
        let (origin, at) = self
            .touch
            .controls
            .stick()
            .unwrap_or_else(|| (layout.stick_rest(), layout.stick_rest()));
        let r = horizon_client::touch_controls::STICK_RADIUS;
        batch.fill(
            atlas,
            origin.0 - r,
            origin.1 - r,
            r * 2.0,
            r * 2.0,
            rgba(0, 0, 0, 0x40),
        );
        let (dx, dy) = (at.0 - origin.0, at.1 - origin.1);
        let length = dx.hypot(dy).max(1.0);
        let k = length.min(r) / length;
        let knob = (origin.0 + dx * k, origin.1 + dy * k);
        batch.fill(
            atlas,
            knob.0 - 7.0,
            knob.1 - 7.0,
            14.0,
            14.0,
            rgba(255, 255, 255, 0x80),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;

    fn touch(id: u64, phase: TouchPhase, x: f64, y: f64) -> Touch {
        Touch {
            device_id: DeviceId::dummy(),
            phase,
            location: PhysicalPosition::new(x, y),
            force: None,
            id,
        }
    }

    #[test]
    fn keyboard_fingers_stay_captured_after_closing() {
        let mut keyboard = TouchKeyboard::default();
        keyboard.open(false);
        keyboard.captured.extend([1, 2]);
        keyboard.keyboard = None;
        assert!(keyboard.captured(&touch(1, TouchPhase::Moved, 0.0, 0.0)));
        assert!(keyboard.captured(&touch(1, TouchPhase::Ended, 0.0, 0.0)));
        assert!(keyboard.captured(&touch(2, TouchPhase::Cancelled, 0.0, 0.0)));
        assert!(!keyboard.captured(&touch(3, TouchPhase::Started, 0.0, 0.0)));
        assert!(keyboard.captured.is_empty());
    }

    #[test]
    fn auto_keyboard_respects_dismissal_and_aa_toggles() {
        let mut menu = MenuTouch::default();
        menu.controls.used = true;
        let size = [1280, 720];
        menu.auto_keyboard(true, false, size);
        assert!(menu.keyboard.keyboard.is_some());
        menu.close_keyboard();
        menu.auto_keyboard(true, false, size);
        assert!(menu.keyboard.keyboard.is_none(), "Close stays closed");
        menu.toggle_keyboard(size, true);
        assert!(menu.keyboard.keyboard.is_some());
        menu.toggle_keyboard(size, true);
        menu.auto_keyboard(true, false, size);
        assert!(menu.keyboard.keyboard.is_none(), "Aa stays closed");
        menu.auto_keyboard(false, false, size);
        menu.auto_keyboard(true, false, size);
        assert!(
            menu.keyboard.keyboard.is_some(),
            "a new field opens it again"
        );
    }

    #[test]
    fn tapping_a_lower_menu_field_opens_keyboard_above_it() {
        let size = [1280, 720];
        let (layout, scale) = layout_of(size);
        let field = [layout.width / 2.0, layout.height * 0.75, 40.0, 16.0];
        widgets::target_as(field, Kind::Field);
        widgets::end_frame(widgets::generation() + 1);
        let mut menu = MenuTouch::default();
        let (x, y) = (
            f64::from((field[0] + 5.0) * scale),
            f64::from((field[1] + 5.0) * scale),
        );
        menu.touch(touch(1, TouchPhase::Started, x, y), size, None);
        assert!(matches!(
            menu.touch(touch(1, TouchPhase::Ended, x, y), size, None),
            MenuTouchAction::Events(_)
        ));
        assert!(menu.keyboard.keyboard.is_some());
        assert!(menu.keyboard.at_top);
        menu.close_keyboard();
        menu.auto_keyboard(true, false, size);
        assert!(menu.keyboard.keyboard.is_none());
    }

    #[test]
    fn touch_coordinates_match_dp_and_keyboard_touches_do_not_become_pointer_events() {
        let atlas = HudAtlas::fonts(std::path::Path::new("/nonexistent-horizon-osk-test")).unwrap();
        for size in [[1040, 700], [2532, 1170]] {
            for top in [false, true] {
                let mut menu = MenuTouch::default();
                menu.keyboard.open(top);
                let mut batch = HudBatch::new(size);
                let ui = Ui::new(&atlas, &mut batch, (0.0, 0.0));
                // Centre of q, in dp, converted to physical touch coordinates.
                let x = ((ui.w - 554.0) / 2.0).round() + 25.0;
                let y = if top { 16.0 } else { ui.h - 268.0 - 16.0 } + 44.0 + 19.0;
                let (x, y) = (f64::from(x * ui.density), f64::from(y * ui.density));
                assert!(
                    matches!(menu.touch(touch(1, TouchPhase::Started, x, y), size, Some(&atlas)),
                    MenuTouchAction::Keyboard(Some(osk::Output::Text(text))) if text == "q")
                );
                menu.close_keyboard();
                assert!(matches!(
                    menu.touch(touch(1, TouchPhase::Ended, x, y), size, Some(&atlas)),
                    MenuTouchAction::Keyboard(None)
                ));
            }
        }
    }
}
