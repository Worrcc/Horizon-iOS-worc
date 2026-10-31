//! Touch controls in the game (`touch_controls.rs` turns fingers into [`Output`]s): applying
//! them as the keys, clicks and look movement a keyboard and mouse would give, and drawing
//! the buttons over the HUD. On iOS only: desktop touchscreens keep winit's own behaviour.
use super::{Game, Screen};
use horizon_client::{
    hud::{HudAtlas, HudBatch, rgba},
    touch_controls::{Button, Layout, Output, TouchControls},
};
use std::time::Instant;
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

    /// Once a frame: a finger held still starts a drag.
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
            .filter_map(|o| pointer_event(device, o, scale))
            .collect()
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
        draw_button(
            batch,
            atlas,
            menu_button(&layout, MenuButton::Keyboard),
            "Aa",
            self.keyboard,
        );
    }
}

#[derive(Default)]
pub(super) struct TouchInput {
    controls: TouchControls,
    /// The touchscreen, for the pointer events the menus get.
    device: Option<DeviceId>,
    /// The on-screen keyboard is up (`Window::set_ime_allowed`).
    keyboard: bool,
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

    /// `WindowEvent::Touch`.
    pub(super) fn touch_event(&mut self, event_loop: &ActiveEventLoop, touch: Touch) {
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
        let playing = self.touch_playing();
        if !playing && touch.phase == TouchPhase::Started {
            if self.title.is_some() && super::account::hit(&layout, &self.gui.atlas, at) {
                super::account::tapped();
                return;
            }
            if inside(menu_button(&layout, MenuButton::Keyboard), at) {
                let shown = !self.touch.keyboard;
                self.show_keyboard(shown);
                return;
            }
            if self.touch_back_shown() && inside(menu_button(&layout, MenuButton::Back), at) {
                self.apply_touch(event_loop, Output::Key(KeyCode::Escape, true), scale);
                self.apply_touch(event_loop, Output::Key(KeyCode::Escape, false), scale);
                return;
            }
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

    /// Once a frame: held fingers start breaking or dragging.
    pub(super) fn touch_update(&mut self, event_loop: &ActiveEventLoop) {
        if !ENABLED || !self.touch.controls.used {
            return;
        }
        let Some((_, scale)) = self.touch_layout() else {
            return;
        };
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
    #[cfg(target_os = "ios")]
    pub(super) fn touch_release_all(&mut self, event_loop: &ActiveEventLoop) {
        let Some((_, scale)) = self.touch_layout() else {
            return;
        };
        for output in self.touch.controls.release_all() {
            self.apply_touch(event_loop, output, scale);
        }
    }

    fn show_keyboard(&mut self, shown: bool) {
        self.touch.keyboard = shown;
        if let Some(window) = &self.window {
            window.set_ime_allowed(shown);
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
            button(
                batch,
                menu_button(&layout, MenuButton::Keyboard),
                "Aa",
                self.touch.keyboard,
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
