//! Controllers through gilrs (GameController on iOS; Horizon's own; docs/controllers.md): the
//! active pad's state each loop iteration, its button edges dispatched as
//! the vanilla key mappings they are bound to, the left stick as an analog
//! move vector and the right stick turning the view. `horizon-ctl pad`
//! injects a virtual pad on top of the real one.
use super::{Game, Screen};
use horizon_client::{
    Result,
    gamepad::{self, Button, Edges, GlyphStyle, HealthWatch, PadState, Rumble},
    keybinds::{KeyBinds, key_code, mouse_button},
    mouse::Rotation,
};
use serde_json::{Value, json};
use std::time::Instant;
use winit::{event_loop::ActiveEventLoop, keyboard::KeyCode};

pub(super) const HELP: &[&str] = &[
    "pad BUTTON down|up|tap         a virtual controller button (south east west north lb rb lt rt select start ls rs up down left right)",
    "pad stick left|right X Y       tilt a virtual stick (-1..1, up positive); 0 0 centres it",
    "pad trigger left|right VALUE   pull a virtual trigger (0..1)",
    "pad release                    centre and release the virtual controller",
    "pad status                     connected controllers, the active one and its state",
];

/// The controllers and the state the game last acted on.
#[derive(Default)]
pub(super) struct Pads {
    /// The real controllers and the DualSense's output.
    #[cfg(any(feature = "controllers", target_os = "ios"))]
    devices: hardware::Devices,
    /// The virtual pad of `horizon-ctl pad`.
    injected: PadState,
    /// The virtual pad was in use at the last poll.
    virtual_input: bool,
    /// Buttons a `pad ... tap` releases at the next poll.
    taps: Vec<Button>,
    edges: Edges,
    /// The game tick each button went down in, and releases held back until
    /// a tick has passed: vanilla samples keys once per tick, so a click
    /// shorter than that (a quick stick click for sprint) would be lost.
    pressed_at: [Option<u64>; 16],
    late_releases: Vec<Button>,
    /// The merged state of the last poll.
    pub(super) state: PadState,
    /// The pad drove the game last (the mouse and keyboard hand control back).
    pub(super) in_use: bool,
    pub(super) style: GlyphStyle,
    /// The cursor the pad drives in menus.
    pub(super) menu: super::pad_menu::MenuCursor,
    /// The last vibration asked for, for `pad status`.
    last_rumble: Option<(&'static str, gamepad::Pulse)>,
    /// The player's health between polls (a drop is damage taken).
    health: HealthWatch,
}

#[cfg(all(feature = "controllers", not(target_os = "ios")))]
mod hardware;

/// iOS: gilrs has no backend there; Apple's GameController framework reads the pads.
#[cfg(target_os = "ios")]
#[path = "pad/hardware_ios.rs"]
mod hardware;

/// A build without the `controllers` feature (the server build, which must not need libudev): no
/// hardware pads, only `horizon-ctl pad`'s virtual one.
#[cfg(not(any(feature = "controllers", target_os = "ios")))]
mod hardware {
    use super::{super::Game, Pads};
    use horizon_client::gamepad::{PadState, Pulse, Rumble};
    use serde_json::{Value, json};

    impl Pads {
        pub(super) fn read(&mut self) -> PadState {
            PadState::default()
        }
    }

    impl Game<'_> {
        pub(super) fn pad_output_off(&mut self) {}
        pub(super) fn pad_output_frame(&mut self) {}
        pub(in crate::game) fn pad_close_output(&mut self) {}
        pub(super) fn pad_vibrate(&mut self, _: Rumble, _: Pulse) {}
        pub(super) fn pad_devices_status(&self) -> Value {
            json!({ "backend": false, "controllers": [] })
        }
    }
}

/// What the adaptive triggers feel: the DualSense's own output on desktop, Apple's API on iOS.
#[cfg(any(feature = "controllers", target_os = "ios"))]
impl Game<'_> {
    /// What `key.use` and `key.attack` are doing, for the triggers bound to them.
    pub(super) fn trigger_feels(&self) -> (gamepad::TriggerFeel, gamepad::TriggerFeel) {
        let using = match self.using {
            Some((slot, ticks)) => match self.stack_in(slot) {
                Some(s) if &*s.item == "minecraft:bow" => {
                    gamepad::TriggerFeel::Draw(gamepad::bow_power(ticks))
                }
                // TridentItem.THROW_THRESHOLD_TIME.
                Some(s) if &*s.item == "minecraft:trident" => {
                    gamepad::TriggerFeel::Draw(ticks as f32 / 10.0)
                }
                Some(s) if &*s.item == "minecraft:crossbow" => {
                    gamepad::TriggerFeel::Draw(ticks as f32 / Self::charge_ticks(s).max(1) as f32)
                }
                Some(s) if self.blocks_attacks(s).is_some() => gamepad::TriggerFeel::Brace,
                _ => gamepad::TriggerFeel::Free,
            },
            None => self.idle_use_feel(),
        };
        // Creative breaks at once; blocks that can't be broken have a negative hardness.
        let attacking = match &self.mining {
            Some(m) if !self.creative => {
                let state = self.world.block(m.pos[0], m.pos[1], m.pos[2]);
                gamepad::TriggerFeel::Texture(self.res.facts().get(state).hardness)
            }
            _ => gamepad::TriggerFeel::Free,
        };
        (using, attacking)
    }

    /// The use trigger while nothing is in use: a loaded crossbow clicks, an item that boosts (a
    /// rocket while gliding, the stick of the mount being ridden) catches, a shield weighs, and
    /// locks while an axe has disabled it.
    fn idle_use_feel(&self) -> gamepad::TriggerFeel {
        let gliding = self.player.as_ref().is_some_and(|p| p.fall_flying);
        let mount = self
            .riding
            .and_then(|id| self.mobs.iter().find(|m| m.id == id));
        let mount = mount.map_or("", |m| m.kind.id.as_str());
        let feel = |stack: Option<&horizon_client::items::ItemStack>| match stack
            .map(|s| (s, &*s.item))
        {
            Some((s, "minecraft:crossbow")) if s.data().is_some_and(|d| !d.charged.is_empty()) => {
                gamepad::TriggerFeel::Click
            }
            Some((_, "minecraft:firework_rocket")) if gliding => gamepad::TriggerFeel::Boost,
            Some((_, "minecraft:carrot_on_a_stick")) if mount == "minecraft:pig" => {
                gamepad::TriggerFeel::Boost
            }
            Some((_, "minecraft:warped_fungus_on_a_stick")) if mount == "minecraft:strider" => {
                gamepad::TriggerFeel::Boost
            }
            Some((s, _)) if self.blocks_attacks(s).is_some() => match self.shield_cooldown {
                0 => gamepad::TriggerFeel::Brace,
                _ => gamepad::TriggerFeel::Locked,
            },
            _ => gamepad::TriggerFeel::Free,
        };
        // Use tries the main hand first.
        match feel(self.stack_in(self.selected)) {
            gamepad::TriggerFeel::Free => feel(self.offhand.as_ref()),
            main => main,
        }
    }
}

impl Pads {
    /// A new player entity took over (spawn, respawn, a server joined, a
    /// dimension change): its health is not compared with the old one's.
    pub(super) fn player_replaced(&mut self) {
        self.health.forget();
    }
}

impl Game<'_> {
    /// Once per loop iteration: read the pads and act on button changes.
    pub(super) fn poll_pads(&mut self, event_loop: &ActiveEventLoop) {
        let settings = &self.settings.controller;
        if !settings.enabled {
            self.pad_output_off();
            if self.pad.state != PadState::default() {
                self.pad.state = PadState::default();
                for (button, down) in self.pad.edges.release_all() {
                    self.pad_edge(event_loop, button, down);
                }
            }
            return;
        }
        let hardware = self.pad.read();
        self.ui.options.pad_style = self.settings.controller.glyphs.resolve(self.pad.style);
        let state = hardware.merge(&self.pad.injected);
        self.pad.virtual_input = self.pad.injected != PadState::default();
        for button in std::mem::take(&mut self.pad.taps) {
            self.pad.injected.set(button, false);
        }
        let threshold = self.settings.controller.trigger_threshold;
        let edges = self.pad.edges.update(&state, threshold);
        if state.active(&self.settings.controller) {
            self.last_input = Instant::now();
            self.pad.in_use = true;
            #[cfg(target_os = "ios")]
            self.touch_hide(event_loop);
        }
        self.pad.state = state;
        self.pad_output_frame();
        // Damage from any source, here or on a server; nothing counts on
        // the title screen.
        let health = self
            .player
            .as_ref()
            .filter(|_| self.title.is_none())
            .map(|p| p.health.health());
        if let Some(damage) = self.pad.health.observe(health) {
            self.pad_rumble(Rumble::Hurt(damage));
        }
        let ticking = self.title.is_none() && self.screen == Screen::Playing && !self.pause;
        for button in std::mem::take(&mut self.pad.late_releases) {
            if !ticking || self.pad.pressed_at[button as usize] != Some(self.ticks) {
                self.pad_button_logged(event_loop, button, false);
            } else {
                self.pad.late_releases.push(button);
            }
        }
        for (button, down) in edges {
            self.pad_edge(event_loop, button, down);
        }
        self.pad_menu_frame(event_loop);
    }

    fn pad_edge(&mut self, event_loop: &ActiveEventLoop, button: Button, down: bool) {
        let ticking = self.title.is_none() && self.screen == Screen::Playing && !self.pause;
        if down {
            // Pressed again before its held-back release went out.
            if let Some(i) = self.pad.late_releases.iter().position(|b| *b == button) {
                self.pad.late_releases.remove(i);
                self.pad_button_logged(event_loop, button, false);
            }
            self.pad.pressed_at[button as usize] = Some(self.ticks);
        } else if ticking && self.pad.pressed_at[button as usize] == Some(self.ticks) {
            self.pad.late_releases.push(button);
            return;
        }
        self.pad_button_logged(event_loop, button, down);
    }

    fn pad_button_logged(&mut self, event_loop: &ActiveEventLoop, button: Button, down: bool) {
        if let Err(e) = self.pad_button(event_loop, button, down) {
            log::warn!("Horizon: controller {}: {e}", button.name());
        }
    }

    /// Input from the pad counts while the window has focus, or always for
    /// the virtual pad (a script can't focus the window).
    fn pad_focused(&self) -> bool {
        self.focused || self.pad.virtual_input
    }

    fn pad_button(
        &mut self,
        event_loop: &ActiveEventLoop,
        button: Button,
        down: bool,
    ) -> Result<()> {
        if down && !self.pad_focused() {
            return Ok(());
        }
        let playing = !self.pad_in_menu();
        if down && playing && !self.captured && self.focused {
            // Like a click into the window: take the pointer.
            self.capture(true);
        }
        if !playing {
            self.pad_menu_button(event_loop, button, down)?;
            if down {
                return Ok(());
            }
        } else if button == Button::Start {
            return if down { self.pad_back() } else { Ok(()) };
        }
        // Releases always reach the mappings, so nothing stays held when a
        // screen opened in between.
        for action in self.settings.controller.actions_of(button) {
            self.pad_action(action, down)?;
        }
        Ok(())
    }

    /// A bound action: a vanilla mapping acts as its default key or mouse
    /// button, like a rebound keyboard key.
    fn pad_action(&mut self, action: &str, down: bool) -> Result<()> {
        match action {
            gamepad::HOTBAR_PREVIOUS | gamepad::HOTBAR_NEXT => {
                if down && self.screen == Screen::Playing {
                    self.selected =
                        gamepad::hotbar_step(self.selected, action == gamepad::HOTBAR_NEXT);
                }
                Ok(())
            }
            "key.sneak"
                if self.settings.controller.toggle_sneak && !self.settings.toggle_crouch =>
            {
                // A stick click flips sneaking instead of holding it.
                if down {
                    let sneaking = self.keys.contains(&KeyCode::ShiftLeft);
                    self.mapped_key(KeyCode::ShiftLeft, !sneaking, false)?;
                }
                Ok(())
            }
            _ => {
                let default = KeyBinds::default_of(action);
                if let Some(button) = mouse_button(default) {
                    if self.title.is_none() && self.screen == Screen::Playing {
                        self.play_mouse(button, down)?;
                    }
                    Ok(())
                } else if let Some(key) = key_code(default) {
                    self.mapped_key(key, down, false)
                } else {
                    Ok(())
                }
            }
        }
    }

    /// B and Start: what Escape does where the pad is.
    pub(super) fn pad_back(&mut self) -> Result<()> {
        // The options open over the title screen too: they come first, as
        // for the keyboard's Escape.
        if self.screen == Screen::Options {
            return self.options_key(KeyCode::Escape, None);
        }
        if let Some(title) = self.title.as_mut() {
            title.escape();
            self.save_title_options();
            return Ok(());
        }
        if self.sign_edit.is_some() {
            self.close_sign_editor();
            return Ok(());
        }
        if self.book.is_some() {
            self.close_book();
            return Ok(());
        }
        self.key_pressed(KeyCode::Escape)
    }

    /// The left stick as the move vector, while playing.
    pub(super) fn pad_move(&self) -> Option<[f32; 2]> {
        let settings = &self.settings.controller;
        if !settings.enabled || self.pad_in_menu() || !self.pad_focused() {
            return None;
        }
        gamepad::move_vector(self.pad.state.left, settings.move_deadzone)
    }

    /// The right stick turns the view, once per frame (`Entity.turn`).
    pub(super) fn pad_look(&mut self, seconds: f32) {
        let settings = &self.settings.controller;
        if !settings.enabled || self.pad_in_menu() || self.player.is_none() || !self.pad_focused() {
            return;
        }
        // A long frame (a hitch) must not spin the view.
        let (yaw, pitch) = gamepad::look_delta(
            self.pad.state.right,
            settings,
            seconds.min(0.1),
            self.scoping(),
        );
        if yaw == 0.0 && pitch == 0.0 {
            return;
        }
        let mut rotation = Rotation {
            x_rot: self.pitch,
            y_rot: self.yaw,
            x_rot_o: self.pitch,
            y_rot_o: self.yaw,
        };
        rotation.turn(f64::from(yaw / 0.15), f64::from(pitch / 0.15));
        self.yaw = rotation.y_rot;
        self.pitch = rotation.x_rot;
    }

    /// Vibrate the active pad, when vibration is on and the pad is the one
    /// playing (a pad lying idle while the mouse plays stays still).
    pub(super) fn pad_rumble(&mut self, rumble: Rumble) {
        let settings = &self.settings.controller;
        if !settings.enabled || !settings.vibration || !self.pad.in_use {
            return;
        }
        let Some(pulse) = rumble.pulse() else {
            return;
        };
        self.pad.last_rumble = Some((rumble.name(), pulse));
        self.pad_vibrate(rumble, pulse);
    }

    /// `horizon-ctl pad ...`.
    pub(super) fn pad_command(&mut self, words: &[&str]) -> Result<Value> {
        let arg = |i: usize| -> Result<&str> {
            words
                .get(i)
                .copied()
                .ok_or_else(|| format!("usage: {}", HELP[0]).into())
        };
        let number = |i: usize| -> Result<f32> {
            let v: f32 = arg(i)?.parse()?;
            if v.is_finite() {
                Ok(v)
            } else {
                Err("not a number".into())
            }
        };
        match arg(1)? {
            "status" => {
                let s = &self.pad.state;
                let held: Vec<&str> = Button::ALL
                    .into_iter()
                    .filter(|b| self.pad.edges.is_held(*b))
                    .map(Button::name)
                    .collect();
                let mut status = json!({
                    "enabled": self.settings.controller.enabled,
                    "glyphs": self.pad.style.name(),
                    "last_rumble": self.pad.last_rumble.map(|(why, p)| json!({
                        "reason": why, "strong": p.strong, "weak": p.weak, "ms": p.millis,
                    })),
                    "in_use": self.pad.in_use,
                    "menu": self.pad_menu_status(),
                    "held": held,
                    "left": s.left,
                    "right": s.right,
                    "triggers": s.triggers,
                });
                if let (Some(status), Value::Object(devices)) =
                    (status.as_object_mut(), self.pad_devices_status())
                {
                    status.extend(devices);
                }
                Ok(status)
            }
            "release" => {
                self.pad.injected = PadState::default();
                self.pad.taps.clear();
                Ok(json!({ "ok": true }))
            }
            "stick" => {
                let v = [number(3)?.clamp(-1.0, 1.0), number(4)?.clamp(-1.0, 1.0)];
                match arg(2)? {
                    "left" => self.pad.injected.left = v,
                    "right" => self.pad.injected.right = v,
                    other => return Err(format!("stick must be left or right, not {other}").into()),
                }
                Ok(json!({ "ok": true }))
            }
            "trigger" => {
                let v = number(3)?.clamp(0.0, 1.0);
                match arg(2)? {
                    "left" => self.pad.injected.triggers[0] = v,
                    "right" => self.pad.injected.triggers[1] = v,
                    other => {
                        return Err(format!("trigger must be left or right, not {other}").into());
                    }
                }
                Ok(json!({ "ok": true }))
            }
            name => {
                let button = Button::parse(name)
                    .ok_or_else(|| format!("unknown pad button {name}; {}", HELP[0]))?;
                match arg(2)? {
                    "down" => self.pad.injected.set(button, true),
                    "up" => self.pad.injected.set(button, false),
                    "tap" => {
                        self.pad.injected.set(button, true);
                        self.pad.taps.push(button);
                    }
                    other => {
                        return Err(
                            format!("pad action must be down, up or tap, not {other}").into()
                        );
                    }
                }
                Ok(json!({ "ok": true }))
            }
        }
    }
}

/// A controller on the startup title, before the game exists (the title shown while resources
/// load): the left stick or the D-pad moves a pointer, A clicks, B goes back, the right stick
/// scrolls, as pointer mode does in `pad_menu.rs` once the game is up. Its pads go to the game.
#[derive(Default)]
pub(super) struct StartupPad {
    pub(super) pads: Pads,
    /// The pointer, in GUI pixels, once the pad has moved it.
    pub(super) pos: Option<(f32, f32)>,
    last: Option<Instant>,
}

/// What the startup title does with a poll.
pub(super) enum StartupInput {
    /// Pointer events, as a mouse sends them.
    Events(Vec<winit::event::WindowEvent>),
    /// B: Esc.
    Back,
}

impl StartupPad {
    /// Reads the pads: `size` is the screen in GUI pixels, `scale` physical pixels per GUI pixel.
    pub(super) fn poll(
        &mut self,
        settings: &gamepad::PadSettings,
        size: (f32, f32),
        scale: f32,
    ) -> Vec<StartupInput> {
        use winit::dpi::PhysicalPosition;
        use winit::event::{
            DeviceId, ElementState, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
        };
        let now = Instant::now();
        let seconds = self
            .last
            .replace(now)
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.1));
        if !settings.enabled {
            return Vec::new();
        }
        let state = self.pads.read();
        let previous = self.pads.state;
        self.pads.state = state;
        // Kept current, so a button held when the game takes over is no new press there.
        let _ = self.pads.edges.update(&state, settings.trigger_threshold);
        if !state.active(settings) && state == previous {
            return Vec::new();
        }
        self.pads.in_use = true;
        let pressed = |b: Button| state.held(b) && !previous.held(b);
        let released = |b: Button| !state.held(b) && previous.held(b);
        let mut out = Vec::new();
        let mut events = Vec::new();
        // GUI pixels a second at full tilt: the screen's height in about a second and a half.
        let speed = size.1 / 1.5;
        let mut motion = [0.0f32; 2];
        if state.left[0].hypot(state.left[1]) > settings.move_deadzone {
            motion = [state.left[0], -state.left[1]];
        }
        for (button, dx, dy) in [
            (Button::DPadLeft, -1.0, 0.0),
            (Button::DPadRight, 1.0, 0.0),
            (Button::DPadUp, 0.0, -1.0),
            (Button::DPadDown, 0.0, 1.0),
        ] {
            if state.held(button) {
                motion[0] += dx * 0.6;
                motion[1] += dy * 0.6;
            }
        }
        let start = self.pos.unwrap_or((size.0 / 2.0, size.1 / 2.0));
        let at = (
            (start.0 + motion[0] * speed * seconds).clamp(0.0, size.0 - 1.0),
            (start.1 + motion[1] * speed * seconds).clamp(0.0, size.1 - 1.0),
        );
        if self.pos != Some(at) {
            self.pos = Some(at);
            events.push(WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(f64::from(at.0 * scale), f64::from(at.1 * scale)),
            });
        }
        for (down, state) in [
            (pressed(Button::South), ElementState::Pressed),
            (released(Button::South), ElementState::Released),
        ] {
            if down {
                events.push(WindowEvent::MouseInput {
                    device_id: DeviceId::dummy(),
                    state,
                    button: MouseButton::Left,
                });
            }
        }
        if state.right[1].abs() > settings.look_deadzone {
            events.push(WindowEvent::MouseWheel {
                device_id: DeviceId::dummy(),
                delta: MouseScrollDelta::LineDelta(0.0, state.right[1] * 10.0 * seconds),
                phase: TouchPhase::Moved,
            });
        }
        if !events.is_empty() {
            out.push(StartupInput::Events(events));
        }
        if pressed(Button::East) {
            out.push(StartupInput::Back);
        }
        out
    }
}
