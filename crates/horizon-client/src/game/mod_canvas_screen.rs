//! A mod's full-window canvas: ownership, drawing and GUI input routing.
//! Frame hooks run from RedrawRequested even with menus open; this surface
//! also leaves the server running, like native mods' own screens.
use super::Game;
#[cfg(feature = "mods")]
use {
    super::Screen,
    horizon_client::{hud::HudBatch, keybinds, ui},
    horizon_mods::PointerKind,
    winit::{
        event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
        keyboard::{KeyCode, PhysicalKey},
    },
};

/// What the window keeps while a canvas screen covers the world.
#[cfg(feature = "mods")]
pub(super) struct CanvasScreenHost {
    pub(super) owner: usize,
    pub(super) under: Screen,
    buttons: Vec<u32>,
    shift: bool,
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// Opens a canvas in the same Screen::Mod slot as widget/native screens.
    pub(super) fn open_mod_canvas_screen(&mut self, owner: usize, under: Screen) {
        let under = self.mods.canvas_screen.as_ref().map_or(under, |s| s.under);
        self.mods.canvas_screen = Some(CanvasScreenHost {
            owner,
            under,
            buttons: Vec::new(),
            shift: self.modifiers.shift_key(),
        });
        self.screen = Screen::Mod;
        if self.player.is_some() {
            self.capture(false);
        }
    }
    /// Returns to the covered surface. Only a player's close sends closed.
    pub(super) fn close_mod_canvas_screen(&mut self, tell: bool) -> bool {
        let Some(state) = self.mods.canvas_screen.take() else {
            return false;
        };
        self.screen = state.under;
        if self.screen == Screen::Playing && self.player.is_some() {
            self.capture(true);
        }
        if tell {
            self.mods.mods.ui_closed(state.owner);
            self.show_mod_output();
        }
        true
    }
    /// Dim the world as a menu does, then draw shapes clipped to the window.
    pub(super) fn draw_mod_canvas_screen(&mut self, batch: &mut HudBatch) -> bool {
        let Some(state) = &self.mods.canvas_screen else {
            return false;
        };
        let owner = state.owner;
        let Some(screen) = self.mods.mods.canvas_screen(owner) else {
            self.close_mod_canvas_screen(false);
            return true;
        };
        ui::menu_background(
            batch,
            &self.gui.atlas,
            0.0,
            0.0,
            batch.width,
            batch.height,
            self.player.is_some(),
        );
        let start = batch.vertices.len();
        self.draw_canvas_shapes(batch, owner, &screen.shapes, (0.0, 0.0));
        batch.clip_since(start, 0.0, 0.0, batch.width, batch.height);
        true
    }
    /// Consume screen input before gameplay hotkeys. Touch's synthetic
    /// mouse events take this same path, with GUI coordinates scaled once.
    pub(super) fn mod_canvas_screen_event(&mut self, event: &WindowEvent) -> bool {
        if self.screen != Screen::Mod {
            return false;
        }
        let Some(state) = &mut self.mods.canvas_screen else {
            return false;
        };
        let owner = state.owner;
        let (x, y) = self.cursor;
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                let scale = self.renderer.as_ref().map_or(1.0, |r| {
                    HudBatch::new([r.config.width, r.config.height]).scale
                });
                self.cursor = (position.x as f32 / scale, position.y as f32 / scale);
                let (x, y) = self.cursor;
                let buttons = state.buttons.clone();
                if buttons.is_empty() {
                    self.mods.mods.ui_pointer(owner, PointerKind::Move, x, y, 0);
                } else {
                    for button in buttons {
                        self.mods
                            .mods
                            .ui_pointer(owner, PointerKind::Drag, x, y, button);
                    }
                }
            }
            WindowEvent::MouseInput {
                state: action,
                button,
                ..
            } => {
                let button = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    _ => return true,
                };
                let down = *action == ElementState::Pressed;
                state.buttons.retain(|b| *b != button);
                if down {
                    state.buttons.push(button);
                }
                self.mods.mods.ui_pointer(
                    owner,
                    if down {
                        PointerKind::Down
                    } else {
                        PointerKind::Up
                    },
                    x,
                    y,
                    button,
                );
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 40.0,
                };
                self.mods.mods.ui_scroll(owner, x, y, steps);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(key) = event.physical_key {
                    let down = event.state == ElementState::Pressed;
                    if key == KeyCode::Escape {
                        if down {
                            self.close_wasm_mod_screen(true);
                        }
                        return true;
                    }
                    if matches!(key, KeyCode::ShiftLeft | KeyCode::ShiftRight) {
                        let shift = self.physical_keys.contains(&KeyCode::ShiftLeft)
                            || self.physical_keys.contains(&KeyCode::ShiftRight);
                        if shift != state.shift {
                            state.shift = shift;
                            self.mods
                                .mods
                                .ui_screen_key(owner, "key.keyboard.left.shift", shift);
                        }
                    } else if let Some(key) = keybinds::key_name(key) {
                        self.mods.mods.ui_screen_key(owner, key, down);
                    }
                }
            }
            WindowEvent::ModifiersChanged(_) => {
                // Some platforms report Shift only in the modifier word.
                let shift = self.modifiers.shift_key();
                if shift != state.shift {
                    state.shift = shift;
                    self.mods
                        .mods
                        .ui_screen_key(owner, "key.keyboard.left.shift", shift);
                }
            }
            WindowEvent::Focused(false) => {
                for button in std::mem::take(&mut state.buttons) {
                    self.mods
                        .mods
                        .ui_pointer(owner, PointerKind::Up, x, y, button);
                }
                if std::mem::take(&mut state.shift) {
                    self.mods
                        .mods
                        .ui_screen_key(owner, "key.keyboard.left.shift", false);
                }
                self.show_mod_output();
                return false;
            }
            _ => return false,
        }
        self.show_mod_output();
        true
    }
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn mod_canvas_screen_event(&mut self, _event: &winit::event::WindowEvent) -> bool {
        false
    }
}
