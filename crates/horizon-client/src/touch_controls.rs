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
//! On menus the first finger is the pointer: a tap left clicks, a quick swipe scrolls, and
//! moving after a short hold drags (sliders, scroll bars, stacks). Holding still for a long
//! press right clicks once; that finger then does nothing until lifted.
use std::collections::HashMap;
use std::time::{Duration, Instant};
use winit::{
    event::{MouseButton, TouchPhase},
    keyboard::KeyCode,
};

/// A finger kept still this long breaks blocks (look side).
pub const HOLD: Duration = Duration::from_millis(300);
/// On menus movement after a press held this long starts a drag instead of a scroll.
pub const POINTER_HOLD: Duration = Duration::from_millis(150);
/// On menus a finger held still this long right clicks once.
pub const LONG_PRESS: Duration = Duration::from_millis(500);
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
    Sprint,
    Attack,
    Use,
    Drop,
    Inventory,
    Chat,
    Perspective,
    Pause,
}

impl Button {
    pub fn id(self) -> &'static str {
        match self {
            Self::Jump => "game.jump",
            Self::Sneak => "game.sneak",
            Self::Sprint => "game.sprint",
            Self::Attack => "game.attack",
            Self::Use => "game.use",
            Self::Drop => "game.drop",
            Self::Inventory => "game.inventory",
            Self::Chat => "game.chat",
            Self::Perspective => "game.perspective",
            Self::Pause => "game.pause",
        }
    }
    pub fn name_key(self) -> &'static str {
        match self {
            Self::Jump => "key.jump",
            Self::Sneak => "key.sneak",
            Self::Sprint => "key.sprint",
            Self::Attack => "key.attack",
            Self::Use => "key.use",
            Self::Drop => "key.drop",
            Self::Inventory => "key.inventory",
            Self::Chat => "key.chat",
            Self::Perspective => "key.togglePerspective",
            Self::Pause => "menu.game",
        }
    }
    pub const ALL: [Button; 10] = [
        Button::Jump,
        Button::Sneak,
        Button::Use,
        Button::Drop,
        Button::Inventory,
        Button::Chat,
        Button::Perspective,
        Button::Pause,
        Button::Sprint,
        Button::Attack,
    ];
    /// The vanilla default key of the button's mapping (`key.jump`...): the game translates
    /// keys to their defaults before acting on them, so these follow nothing the player rebinds.
    /// Use is the right mouse button instead.
    pub fn key(self) -> Option<KeyCode> {
        Some(match self {
            Button::Jump => KeyCode::Space,
            Button::Sneak => KeyCode::ShiftLeft,
            Button::Sprint => KeyCode::ControlLeft,
            Button::Attack => return None,
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
            Button::Sprint => "Sprint",
            Button::Attack => "Attack",
            Button::Use => "Use",
            Button::Drop => "Drop",
            Button::Inventory => "...",
            Button::Chat => "T",
            Button::Perspective => "F5",
            Button::Pause => "||",
        }
    }
    /// Translation for the label; the renderer retains `label()` before languages load.
    pub fn label_key(self) -> &'static str {
        match self {
            Button::Jump => "horizon.touch.jump",
            Button::Sneak => "horizon.touch.sneak",
            Button::Sprint => "horizon.touch.sprint",
            Button::Attack => "horizon.touch.attack",
            Button::Use => "horizon.touch.use",
            Button::Drop => "horizon.touch.drop",
            Button::Inventory => "horizon.touch.inventory",
            Button::Chat => "horizon.touch.chat",
            Button::Perspective => "horizon.touch.perspective",
            Button::Pause => "horizon.touch.pause",
        }
    }
    pub fn riding_default(self) -> bool {
        matches!(self, Self::Sprint | Self::Attack)
    }
    fn mouse(self) -> MouseButton {
        if self == Self::Attack {
            MouseButton::Left
        } else {
            MouseButton::Right
        }
    }
    /// Held while the finger is on it; the others are a tap of their key.
    fn held(self) -> bool {
        matches!(
            self,
            Button::Jump | Button::Use | Button::Sprint | Button::Attack
        )
    }
}

/// Where the controls are on a GUI of `width` x `height` pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: f32,
    pub height: f32,
    /// Inventory goes opposite the actual off-hand slot, mirroring with the main arm.
    pub main_hand_left: bool,
}

impl Layout {
    /// `[x, y, w, h]` of a button.
    pub fn button(&self, button: Button) -> [f32; 4] {
        let (w, h) = (self.width, self.height);
        let centre = (w / 2.0).floor();
        match button {
            Button::Jump => [w - 38.0, h - 60.0, 32.0, 24.0],
            Button::Sneak => [w - 38.0, h - 32.0, 32.0, 24.0],
            Button::Sprint => [w - 110.0, h - 88.0, 32.0, 24.0],
            Button::Attack => [w - 110.0, h - 60.0, 32.0, 24.0],
            Button::Use => [w - 74.0, h - 60.0, 32.0, 24.0],
            Button::Drop => [w - 38.0, h - 88.0, 32.0, 24.0],
            Button::Inventory => [
                if self.main_hand_left {
                    centre - 120.0
                } else {
                    centre + 91.0
                },
                h - 23.0,
                29.0,
                24.0,
            ],
            Button::Pause => [w - 24.0, 54.0, 20.0, 20.0],
            Button::Chat => [w - 46.0, 54.0, 20.0, 20.0],
            Button::Perspective => [w - 68.0, 54.0, 20.0, 20.0],
        }
    }
    /// The off-hand artwork is asymmetric: centre the label over its item well.
    pub fn inventory_label_center(&self) -> (f32, f32) {
        (
            (self.width / 2.0).floor() + if self.main_hand_left { -109.0 } else { 109.0 },
            self.height - 11.0,
        )
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
    /// Press and release the menu pointer's right button.
    PointerRight,
    /// Scroll a menu by this many lines (positive is up, as a wheel turned away).
    Scroll(f32),
}

#[derive(Clone, Copy, Debug)]
enum PointerMode {
    Pending,
    Pressed,
    Scrolling,
    Spent,
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
    /// The resolved player layout. Menus and standalone callers retain the defaults.
    geometry: Option<Geometry>,
    fingers: HashMap<u64, Finger>,
    /// Forward, back, left, right and sprint, as the joystick holds them.
    stick_keys: [bool; 5],
    sneaking: bool,
    /// Whether the last touch was in the world (the controls are drawn) or on a menu.
    playing: bool,
    /// A finger has touched the screen: the controls are drawn until a controller is used.
    pub used: bool,
}

#[derive(Clone, Debug)]
pub struct Geometry {
    pub buttons: [Option<[f32; 4]>; 10],
    pub stick: Option<[f32; 4]>,
    pub moved_stick: bool,
}
impl Geometry {
    pub fn button_at(&self, at: (f32, f32)) -> Option<Button> {
        Button::ALL
            .into_iter()
            .zip(self.buttons)
            .find_map(|(b, r)| {
                r.filter(|&[x, y, w, h]| {
                    crate::touch_layout::contains([x - 2.0, y - 2.0, w + 4.0, h + 4.0], at)
                })
                .map(|_| b)
            })
    }
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
    pub fn configure(&mut self, geometry: Geometry) -> Vec<Output> {
        let mut out = Vec::new();
        let now = Instant::now();
        let hidden: Vec<_> = self
            .fingers
            .iter()
            .filter_map(|(&id, finger)| {
                let hidden = match finger {
                    Finger::Button(button) => geometry.buttons
                        [Button::ALL.iter().position(|b| b == button).unwrap()]
                    .is_none(),
                    Finger::Stick { .. } => geometry.stick.is_none(),
                    _ => false,
                };
                hidden.then_some(id)
            })
            .collect();
        for id in hidden {
            let finger = self.fingers.insert(id, Finger::Ignored).unwrap();
            out.extend(self.lift(finger, now, true));
        }
        if geometry.buttons[Button::ALL
            .iter()
            .position(|b| *b == Button::Sneak)
            .unwrap()]
        .is_none()
            && self.sneaking
        {
            self.sneaking = false;
            out.push(Output::Key(KeyCode::ShiftLeft, false));
        }
        self.geometry = Some(geometry);
        out
    }
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
                } else if let Some(button) = self
                    .geometry
                    .as_ref()
                    .map_or_else(|| layout.button_at(x, y), |g| g.button_at((x, y)))
                {
                    out.extend(self.press_button(button));
                    Finger::Button(button)
                } else if let Some(slot) = layout.hotbar_slot(x, y) {
                    out.push(Output::Hotbar(slot));
                    Finger::Hotbar
                } else if self.geometry.as_ref().map_or_else(
                    || layout.in_stick_zone(x, y),
                    |g| {
                        g.stick.is_some_and(|r| {
                            if g.moved_stick {
                                crate::touch_layout::contains(r, (x, y))
                            } else {
                                layout.in_stick_zone(x, y)
                            }
                        })
                    },
                ) && !self
                    .fingers
                    .values()
                    .any(|f| matches!(f, Finger::Stick { .. }))
                {
                    Finger::Stick {
                        origin: self
                            .geometry
                            .as_ref()
                            .filter(|g| g.moved_stick)
                            .and_then(|g| g.stick)
                            .map_or((x, y), |r| (r[0] + r[2] / 2.0, r[1] + r[3] / 2.0)),
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
                            PointerMode::Pending | PointerMode::Spent => {}
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

    /// Once a frame: fingers held still turn into breaking (world) or a right click (menus).
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
                        && now.duration_since(*start) >= LONG_PRESS =>
                {
                    *mode = PointerMode::Spent;
                    out.push(Output::PointerRight);
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
        self.latched(button) || self.touching(button)
    }
    pub fn holds_sprint(&self) -> bool {
        self.pressed(Button::Sprint) || self.stick_keys[4]
    }
    /// A finger is on this button, independently of the Sneak latch.
    pub fn touching(&self, button: Button) -> bool {
        self.fingers
            .values()
            .any(|f| matches!(f, Finger::Button(b) if *b == button))
    }
    /// The existing touch Sneak toggle, for the gold selection frame.
    pub fn latched(&self, button: Button) -> bool {
        button == Button::Sneak && self.sneaking
    }

    fn press_button(&mut self, button: Button) -> Vec<Output> {
        let Some(key) = button.key() else {
            return vec![Output::Mouse(button.mouse(), true)];
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

    fn holds_mouse(&self, button: MouseButton) -> bool {
        self.fingers.values().any(|f| match f {
            Finger::Button(b) => b.held() && b.key().is_none() && b.mouse() == button,
            Finger::Look { breaking: true, .. } => button == MouseButton::Left,
            _ => false,
        })
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
                    if self.holds_mouse(MouseButton::Left) {
                        Vec::new()
                    } else {
                        vec![Output::Mouse(MouseButton::Left, false)]
                    }
                } else if !cancelled && moved < TAP_SLOP && now.duration_since(start) < HOLD {
                    vec![Output::Use]
                } else {
                    Vec::new()
                }
            }
            Finger::Button(button) if button.held() => match button.key() {
                Some(key)
                    if self.fingers.values().any(
                        |f| matches!(f, Finger::Button(b) if b.held() && b.key() == Some(key)),
                    ) || (key == KeyCode::ControlLeft && self.stick_keys[4]) =>
                {
                    Vec::new()
                }
                Some(key) => vec![Output::Key(key, false)],
                None if self.holds_mouse(button.mouse()) => Vec::new(),
                None => vec![Output::Mouse(button.mouse(), false)],
            },
            Finger::Pointer { start, mode, .. } => match mode {
                PointerMode::Pending if !cancelled && now.duration_since(start) < LONG_PRESS => {
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
                if want[i] || key != KeyCode::ControlLeft || !self.touching(Button::Sprint) {
                    out.push(Output::Key(key, want[i]));
                }
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
        main_hand_left: false,
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
    fn sprint_and_attack_hold_until_lift_or_context_hides_them() {
        let now = Instant::now();
        for (button, down, up) in [
            (
                Button::Sprint,
                Output::Key(KeyCode::ControlLeft, true),
                Output::Key(KeyCode::ControlLeft, false),
            ),
            (
                Button::Attack,
                Output::Mouse(MouseButton::Left, true),
                Output::Mouse(MouseButton::Left, false),
            ),
        ] {
            let mut controls = TouchControls::default();
            let r = LAYOUT.button(button);
            let point = (r[0] + 10.0, r[1] + 10.0);
            assert_eq!(
                at(&mut controls, 1, TouchPhase::Started, point, now),
                [down]
            );
            assert!(controls.pressed(button));
            if button == Button::Sprint {
                assert!(controls.holds_sprint());
            }
            assert!(controls.update(now + HOLD).is_empty());
            assert_eq!(
                at(&mut controls, 1, TouchPhase::Ended, point, now + HOLD),
                [up]
            );
            assert_eq!(
                at(&mut controls, 2, TouchPhase::Started, point, now),
                [down]
            );
            let geometry = Geometry {
                buttons: [None; 10],
                stick: None,
                moved_stick: false,
            };
            assert_eq!(controls.configure(geometry), [up]);
            assert!(!controls.pressed(button));
            assert!(at(&mut controls, 2, TouchPhase::Ended, point, now).is_empty());
        }
    }

    #[test]
    fn sprint_button_and_joystick_keep_each_other_held() {
        let mut c = TouchControls::default();
        let now = Instant::now();
        let r = LAYOUT.button(Button::Sprint);
        let point = (r[0] + 10.0, r[1] + 10.0);
        at(&mut c, 1, TouchPhase::Started, point, now);
        at(&mut c, 2, TouchPhase::Started, (100.0, 200.0), now);
        at(&mut c, 2, TouchPhase::Moved, (100.0, 170.0), now);
        assert!(
            !at(&mut c, 1, TouchPhase::Ended, point, now)
                .contains(&Output::Key(KeyCode::ControlLeft, false))
        );
        assert!(c.holds_sprint());
        assert!(
            at(&mut c, 2, TouchPhase::Ended, (100.0, 170.0), now)
                .contains(&Output::Key(KeyCode::ControlLeft, false))
        );
        at(&mut c, 1, TouchPhase::Started, point, now);
        at(&mut c, 2, TouchPhase::Started, (100.0, 200.0), now);
        at(&mut c, 2, TouchPhase::Moved, (100.0, 170.0), now);
        assert!(
            !at(&mut c, 2, TouchPhase::Ended, (100.0, 170.0), now)
                .contains(&Output::Key(KeyCode::ControlLeft, false))
        );
        assert!(
            at(&mut c, 1, TouchPhase::Ended, point, now)
                .contains(&Output::Key(KeyCode::ControlLeft, false))
        );
    }

    #[test]
    fn attack_button_and_look_hold_do_not_release_each_others_mouse_press() {
        let mut c = TouchControls::default();
        let now = Instant::now();
        let r = LAYOUT.button(Button::Attack);
        let attack = (r[0] + 10.0, r[1] + 10.0);
        for first_attack in [true, false] {
            at(&mut c, 1, TouchPhase::Started, attack, now);
            at(&mut c, 2, TouchPhase::Started, (400.0, 120.0), now);
            c.update(now + HOLD);
            let (first, second) = if first_attack {
                ((1, attack), (2, (400.0, 120.0)))
            } else {
                ((2, (400.0, 120.0)), (1, attack))
            };
            assert!(at(&mut c, first.0, TouchPhase::Ended, first.1, now + HOLD).is_empty());
            assert_eq!(
                at(&mut c, second.0, TouchPhase::Ended, second.1, now + HOLD),
                [Output::Mouse(MouseButton::Left, false)]
            );
        }
    }

    #[test]
    fn riding_controls_fit_to_left_of_use_without_overlapping_any_default() {
        for (width, height) in [(320.0, 180.0), (633.0, 292.0), (320.0, 600.0)] {
            for main_hand_left in [false, true] {
                let layout = Layout {
                    width,
                    height,
                    main_hand_left,
                };
                for button in [Button::Sprint, Button::Attack] {
                    let rect =
                        crate::touch_layout::clamp(layout.button(button), (width, height), false);
                    assert!(rect[0] + rect[2] < layout.button(Button::Use)[0]);
                    for other in Button::ALL.into_iter().filter(|b| *b != button) {
                        assert!(
                            !crate::touch_layout::overlaps(
                                rect,
                                crate::touch_layout::clamp(
                                    layout.button(other),
                                    (width, height),
                                    false
                                ),
                                0.0
                            ),
                            "{button:?} overlaps {other:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn resolved_geometry_moves_buttons_and_hidden_controls_take_no_touches() {
        let mut controls = TouchControls::default();
        let mut geometry = Geometry {
            buttons: [None; 10],
            stick: None,
            moved_stick: false,
        };
        geometry.buttons[0] = Some([100.0, 100.0, 32.0, 24.0]);
        controls.configure(geometry);
        let now = Instant::now();
        assert_eq!(
            at(&mut controls, 1, TouchPhase::Started, (110.0, 110.0), now),
            [Output::Key(KeyCode::Space, true)]
        );
        assert_eq!(
            at(&mut controls, 1, TouchPhase::Ended, (300.0, 110.0), now),
            [Output::Key(KeyCode::Space, false)]
        );
        // Jump's default box is no longer active. A hidden stick never starts movement.
        let r = LAYOUT.button(Button::Jump);
        assert!(
            at(
                &mut controls,
                2,
                TouchPhase::Started,
                (r[0] + 5.0, r[1] + 5.0),
                now
            )
            .is_empty()
        );
        assert!(!controls.pressed(Button::Jump));
        at(
            &mut controls,
            3,
            TouchPhase::Started,
            LAYOUT.stick_rest(),
            now,
        );
        assert!(controls.stick().is_none());
    }
    #[test]
    fn moved_stick_uses_its_new_centre_and_releases_movement() {
        let mut controls = TouchControls::default();
        controls.configure(Geometry {
            buttons: [None; 10],
            stick: Some([200.0, 80.0, 44.0, 44.0]),
            moved_stick: true,
        });
        let now = Instant::now();
        at(&mut controls, 1, TouchPhase::Started, (222.0, 102.0), now);
        assert_eq!(controls.stick(), Some(((222.0, 102.0), (222.0, 102.0))));
        let out = at(&mut controls, 1, TouchPhase::Moved, (222.0, 50.0), now);
        assert!(out.contains(&Output::Key(KeyCode::KeyW, true)));
        assert!(out.contains(&Output::Key(KeyCode::ControlLeft, true)));
        let out = at(&mut controls, 1, TouchPhase::Ended, (222.0, 50.0), now);
        assert!(out.contains(&Output::Key(KeyCode::KeyW, false)));
        assert!(out.contains(&Output::Key(KeyCode::ControlLeft, false)));
        at(
            &mut controls,
            2,
            TouchPhase::Started,
            LAYOUT.stick_rest(),
            now,
        );
        assert!(controls.stick().is_none());
    }
    #[test]
    fn top_row_and_inventory_follow_the_art_layout_and_main_arm() {
        for (button, rect) in [
            (Button::Perspective, [565.0, 54.0, 20.0, 20.0]),
            (Button::Chat, [587.0, 54.0, 20.0, 20.0]),
            (Button::Pause, [609.0, 54.0, 20.0, 20.0]),
            (Button::Jump, [595.0, 232.0, 32.0, 24.0]),
            (Button::Sneak, [595.0, 260.0, 32.0, 24.0]),
            (Button::Use, [559.0, 232.0, 32.0, 24.0]),
            (Button::Drop, [595.0, 204.0, 32.0, 24.0]),
        ] {
            assert_eq!(LAYOUT.button(button), rect);
            assert_eq!(
                LAYOUT.button_at(rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0),
                Some(button)
            );
        }
        assert_eq!(LAYOUT.button_at(619.0, 13.0), None);
        for (left, rect, centre) in [
            (false, [407.0, 269.0, 29.0, 24.0], (425.0, 281.0)),
            (true, [196.0, 269.0, 29.0, 24.0], (207.0, 281.0)),
        ] {
            let layout = Layout {
                main_hand_left: left,
                ..LAYOUT
            };
            assert_eq!(layout.button(Button::Inventory), rect);
            assert_eq!(layout.inventory_label_center(), centre);
            let mut controls = TouchControls::default();
            assert_eq!(
                controls.touch(
                    0,
                    TouchPhase::Started,
                    centre,
                    &layout,
                    true,
                    Instant::now()
                ),
                vec![
                    Output::Key(KeyCode::KeyE, true),
                    Output::Key(KeyCode::KeyE, false)
                ]
            );
        }
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
        assert!(c.latched(Button::Sneak));
        assert!(!c.touching(Button::Sneak));
        assert_eq!(
            at(&mut c, 3, TouchPhase::Started, sneak, t),
            vec![Output::Key(KeyCode::ShiftLeft, false)]
        );
        assert!(!c.latched(Button::Sneak));
        assert!(c.touching(Button::Sneak));
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
        assert!(c.update(t + POINTER_HOLD).is_empty());
        assert_eq!(
            menu(&mut c, 4, TouchPhase::Moved, (70.0, 60.0), t + POINTER_HOLD),
            vec![Output::PointerButton(true), Output::Pointer(70.0, 60.0)]
        );
        assert_eq!(
            menu(&mut c, 4, TouchPhase::Ended, (70.0, 60.0), t + POINTER_HOLD),
            vec![Output::PointerButton(false)]
        );
    }

    fn menu_at(
        controls: &mut TouchControls,
        phase: TouchPhase,
        p: (f32, f32),
        now: Instant,
    ) -> Vec<Output> {
        controls.touch(1, phase, p, &LAYOUT, false, now)
    }

    #[test]
    fn menu_tap_left_clicks_on_lift() {
        for duration in [
            Duration::from_millis(80),
            POINTER_HOLD,
            LONG_PRESS - Duration::from_millis(1),
        ] {
            let mut c = TouchControls::default();
            let t = Instant::now();
            assert_eq!(
                menu_at(&mut c, TouchPhase::Started, (50.0, 60.0), t),
                vec![Output::Pointer(50.0, 60.0)]
            );
            assert!(menu_at(&mut c, TouchPhase::Moved, (51.0, 60.0), t + duration).is_empty());
            assert!(c.update(t + duration).is_empty());
            assert_eq!(
                menu_at(&mut c, TouchPhase::Ended, (51.0, 60.0), t + duration),
                vec![Output::PointerButton(true), Output::PointerButton(false)]
            );
        }
    }

    #[test]
    fn menu_long_press_right_clicks_before_lift() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        menu_at(&mut c, TouchPhase::Started, (50.0, 60.0), t);
        assert!(c.update(t + POINTER_HOLD).is_empty());
        assert!(
            c.update(t + LONG_PRESS - Duration::from_millis(1))
                .is_empty()
        );
        assert_eq!(c.update(t + LONG_PRESS), vec![Output::PointerRight]);
        assert!(c.update(t + LONG_PRESS * 2).is_empty());
        assert!(menu_at(&mut c, TouchPhase::Ended, (50.0, 60.0), t + LONG_PRESS * 2).is_empty());
    }

    #[test]
    fn menu_hold_then_move_drags_until_lift() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        assert_eq!(
            menu_at(&mut c, TouchPhase::Started, (50.0, 60.0), t),
            vec![Output::Pointer(50.0, 60.0)]
        );
        assert!(c.update(t + POINTER_HOLD).is_empty());
        // Small movement keeps the pointer at its down position until the drag starts.
        assert!(menu_at(&mut c, TouchPhase::Moved, (51.0, 60.0), t + POINTER_HOLD).is_empty());
        assert_eq!(
            menu_at(&mut c, TouchPhase::Moved, (56.0, 60.0), t + POINTER_HOLD),
            vec![Output::PointerButton(true), Output::Pointer(56.0, 60.0)]
        );
        assert!(c.update(t + LONG_PRESS).is_empty());
        assert_eq!(
            menu_at(&mut c, TouchPhase::Moved, (70.0, 65.0), t + LONG_PRESS),
            vec![Output::Pointer(70.0, 65.0)]
        );
        assert_eq!(
            menu_at(&mut c, TouchPhase::Ended, (70.0, 65.0), t + LONG_PRESS),
            vec![Output::PointerButton(false)]
        );
    }

    #[test]
    fn menu_quick_swipe_scrolls_without_clicking() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        menu_at(&mut c, TouchPhase::Started, (50.0, 60.0), t);
        assert_eq!(
            menu_at(
                &mut c,
                TouchPhase::Moved,
                (50.0, 66.0),
                t + Duration::from_millis(50)
            ),
            vec![Output::Scroll(0.5)]
        );
        assert!(c.update(t + LONG_PRESS).is_empty());
        assert_eq!(
            menu_at(&mut c, TouchPhase::Moved, (50.0, 90.0), t + LONG_PRESS),
            vec![Output::Scroll(2.0)]
        );
        assert!(menu_at(&mut c, TouchPhase::Ended, (50.0, 90.0), t + LONG_PRESS).is_empty());
    }

    #[test]
    fn menu_long_press_spends_finger_before_movement() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        menu_at(&mut c, TouchPhase::Started, (50.0, 60.0), t);
        assert!(menu_at(&mut c, TouchPhase::Moved, (51.0, 60.0), t + POINTER_HOLD).is_empty());
        assert_eq!(c.update(t + LONG_PRESS), vec![Output::PointerRight]);
        assert!(menu_at(&mut c, TouchPhase::Moved, (70.0, 90.0), t + LONG_PRESS).is_empty());
        assert!(c.update(t + LONG_PRESS * 2).is_empty());
        assert!(menu_at(&mut c, TouchPhase::Moved, (50.0, 60.0), t + LONG_PRESS * 2).is_empty());
        assert!(menu_at(&mut c, TouchPhase::Ended, (50.0, 60.0), t + LONG_PRESS * 2).is_empty());
    }

    #[test]
    fn menu_cancelled_hold_does_not_click() {
        let mut c = TouchControls::default();
        let t = Instant::now();
        menu_at(&mut c, TouchPhase::Started, (50.0, 60.0), t);
        assert!(c.update(t + POINTER_HOLD).is_empty());
        assert!(
            menu_at(
                &mut c,
                TouchPhase::Cancelled,
                (50.0, 60.0),
                t + POINTER_HOLD
            )
            .is_empty()
        );
        assert!(c.update(t + LONG_PRESS).is_empty());
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
