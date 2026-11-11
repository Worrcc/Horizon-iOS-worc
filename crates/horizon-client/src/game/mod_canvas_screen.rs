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
    touch: CanvasTouch,
}

/// Canvas touch never waits for a hold. After a pinch, remaining fingers
/// stay inert until lifted, so lifting one cannot start an accidental drag.
#[cfg(feature = "mods")]
#[derive(Default)]
struct CanvasTouch {
    fingers: Vec<(u64, (f32, f32))>,
    dragging: bool,
}

#[cfg(feature = "mods")]
#[derive(Debug, PartialEq)]
enum CanvasTouchEvent {
    Pointer(PointerKind, (f32, f32)),
    Scroll((f32, f32), f32),
}

#[cfg(feature = "mods")]
impl CanvasTouch {
    fn touch(
        &mut self,
        id: u64,
        phase: winit::event::TouchPhase,
        at: (f32, f32),
    ) -> Vec<CanvasTouchEvent> {
        use winit::event::TouchPhase;
        let mut out = Vec::new();
        match phase {
            TouchPhase::Started => {
                if self.fingers.iter().any(|f| f.0 == id) {
                    return out;
                }
                if self.fingers.is_empty() {
                    self.dragging = true;
                    out.push(CanvasTouchEvent::Pointer(PointerKind::Down, at));
                } else if self.dragging {
                    self.dragging = false;
                    out.push(CanvasTouchEvent::Pointer(
                        PointerKind::Up,
                        self.fingers[0].1,
                    ));
                }
                self.fingers.push((id, at));
            }
            TouchPhase::Moved => {
                let before = self.pinch();
                let Some(finger) = self.fingers.iter_mut().find(|f| f.0 == id) else {
                    return out;
                };
                finger.1 = at;
                if self.dragging {
                    out.push(CanvasTouchEvent::Pointer(PointerKind::Drag, at));
                } else if let (Some((_, old)), Some((mid, new))) = (before, self.pinch()) {
                    let steps = (new - old) / 24.0;
                    if steps != 0.0 {
                        out.push(CanvasTouchEvent::Scroll(mid, steps));
                    }
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(i) = self.fingers.iter().position(|f| f.0 == id) else {
                    return out;
                };
                let (_, last) = self.fingers.remove(i);
                if std::mem::take(&mut self.dragging) {
                    out.push(CanvasTouchEvent::Pointer(PointerKind::Up, last));
                }
            }
        }
        out
    }

    fn pinch(&self) -> Option<((f32, f32), f32)> {
        let [(_, a), (_, b), ..] = self.fingers.as_slice() else {
            return None;
        };
        Some((
            ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
            (a.0 - b.0).hypot(a.1 - b.1),
        ))
    }
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// Direct GUI events avoid synthetic cursor moves during a pinch.
    pub(super) fn mod_canvas_touch(
        &mut self,
        id: u64,
        phase: winit::event::TouchPhase,
        at: (f32, f32),
    ) -> bool {
        if self.screen != Screen::Mod {
            return false;
        }
        let Some(state) = &mut self.mods.canvas_screen else {
            return false;
        };
        let owner = state.owner;
        let events = state.touch.touch(id, phase, at);
        for event in events {
            match event {
                CanvasTouchEvent::Pointer(kind, (x, y)) => {
                    self.cursor = (x, y);
                    self.mods.mods.ui_pointer(owner, kind, x, y, 0);
                }
                CanvasTouchEvent::Scroll((x, y), steps) => {
                    self.mods.mods.ui_scroll(owner, x, y, steps)
                }
            }
        }
        self.show_mod_output();
        true
    }

    pub(super) fn release_canvas_touch(&mut self) {
        let Some(state) = &mut self.mods.canvas_screen else {
            return;
        };
        let touch = std::mem::take(&mut state.touch);
        if touch.dragging {
            let (x, y) = touch.fingers[0].1;
            self.mods
                .mods
                .ui_pointer(state.owner, PointerKind::Up, x, y, 0);
        }
    }

    /// Opens a canvas in the same Screen::Mod slot as widget/native screens.
    pub(super) fn open_mod_canvas_screen(&mut self, owner: usize, under: Screen) {
        self.release_canvas_touch();
        let under = self.mods.canvas_screen.as_ref().map_or(under, |s| s.under);
        self.mods.canvas_screen = Some(CanvasScreenHost {
            owner,
            under,
            buttons: Vec::new(),
            shift: self.modifiers.shift_key(),
            touch: CanvasTouch::default(),
        });
        self.screen = Screen::Mod;
        if self.player.is_some() {
            self.capture(false);
        }
    }
    /// Returns to the covered surface. Only a player's close sends closed.
    pub(super) fn close_mod_canvas_screen(&mut self, tell: bool) -> bool {
        self.release_canvas_touch();
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
        if matches!(event, WindowEvent::Focused(false)) {
            self.release_canvas_touch();
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

#[cfg(all(test, feature = "mods"))]
mod touch_tests {
    use super::*;
    use CanvasTouchEvent::{Pointer, Scroll};
    use PointerKind::{Down, Drag, Up};
    use winit::event::TouchPhase::{Cancelled, Ended, Moved, Started};

    #[test]
    fn canvas_drag_starts_on_down_without_waiting() {
        let mut t = CanvasTouch::default();
        assert_eq!(
            t.touch(1, Started, (10.0, 20.0)),
            [Pointer(Down, (10.0, 20.0))]
        );
        assert_eq!(
            t.touch(1, Moved, (11.0, 20.0)),
            [Pointer(Drag, (11.0, 20.0))]
        );
        assert_eq!(
            t.touch(1, Moved, (70.0, 50.0)),
            [Pointer(Drag, (70.0, 50.0))]
        );
        assert_eq!(t.touch(1, Ended, (70.0, 50.0)), [Pointer(Up, (70.0, 50.0))]);
    }

    #[test]
    fn canvas_tap_is_down_and_up_at_the_same_point() {
        let mut t = CanvasTouch::default();
        assert_eq!(
            t.touch(1, Started, (10.0, 20.0)),
            [Pointer(Down, (10.0, 20.0))]
        );
        assert_eq!(t.touch(1, Ended, (11.0, 21.0)), [Pointer(Up, (10.0, 20.0))]);
        assert!(t.touch(1, Moved, (30.0, 30.0)).is_empty());
    }

    #[test]
    fn canvas_second_finger_ends_drag_and_never_resumes_it() {
        let mut t = CanvasTouch::default();
        t.touch(1, Started, (10.0, 20.0));
        t.touch(1, Moved, (20.0, 20.0));
        assert_eq!(
            t.touch(2, Started, (40.0, 20.0)),
            [Pointer(Up, (20.0, 20.0))]
        );
        assert!(t.touch(1, Ended, (20.0, 20.0)).is_empty());
        assert!(t.touch(2, Moved, (60.0, 40.0)).is_empty());
        assert!(t.touch(2, Ended, (60.0, 40.0)).is_empty());
        assert_eq!(
            t.touch(3, Started, (10.0, 10.0)),
            [Pointer(Down, (10.0, 10.0))]
        );
    }

    #[test]
    fn canvas_pinch_scrolls_at_midpoint_one_step_per_24_pixels() {
        let mut t = CanvasTouch::default();
        t.touch(1, Started, (0.0, 0.0));
        t.touch(2, Started, (24.0, 0.0));
        assert_eq!(t.touch(2, Moved, (48.0, 0.0)), [Scroll((24.0, 0.0), 1.0)]);
        assert_eq!(t.touch(1, Moved, (12.0, 0.0)), [Scroll((30.0, 0.0), -0.5)]);
        // Rotation at the same distance does not zoom.
        assert!(t.touch(2, Moved, (12.0, 36.0)).is_empty());
        assert!(t.touch(3, Started, (100.0, 100.0)).is_empty());
        assert!(t.touch(3, Moved, (200.0, 200.0)).is_empty());
    }

    #[test]
    fn canvas_cancel_releases_once_and_pinch_cancel_does_not_click() {
        let mut t = CanvasTouch::default();
        t.touch(1, Started, (4.0, 5.0));
        assert_eq!(t.touch(1, Cancelled, (4.0, 5.0)), [Pointer(Up, (4.0, 5.0))]);
        assert!(t.touch(1, Ended, (4.0, 5.0)).is_empty());
        t.touch(1, Started, (4.0, 5.0));
        t.touch(2, Started, (40.0, 50.0));
        assert!(t.touch(2, Cancelled, (40.0, 50.0)).is_empty());
        assert!(t.touch(1, Ended, (4.0, 5.0)).is_empty());
    }
}
