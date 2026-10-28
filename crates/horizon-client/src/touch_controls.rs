//! Touch controls for phones and tablets (the iOS app): fingers on the window turned into the
//! keys, clicks and look movement the game already understands, as Bedrock Edition's touch
//! layout does. Pure input logic in GUI pixels; the game draws it and applies the [`Output`]s
//! (`game/touch.rs`).
//!
//! In the world:
//! - left side: a joystick where the finger lands (W, A, S and D; pushed past its ring, sprint);
//! - right side: drag to look, tap to use or attack, hold to break;
//! - buttons: jump, sneak (a toggle), use (held: eat, draw a bow, trade), drop, inventory, chat,
//!   perspective and pause;
//! - the hotbar: tap a slot to select it.
//!
//! On menus the first finger is the pointer: a tap clicks, a quick swipe scrolls, and a press
//! that stays put a moment holds the button down to drag (sliders, scroll bars, stacks).
use std::collections::HashMap;
use std::time::{Duration, Instant};
use winit::{
    event::{MouseButton, TouchPhase},
    keyboard::KeyCode,
};

/// A finger kept still this long breaks blocks (look side) or starts a drag (menus).
pub const HOLD: Duration = Duration::from_millis(300);
/// On menus a press held this long, unmoved, becomes a drag instead of a scroll.
pub const POINTER_HOLD: Duration = Duration::from_millis(150);
/// Movement under this many GUI pixels still counts as a tap.
pub const TAP_SLOP: f32 = 6.0;
/// The joystick's ring, in GUI pixels from where the finger landed.
pub const STICK_RADIUS: f32 = 22.0;
/// GUI pixels of a swipe that scroll one line.
pub const SCROLL_LINE: f32 = 12.0;

/// An on-screen button: what the vanilla default key of its action does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Button {
    Jump,
    Sneak,
    Use,
    Drop,
    Inventory,
    Chat,
    Perspective,
    Pause,
}

impl Button {
    pub const ALL: [Button; 8] = [
        Button::Jump,
        Button::Sneak,
        Button::Use,
        Button::Drop,
        Button::Inventory,
        Button::Chat,
        Button::Perspective,
        Button::Pause,
    ];
    /// The vanilla default key of the button's mapping (`key.jump`...): the game translates
    /// keys to their defaults before acting on them, so these follow nothing the player rebinds.
    /// Use is the right mouse button instead.
    pub fn key(self) -> Option<KeyCode> {
        Some(match self {
            Button::Jump => KeyCode::Space,
            Button::Sneak => KeyCode::ShiftLeft,
            Button::Use => return None,
            Button::Drop => KeyCode::KeyQ,
            Button::Inventory => KeyCode::KeyE,
            Button::Chat => KeyCode::KeyT,
            Button::Perspective => KeyCode::F5,
            Button::Pause => KeyCode::Escape,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            Button::Jump => "Jump",
            Button::Sneak => "Sneak",
            Button::Use => "Use",
            Button::Drop => "Drop",
            Button::Inventory => "...",
            Button::Chat => "T",
            Button::Perspective => "F5",
            Button::Pause => "||",
        }
    }
    /// Held while the finger is on it; the others are a tap of their key.
    fn held(self) -> bool {
        matches!(self, Button::Jump | Button::Use)
    }
}

/// Where the controls are on a GUI of `width` x `height` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
}

impl Layout {
    /// `[x, y, w, h]` of a button.
    pub fn button(&self, button: Button) -> [f32; 4] {
        let (w, h) = (self.width, self.height);
        let centre = (w / 2.0).floor();
        match button {
            Button::Jump => [w - 38.0, h - 60.0, 32.0, 24.0],
            Button::Sneak => [w - 38.0, h - 32.0, 32.0, 24.0],
            Button::Use => [w - 74.0, h - 60.0, 32.0, 24.0],
            Button::Drop => [w - 38.0, h - 88.0, 32.0, 24.0],
            Button::Inventory => [centre + 95.0, h - 22.0, 20.0, 20.0],
            Button::Pause => [w - 22.0, 4.0, 18.0, 18.0],
            Button::Chat => [w - 44.0, 4.0, 18.0, 18.0],
            Button::Perspective => [w - 66.0, 4.0, 18.0, 18.0],
        }
    }
    pub fn button_at(&self, x: f32, y: f32) -> Option<Button> {
        Button::ALL.into_iter().find(|&b| {
            let [bx, by, bw, bh] = self.button(b);
            // A finger is wider than the drawn box.
            x >= bx - 2.0 && x < bx + bw + 2.0 && y >= by - 2.0 && y < by + bh + 2.0
        })
    }
    /// The hotbar slot (0-8) under a point: `Gui.extractItemHotbar`'s 182 x 22 bar.
    pub fn hotbar_slot(&self, x: f32, y: f32) -> Option<usize> {
        let left = (self.width / 2.0).floor() - 91.0;
        if y < self.height - 24.0 || x < left || x >= left + 182.0 {
            return None;
        }
        Some((((x - left - 1.0) / 20.0).floor().max(0.0) as usize).min(8))
    }
    /// Where a finger starts the joystick: the lower left of the screen.
    pub fn in_stick_zone(&self, x: f32, y: f32) -> bool {
        x < self.width * 0.45 && y > self.height * 0.25
    }
    /// Where the joystick is drawn while no finger holds it.
    pub fn stick_rest(&self) -> (f32, f32) {
        (self.width * 0.14 + 10.0, self.height - 50.0)
    }
}

/// What the game should do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Output {
    /// Press or release a vanilla default key.
    Key(KeyCode, bool),
    /// Press or release a mouse button in the world.
    Mouse(MouseButton, bool),
    /// A tap on the world: attack what the crosshair points at if it is an entity, else use.
    Use,
    /// Turn the view by this many GUI pixels of finger movement.
    Look(f32, f32),
    /// Select a hotbar slot.
    Hotbar(usize),
    /// Move the menu pointer here (GUI pixels).
    Pointer(f32, f32),
    /// Press or release the menu pointer's left button.
    PointerButton(bool),
    /// Scroll a menu by this many lines (positive is up, as a wheel turned away).
    Scroll(f32),
}

#[derive(Clone, Copy, Debug)]
enum PointerMode {
    Pending,
    Pressed,
    Scrolling,
}

#[derive(Clone, Copy, Debug)]
enum Finger {
    Stick {
        origin: (f32, f32),
        at: (f32, f32),
    },
    Look {
        last: (f32, f32),
        start: Instant,
        moved: f32,
        breaking: bool,
    },
    Button(Button),
    Hotbar,
    Pointer {
        last: (f32, f32),
        start: Instant,
        moved: f32,
        mode: PointerMode,
    },
    Ignored,
}

/// The fingers on the screen and the keys they hold.
#[derive(Default)]
pub struct TouchControls {
    fingers: HashMap<u64, Finger>,
    /// Forward, back, left, right and sprint, as the joystick holds them.
    stick_keys: [bool; 5],
    sneaking: bool,
    /// Whether the last touch was in the world (the controls are drawn) or on a menu.
    playing: bool,
    /// A finger has touched the screen: the controls are drawn from then on.
    pub used: bool,
}

const STICK_KEYS: [KeyCode; 5] = [
    KeyCode::KeyW,
    KeyCode::KeyS,
    KeyCode::KeyA,
    KeyCode::KeyD,
    KeyCode::ControlLeft,
];

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

impl TouchControls {
    /// One touch event at `(x, y)` GUI pixels. `playing`: the world is on screen with no menu.
    pub fn touch(
        &mut self,
        id: u64,
        phase: TouchPhase,
        (x, y): (f32, f32),
        layout: &Layout,
        playing: bool,
        now: Instant,
    ) -> Vec<Output> {
        self.used = true;
        let mut out = Vec::new();
        if playing != self.playing {
            // A menu opened or closed under the fingers: let go of everything.
            out.extend(self.release_all());
            self.playing = playing;
        }
        match phase {
            TouchPhase::Started => {
                let finger = if !playing {
                    if self
                        .fingers
                        .values()
                        .any(|f| matches!(f, Finger::Pointer { .. }))
                    {
                        Finger::Ignored
                    } else {
                        out.push(Output::Pointer(x, y));
                        Finger::Pointer {
                            last: (x, y),
                            start: now,
                            moved: 0.0,
                            mode: PointerMode::Pending,
                        }
                    }
                } else if let Some(button) = layout.button_at(x, y) {
                    out.extend(self.press_button(button));
                    Finger::Button(button)
                } else if let Some(slot) = layout.hotbar_slot(x, y) {
                    out.push(Output::Hotbar(slot));
                    Finger::Hotbar
                } else if layout.in_stick_zone(x, y)
                    && !self
                        .fingers
                        .values()
                        .any(|f| matches!(f, Finger::Stick { .. }))
                {
                    Finger::Stick {
                        origin: (x, y),
                        at: (x, y),
                    }
                } else {
                    Finger::Look {
                        last: (x, y),
                        start: now,
                        moved: 0.0,
                        breaking: false,
                    }
                };
                self.fingers.insert(id, finger);
            }
            TouchPhase::Moved => {
                let Some(finger) = self.fingers.get_mut(&id) else {
                    return out;
                };
                match finger {
                    Finger::Stick { at, .. } => {
                        *at = (x, y);
                        out.extend(self.update_stick());
                    }
                    Finger::Look { last, moved, .. } => {
                        let (dx, dy) = (x - last.0, y - last.1);
                        *moved += dx.hypot(dy);
                        *last = (x, y);
                        out.push(Output::Look(dx, dy));
                    }
                    Finger::Pointer {
                        last,
                        start,
                        moved,
                        mode,
                    } => {
                        let dy = y - last.1;
                        *moved += distance(*last, (x, y));
                        *last = (x, y);
                        match mode {
                            PointerMode::Pending if *moved >= TAP_SLOP => {
                                if now.duration_since(*start) < POINTER_HOLD {
                                    *mode = PointerMode::Scrolling;
                                    out.push(Output::Scroll(dy / SCROLL_LINE));
                                } else {
                                    *mode = PointerMode::Pressed;
                                    out.push(Output::PointerButton(true));
                                    out.push(Output::Pointer(x, y));
                                }
                            }
                            PointerMode::Pending => {}
                            PointerMode::Pressed => out.push(Output::Pointer(x, y)),
                            PointerMode::Scrolling => out.push(Output::Scroll(dy / SCROLL_LINE)),
                        }
                    }
                    Finger::Button(_) | Finger::Hotbar | Finger::Ignored => {}
                }
            }
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let Some(finger) = self.fingers.remove(&id) else {
                    return out;
                };
                let cancelled = phase == TouchPhase::Cancelled;
                out.extend(self.lift(finger, now, cancelled));
            }
        }
        out
    }

    /// Once a frame: fingers held still turn into breaking (world) or a drag (menus).
    pub fn update(&mut self, now: Instant) -> Vec<Output> {
        let mut out = Vec::new();
        for finger in self.fingers.values_mut() {
            match finger {
                Finger::Look {
                    start,
                    moved,
                    breaking,
                    ..
                } if !*breaking && *moved < TAP_SLOP && now.duration_since(*start) >= HOLD => {
                    *breaking = true;
                    out.push(Output::Mouse(MouseButton::Left, true));
                }
                Finger::Pointer { start, mode, .. }
                    if matches!(mode, PointerMode::Pending)
                        && now.duration_since(*start) >= POINTER_HOLD =>
                {
                    *mode = PointerMode::Pressed;
                    out.push(Output::PointerButton(true));
                }
                _ => {}
            }
        }
        out
    }

    /// Lets go of every finger and held key: the app went to the background, or a menu opened.
    pub fn release_all(&mut self) -> Vec<Output> {
        let mut out = Vec::new();
        let now = Instant::now();
        for (_, finger) in std::mem::take(&mut self.fingers) {
            out.extend(self.lift(finger, now, true));
        }
        out.extend(self.update_stick());
        out
    }

    /// The joystick's centre and the finger on it, for drawing.
    pub fn stick(&self) -> Option<((f32, f32), (f32, f32))> {
        self.fingers.values().find_map(|f| match f {
            Finger::Stick { origin, at } => Some((*origin, *at)),
            _ => None,
        })
    }
    /// Whether a finger is on `button`, or the sneak toggle is on.
    pub fn pressed(&self, button: Button) -> bool {
        (button == Button::Sneak && self.sneaking)
            || self
                .fingers
                .values()
                .any(|f| matches!(f, Finger::Button(b) if *b == button))
    }

    fn press_button(&mut self, button: Button) -> Vec<Output> {
        let Some(key) = button.key() else {
            return vec![Output::Mouse(MouseButton::Right, true)];
        };
        if button == Button::Sneak {
            // A toggle: holding a finger there would leave none to look with.
            self.sneaking = !self.sneaking;
            return vec![Output::Key(key, self.sneaking)];
        }
        if button.held() {
            vec![Output::Key(key, true)]
        } else {
            vec![Output::Key(key, true), Output::Key(key, false)]
        }
    }

    fn lift(&mut self, finger: Finger, now: Instant, cancelled: bool) -> Vec<Output> {
        match finger {
            Finger::Stick { .. } => self.update_stick(),
            Finger::Look {
                start,
                moved,
                breaking,
                ..
            } => {
                if breaking {
                    vec![Output::Mouse(MouseButton::Left, false)]
                } else if !cancelled && moved < TAP_SLOP && now.duration_since(start) < HOLD {
                    vec![Output::Use]
                } else {
                    Vec::new()
                }
            }
            Finger::Button(button) if button.held() => match button.key() {
                Some(key) => vec![Output::Key(key, false)],
                None => vec![Output::Mouse(MouseButton::Right, false)],
            },
            Finger::Pointer { mode, .. } => match mode {
                PointerMode::Pending if !cancelled => {
                    vec![Output::PointerButton(true), Output::PointerButton(false)]
                }
                PointerMode::Pressed => vec![Output::PointerButton(false)],
                _ => Vec::new(),
            },
            Finger::Button(_) | Finger::Hotbar | Finger::Ignored => Vec::new(),
        }
    }

    /// The keys the joystick holds now, as presses and releases of what changed.
    fn update_stick(&mut self) -> Vec<Output> {
        let mut want = [false; 5];
        if let Some((origin, at)) = self.stick() {
            let (vx, vy) = (
                (at.0 - origin.0) / STICK_RADIUS,
                (at.1 - origin.1) / STICK_RADIUS,
            );
            let length = vx.hypot(vy);
            if length >= 0.25 {
                // 8 directions: each axis counts once it is past ~22 degrees off the other.
                want[0] = vy < -0.38 * length;
                want[1] = vy > 0.38 * length;
                want[2] = vx < -0.38 * length;
                want[3] = vx > 0.38 * length;
                want[4] = want[0] && length > 1.15;
            }
        }
        let mut out = Vec::new();
        for (i, key) in STICK_KEYS.into_iter().enumerate() {
            if want[i] != self.stick_keys[i] {
                self.stick_keys[i] = want[i];
                out.push(Output::Key(key, want[i]));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAYOUT: Layout = Layout {
        width: 633.0,
        height: 292.0,
    };

    fn at(
        controls: &mut TouchControls,
        id: u64,
        phase: TouchPhase,
        p: (f32, f32),
        now: Instant,
    ) -> Vec<Output> {
        controls.touch(id, phase, p, &LAYOUT, true, now)
    }

    #[test]
    fn joystick_walks_and_sprints() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        assert!(at(&mut c, 1, TouchPhase::Started, (100.0, 200.0), t).is_empty());
        assert_eq!(
            at(&mut c, 1, TouchPhase::Moved, (100.0, 185.0), t),
            vec![Output::Key(KeyCode::KeyW, true)]
        );
        assert_eq!(
            at(&mut c, 1, TouchPhase::Moved, (100.0, 170.0), t),
            vec![Output::Key(KeyCode::ControlLeft, true)]
        );
        // Diagonal forward-left, back inside the ring.
        assert_eq!(
            at(&mut c, 1, TouchPhase::Moved, (88.0, 188.0), t),
            vec![
                Output::Key(KeyCode::KeyA, true),
                Output::Key(KeyCode::ControlLeft, false)
            ]
        );
        assert_eq!(
            at(&mut c, 1, TouchPhase::Ended, (88.0, 188.0), t),
            vec![
                Output::Key(KeyCode::KeyW, false),
                Output::Key(KeyCode::KeyA, false)
            ]
        );
    }

    #[test]
    fn look_side_taps_uses_and_holds_break() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        at(&mut c, 1, TouchPhase::Started, (400.0, 120.0), t);
        assert_eq!(
            at(
                &mut c,
                1,
                TouchPhase::Ended,
                (401.0, 120.0),
                t + Duration::from_millis(100)
            ),
            vec![Output::Use]
        );
        at(&mut c, 2, TouchPhase::Started, (400.0, 120.0), t);
        assert!(c.update(t + Duration::from_millis(100)).is_empty());
        assert_eq!(
            c.update(t + HOLD),
            vec![Output::Mouse(MouseButton::Left, true)]
        );
        assert_eq!(
            at(&mut c, 2, TouchPhase::Moved, (410.0, 115.0), t + HOLD),
            vec![Output::Look(10.0, -5.0)]
        );
        assert_eq!(
            at(&mut c, 2, TouchPhase::Ended, (410.0, 115.0), t + HOLD * 2),
            vec![Output::Mouse(MouseButton::Left, false)]
        );
    }

    #[test]
    fn a_dragged_look_is_no_tap() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        at(&mut c, 1, TouchPhase::Started, (400.0, 120.0), t);
        at(&mut c, 1, TouchPhase::Moved, (430.0, 120.0), t);
        assert!(c.update(t + HOLD).is_empty());
        assert!(at(&mut c, 1, TouchPhase::Ended, (430.0, 120.0), t + HOLD).is_empty());
    }

    #[test]
    fn buttons_and_hotbar() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        let [x, y, w, h] = LAYOUT.button(Button::Jump);
        let jump = (x + w / 2.0, y + h / 2.0);
        assert_eq!(
            at(&mut c, 1, TouchPhase::Started, jump, t),
            vec![Output::Key(KeyCode::Space, true)]
        );
        assert!(c.pressed(Button::Jump));
        assert_eq!(
            at(&mut c, 1, TouchPhase::Ended, jump, t),
            vec![Output::Key(KeyCode::Space, false)]
        );
        let [x, y, w, h] = LAYOUT.button(Button::Sneak);
        let sneak = (x + w / 2.0, y + h / 2.0);
        assert_eq!(
            at(&mut c, 2, TouchPhase::Started, sneak, t),
            vec![Output::Key(KeyCode::ShiftLeft, true)]
        );
        assert!(at(&mut c, 2, TouchPhase::Ended, sneak, t).is_empty());
        assert!(c.pressed(Button::Sneak));
        assert_eq!(
            at(&mut c, 3, TouchPhase::Started, sneak, t),
            vec![Output::Key(KeyCode::ShiftLeft, false)]
        );
        let [x, y, w, h] = LAYOUT.button(Button::Use);
        let use_at = (x + w / 2.0, y + h / 2.0);
        assert_eq!(
            at(&mut c, 5, TouchPhase::Started, use_at, t),
            vec![Output::Mouse(MouseButton::Right, true)]
        );
        assert_eq!(
            at(&mut c, 5, TouchPhase::Ended, use_at, t),
            vec![Output::Mouse(MouseButton::Right, false)]
        );
        // Slot 5 of the bar centred at 316: x from 225 + 4 * 20.
        assert_eq!(
            at(&mut c, 4, TouchPhase::Started, (316.0, 285.0), t),
            vec![Output::Hotbar(4)]
        );
        assert_eq!(LAYOUT.hotbar_slot(225.5, 280.0), Some(0));
        assert_eq!(LAYOUT.hotbar_slot(406.0, 280.0), Some(8));
        assert_eq!(LAYOUT.hotbar_slot(316.0, 200.0), None);
    }

    #[test]
    fn menus_click_scroll_and_drag() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        let menu =
            |c: &mut TouchControls, id, phase, p, now| c.touch(id, phase, p, &LAYOUT, false, now);
        assert_eq!(
            menu(&mut c, 1, TouchPhase::Started, (50.0, 60.0), t),
            vec![Output::Pointer(50.0, 60.0)]
        );
        // A second finger does nothing while the first is the pointer.
        assert!(menu(&mut c, 2, TouchPhase::Started, (90.0, 60.0), t).is_empty());
        assert_eq!(
            menu(
                &mut c,
                1,
                TouchPhase::Ended,
                (51.0, 60.0),
                t + Duration::from_millis(80)
            ),
            vec![Output::PointerButton(true), Output::PointerButton(false)]
        );
        // A quick swipe scrolls.
        menu(&mut c, 3, TouchPhase::Started, (50.0, 60.0), t);
        assert_eq!(
            menu(
                &mut c,
                3,
                TouchPhase::Moved,
                (50.0, 84.0),
                t + Duration::from_millis(50)
            ),
            vec![Output::Scroll(2.0)]
        );
        assert!(menu(&mut c, 3, TouchPhase::Ended, (50.0, 84.0), t).is_empty());
        // Held still, then moved: a drag.
        menu(&mut c, 4, TouchPhase::Started, (50.0, 60.0), t);
        assert_eq!(
            c.update(t + POINTER_HOLD),
            vec![Output::PointerButton(true)]
        );
        assert_eq!(
            menu(&mut c, 4, TouchPhase::Moved, (70.0, 60.0), t + POINTER_HOLD),
            vec![Output::Pointer(70.0, 60.0)]
        );
        assert_eq!(
            menu(&mut c, 4, TouchPhase::Ended, (70.0, 60.0), t + POINTER_HOLD),
            vec![Output::PointerButton(false)]
        );
    }

    #[test]
    fn opening_a_menu_lets_go_of_held_keys() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        at(&mut c, 1, TouchPhase::Started, (100.0, 200.0), t);
        at(&mut c, 1, TouchPhase::Moved, (100.0, 185.0), t);
        let out = c.touch(2, TouchPhase::Started, (50.0, 50.0), &LAYOUT, false, t);
        assert_eq!(
            out,
            vec![
                Output::Key(KeyCode::KeyW, false),
                Output::Pointer(50.0, 50.0)
            ]
        );
    }
}
