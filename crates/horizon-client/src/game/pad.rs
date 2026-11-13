//! Controllers through gilrs (GameController on iOS; Horizon's own; docs/controllers.md): the
//! active pad's state each loop iteration, its button edges dispatched as
//! the vanilla key mappings they are bound to, the left stick as an analog
//! move vector and the right stick turning the view. `horizon-ctl pad`
//! injects a virtual pad on top of the real one.
use super::event_loop::EventLoopCtl;
use super::{Game, Screen};
use horizon_client::held::Source;
use horizon_client::{
    Result,
    controller_notice::{Glyph, Notice, NoticeId, Priority},
    controller_ui::Device,
    gamepad::{self, Button, Edges, GlyphStyle, HealthWatch, PadState, Route, Rumble},
    keymapping::SNEAK,
};
use serde_json::{Value, json};
use std::time::Instant;
use winit::keyboard::KeyCode;

pub(super) const HELP: &[&str] = &[
    "pad BUTTON[+BUTTON] down|up|tap   a virtual controller button or atomic chord (south east west north lb rb lt rt select start ls rs up down left right)",
    "pad stick left|right X Y       tilt a virtual stick (-1..1, up positive); 0 0 centres it",
    "pad trigger left|right VALUE   pull a virtual trigger (0..1)",
    "pad release                    centre and release the virtual controller",
    "pad status                     connected controllers, the active one and its state",
    "pad connection connected|disconnected   as if a controller came or went at this game's own reader",
];

/// The controllers and the state the game last acted on.
#[derive(Default)]
pub(super) struct Pads {
    /// Splitscreen: this player's pad, read by `PadRouter` for every game, instead of the pads.
    pub(super) feed: Option<PadState>,
    /// The pad behind `feed`.
    feed_source: Option<PadId>,
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
    activity: gamepad::Activity,
    context: Option<Context>,
    blocked_sticks: [bool; 2],
    resolver: gamepad::Resolver,
    clock: Option<Instant>,
    /// Dispatched holds, including quick taps kept until the next simulation tick.
    held_actions: std::collections::BTreeMap<String, u64>,
    late_releases: Vec<String>,
    /// The merged state of the last poll.
    pub(super) state: PadState,
    /// The hardware's buttons at the last poll, for `input-log`'s `pad`
    /// events: the merged state hides which source moved.
    last_hardware: PadState,
    /// The pad drove the game last (the mouse and keyboard hand control back).
    pub(super) in_use: bool,
    pub(super) style: GlyphStyle,
    /// The controller driving this player, or else the first one its own reader sees.
    pub(super) device: Option<Device>,
    /// Controllers that came and went at this game's own reader since the last poll.
    connections: Vec<Connection>,
    /// The cursor the pad drives in menus.
    pub(super) menu: super::pad_menu::MenuCursor,
    /// The last vibration asked for, for `pad status`.
    last_rumble: Option<(&'static str, gamepad::Pulse)>,
    /// The player's health between polls (a drop is damage taken).
    health: HealthWatch,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Context {
    screen: u64,
    focus_navigation: bool,
    focused: bool,
    osk: bool,
    dialog: bool,
    enabled: bool,
}

#[cfg(all(feature = "controllers", not(target_os = "ios")))]
mod hardware;

/// iOS: gilrs has no backend there; Apple's GameController framework reads the pads.
#[cfg(target_os = "ios")]
#[path = "pad/hardware_ios.rs"]
mod hardware;
#[cfg(test)]
mod parity_tests;
pub(super) use hardware::{PadId, PadRouter};

/// A controller coming or going, for this player's notice (`pad_connection_notice`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Connection {
    /// A controller of this glyph family connected.
    Connected(GlyphStyle),
    /// One disconnected; `active`: it was driving this player.
    Disconnected { active: bool },
}

/// What a controller did since the last frame.
// iOS reads no pads for splitscreen (`hardware_ios.rs` routes none).
#[cfg_attr(any(not(feature = "controllers"), target_os = "ios"), allow(dead_code))]
pub(in crate::game) enum PadEvent {
    Press(PadId, Button),
    /// Disconnected (out of battery, unplugged, out of range).
    Gone(PadId),
    /// Connected again under the same id (Windows keeps it; macOS gives a new one). On Linux
    /// gilrs gives a new device the slot of a disconnected one of the same model, so there this
    /// can be another controller of that model.
    Back(PadId),
}

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
        pub(in crate::game) fn release_hardware(&mut self) {}
    }

    pub(in crate::game) type PadId = usize;
    #[derive(Default)]
    pub(in crate::game) struct PadRouter {}
    impl PadRouter {
        pub(in crate::game) fn events(&mut self) -> Vec<super::PadEvent> {
            Vec::new()
        }
        pub(in crate::game) fn device(
            &self,
            _: PadId,
        ) -> Option<horizon_client::controller_ui::Device> {
            None
        }
        pub(in crate::game) fn lend_output(&mut self, _: &mut Pads, _: PadId) {}
        pub(in crate::game) fn reclaim_output(&mut self, _: &mut Pads) {}
        pub(in crate::game) fn connected(&self, _: PadId) -> bool {
            false
        }
        pub(in crate::game) fn uuid(&self, _: PadId) -> [u8; 16] {
            [0; 16]
        }
        pub(in crate::game) fn state(&self, _: PadId) -> PadState {
            PadState::default()
        }
        pub(in crate::game) fn name(&self, _: PadId) -> String {
            String::new()
        }
        pub(in crate::game) fn style(&self, _: PadId) -> horizon_client::gamepad::GlyphStyle {
            horizon_client::gamepad::GlyphStyle::Auto
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
                Some(s) if self.res.items().behavior(&s.item) == "minecraft:bow" => {
                    gamepad::TriggerFeel::Draw(gamepad::bow_power(ticks))
                }
                // TridentItem.THROW_THRESHOLD_TIME.
                Some(s) if self.res.items().behavior(&s.item) == "minecraft:trident" => {
                    gamepad::TriggerFeel::Draw(ticks as f32 / 10.0)
                }
                Some(s) if self.res.items().behavior(&s.item) == "minecraft:crossbow" => {
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
            .map(|s| (s, self.res.items().behavior(&s.item)))
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
    /// The digital buttons the person's pad changed since the last poll.
    pub(super) fn hardware_edges(&mut self, hardware: PadState) -> Vec<(Button, bool)> {
        let edges = Button::ALL
            .into_iter()
            .filter_map(|button| {
                let was = self.last_hardware.held(button);
                let now = hardware.held(button);
                (was != now).then_some((button, now))
            })
            .collect();
        self.last_hardware = hardware;
        edges
    }

    /// A replay's pad button, in place of `pad`'s line.
    pub(super) fn inject_button(&mut self, button: Button, down: bool) {
        self.injected.set(button, down);
        match button {
            Button::LeftTrigger => self.injected.triggers[0] = if down { 1.0 } else { 0.0 },
            Button::RightTrigger => self.injected.triggers[1] = if down { 1.0 } else { 0.0 },
            _ => {}
        }
    }

    /// Splitscreen: this player's pad state, from `source`. Another pad, or
    /// none: the old one's A going away isn't a release.
    pub(super) fn feed_from(&mut self, source: Option<PadId>, state: PadState) {
        if self.feed_source != source {
            self.feed_source = source;
            self.menu.armed = None;
        }
        self.feed = Some(state);
    }
    /// The device the controller settings name: a real one, else `horizon-ctl`'s virtual pad.
    pub(super) fn device_or_virtual(&self) -> Option<Device> {
        self.device.clone().or_else(|| {
            self.in_use.then(|| Device {
                name: "Virtual controller".to_owned(),
                vendor: None,
                product: None,
            })
        })
    }
    /// A replay ended: let go of everything it held.
    pub(super) fn clear_injected(&mut self) {
        self.menu.armed = None;
        self.injected = PadState::default();
    }

    /// Resolved holds only: raw chord members must never leak into movement or key handoff.
    pub(super) fn holds_action(&self, _settings: &gamepad::PadSettings, action: &str) -> bool {
        self.held_actions.contains_key(action)
    }

    /// A new player entity took over (spawn, respawn, a server joined, a
    /// dimension change): its health is not compared with the old one's.
    pub(super) fn player_replaced(&mut self) {
        self.health.forget();
    }

    /// The active pad is a Nintendo one going by its letters.
    fn by_letter(&self, settings: &gamepad::PadSettings) -> bool {
        self.style == GlyphStyle::Nintendo && settings.nintendo_letters
    }
}

impl Game<'_> {
    fn pad_context(&self) -> Context {
        Context {
            screen: self.key_screen(),
            focus_navigation: self.pad_navigation_mode(),
            focused: self.focused,
            osk: self.pad.menu.osk.is_some(),
            dialog: self.dialog_open() || self.code_of_conduct.is_some(),
            enabled: self.settings.controller.enabled,
        }
    }

    /// Once per loop iteration: read the pads and act on button changes.
    pub(super) fn poll_pads(&mut self, event_loop: &dyn EventLoopCtl) {
        self.pad.virtual_input = self.pad.injected != PadState::default();
        let context = self.pad_context();
        if let Some(old) = self.pad.context.filter(|old| *old != context) {
            if old.screen != context.screen {
                self.pad_remember_focus(old.screen);
            }
            self.pad_cancel_inputs(event_loop);
        }
        self.pad.context = Some(context);
        if !self.settings.controller.enabled {
            self.pad_output_off();
            self.pad_cancel_inputs(event_loop);
            self.pad.state = PadState::default();
            self.pad.in_use = false;
            // The keyboard's menu focus follows its widget all the same.
            self.pad_menu_frame(event_loop);
            return;
        }
        let hardware = match self.pad.feed {
            Some(state) => state,
            None => self.pad.read(),
        };
        // A Nintendo pad going by its letters is read that way: its own, or the one
        // splitscreen's router feeds this player.
        let hardware = if self.pad.by_letter(&self.settings.controller) {
            hardware.by_letter()
        } else {
            hardware
        };
        // A person's pad, for `input-log` and the repro recording; the virtual
        // pad is an agent's `pad` line and is recorded as that.
        let hardware_edges = self.pad.hardware_edges(hardware);
        for (button, down) in hardware_edges {
            self.note_input(
                super::input_trace::InputOrigin::User,
                super::input_trace::TraceKind::Pad {
                    button: button.name().to_owned(),
                    action: if down { "down" } else { "up" }.to_owned(),
                },
            );
        }
        for change in std::mem::take(&mut self.pad.connections) {
            self.pad_connection_notice(change);
        }
        let style = self.settings.controller.glyphs.resolve(self.pad.style);
        self.ui.options.pad_style = style;
        // Nintendo glyphs on a pad going by its letters name the button playing each part.
        self.ui.options.pad_by_letter =
            style == GlyphStyle::Nintendo && self.pad.by_letter(&self.settings.controller);
        self.ui.options.pad_driving = self.pad.in_use && self.pad_focus_mode();
        let mut state = hardware.merge(&self.pad.injected);
        let activity = self.pad.activity.observe(&state, &self.settings.controller);
        for (i, (stick, deadzone)) in [
            (&mut state.left, self.settings.controller.move_deadzone),
            (&mut state.right, self.settings.controller.look_deadzone),
        ]
        .into_iter()
        .enumerate()
        {
            if gamepad::radial(*stick, deadzone) == [0.0; 2] {
                self.pad.blocked_sticks[i] = false;
            }
            if self.pad.blocked_sticks[i] || !self.pad_focused() {
                *stick = [0.0; 2];
            }
        }
        for button in std::mem::take(&mut self.pad.taps) {
            self.pad.inject_button(button, false);
        }
        let threshold = self.settings.controller.trigger_threshold;
        let edges = self.pad.edges.update(&state, threshold);
        if activity && self.pad_focused() {
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
        for action in std::mem::take(&mut self.pad.late_releases) {
            if !ticking || self.pad.held_actions.get(&action) != Some(&self.ticks) {
                if let Err(e) = self.pad_action(&action, false) {
                    log::warn!("Horizon: releasing controller action {action}: {e}");
                }
            } else {
                self.pad.late_releases.push(action);
            }
        }
        // Gameplay edges are a batch so simultaneous overlapping chords use binding order.
        if !self.pad_in_menu() && self.pad_focused() {
            if !self.captured && self.focused && edges.iter().any(|(_, d)| *d) {
                self.capture(true);
            }
            let start_bound = self
                .pad_bindings()
                .iter()
                .any(|(_, b)| b.mask() & (1 << Button::Start as u32) != 0);
            if !start_bound && edges.contains(&(Button::Start, true)) {
                if let Err(e) = self.pad_back() {
                    log::warn!("Horizon: controller pause: {e}");
                }
            } else {
                let edges: Vec<_> = edges
                    .into_iter()
                    .filter(|(b, _)| *b != Button::Start || start_bound)
                    .collect();
                self.pad_resolve(&edges);
            }
            let changed = self
                .pad
                .context
                .is_some_and(|old| old != self.pad_context());
            if changed {
                self.pad_cancel_inputs(event_loop);
                self.pad.context = Some(self.pad_context());
            }
            self.pad_menu_frame(event_loop);
            return;
        }
        for (button, down) in edges {
            self.pad_button_logged(event_loop, button, down);
            let context = self.pad_context();
            if let Some(old) = self.pad.context.filter(|old| *old != context) {
                if old.screen != context.screen {
                    self.pad_remember_focus(old.screen);
                }
                self.pad_cancel_inputs(event_loop);
                self.pad.context = Some(context);
                break;
            }
        }
        if !self.pad_in_menu() && self.pad_focused() {
            self.pad_resolve(&[]);
        }
        self.pad_menu_frame(event_loop);
    }

    fn pad_bindings(&self) -> Vec<(String, gamepad::Binding)> {
        let mods = self.mod_pad_bindings();
        self.settings
            .controller
            .configured()
            .into_iter()
            .filter(|(a, _)| !a.starts_with("mod.") || mods.iter().any(|m| m.action == *a))
            .collect()
    }
    fn pad_resolve(&mut self, edges: &[(Button, bool)]) {
        let bindings = self.pad_bindings();
        let now = self.pad.clock.get_or_insert_with(Instant::now).elapsed();
        let events = self.pad.resolver.advance(now, edges, &bindings);
        let chord_members = events
            .iter()
            .filter(|(_, down)| *down)
            .filter_map(|(a, _)| {
                bindings.iter().find_map(|(name, b)| {
                    (name == a && matches!(b, gamepad::Binding::Chord(_, _))).then_some(b.mask())
                })
            })
            .fold(0, |mask, members| mask | members);
        let member_single = |action: &str| {
            bindings.iter().any(|(a, b)| {
                a == action
                    && matches!(b, gamepad::Binding::Single(_))
                    && b.mask() & chord_members != 0
            })
        };
        // A quick single tap may already await its simulation tick. A newly completed chord
        // must release that hold too, even though the pure resolver already emitted its up.
        for action in self.pad.late_releases.clone() {
            if member_single(&action) {
                self.pad.late_releases.retain(|a| a != &action);
                if let Err(e) = self.pad_action(&action, false) {
                    log::warn!("Horizon: releasing controller action {action}: {e}");
                }
            }
        }
        let context = self.pad_context();
        for (action, down) in events {
            if down && self.pad.late_releases.contains(&action) {
                self.pad.late_releases.retain(|a| a != &action);
                let _ = self.pad_action(&action, false);
            }
            let ticking = self.title.is_none() && self.screen == Screen::Playing && !self.pause;
            if !down
                && !member_single(&action)
                && !action.starts_with("mod.")
                && ticking
                && self.pad.held_actions.get(&action) == Some(&self.ticks)
            {
                if !self.pad.late_releases.contains(&action) {
                    self.pad.late_releases.push(action);
                }
            } else if let Err(e) = self.pad_action(&action, down) {
                log::warn!("Horizon: controller action {action}: {e}");
            }
            if self.pad_context() != context {
                break;
            }
        }
    }

    /// End held actions and captures before another context receives input.
    fn pad_cancel_inputs(&mut self, event_loop: &dyn EventLoopCtl) {
        // Before the releases below: a cancelled press must not click.
        self.pad.menu.armed = None;
        let held_sticks = [
            gamepad::radial(self.pad.state.left, self.settings.controller.move_deadzone)
                != [0.0; 2],
            gamepad::radial(self.pad.state.right, self.settings.controller.look_deadzone)
                != [0.0; 2],
        ];
        for (blocked, held) in self.pad.blocked_sticks.iter_mut().zip(held_sticks) {
            *blocked |= held;
        }
        self.pad.state.left = [0.0; 2];
        self.pad.state.right = [0.0; 2];
        self.pad.late_releases.clear();
        self.pad.edges.cancel();
        self.pad.resolver.cancel();
        for action in self.pad.held_actions.keys().cloned().collect::<Vec<_>>() {
            if let Err(e) = self.pad_action(&action, false) {
                log::warn!("Horizon: releasing controller action {action}: {e}");
            }
        }
        self.pad_cancel_menu(event_loop);
    }

    pub(super) fn pad_yield_input(&mut self, event_loop: &dyn EventLoopCtl) {
        if self.pad.in_use {
            self.pad.in_use = false;
            self.pad_cancel_inputs(event_loop);
        }
    }

    fn pad_button_logged(&mut self, event_loop: &dyn EventLoopCtl, button: Button, down: bool) {
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
        event_loop: &dyn EventLoopCtl,
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
            return self.pad_menu_button(event_loop, button, down);
        }
        // Start remains the ordinary pause shortcut unless explicitly used in a binding.
        if button == Button::Start
            && !self
                .pad_bindings()
                .iter()
                .any(|(_, b)| b.mask() & (1 << Button::Start as u32) != 0)
        {
            return if down { self.pad_back() } else { Ok(()) };
        }
        self.pad_resolve(&[(button, down)]);
        Ok(())
    }

    /// A bound action: a vanilla mapping acts as its default key or mouse
    /// button, like a rebound keyboard key.
    fn pad_action(&mut self, action: &str, down: bool) -> Result<()> {
        if down {
            self.pad.held_actions.insert(action.to_owned(), self.ticks);
        } else {
            self.pad.held_actions.remove(action);
        }
        if action.starts_with("mod.") {
            self.mod_pad_key(action, down);
            return Ok(());
        }
        let Some(route) = gamepad::route(action) else {
            log::warn!("Horizon: controller action {action} has no handler");
            return Ok(());
        };
        match route {
            Route::HotbarStep { next } => {
                if down && self.screen == Screen::Playing {
                    self.selected = gamepad::hotbar_step(self.selected, next);
                }
                Ok(())
            }
            Route::Sneak
                if self.settings.controller.toggle_sneak && !self.settings.toggle_crouch =>
            {
                // A stick click flips sneaking instead of holding it, as
                // vanilla's toggle crouch does: a screen opening releases it
                // and closing restores it (ToggleKeyMapping).
                if !down {
                    return Ok(());
                }
                let screen_open = self.screen != Screen::Playing || self.title.is_some();
                let was = self.keys.contains(&KeyCode::ShiftLeft);
                self.toggles.pad_toggle(SNEAK, screen_open);
                self.sync_toggle(SNEAK);
                if !was && self.keys.contains(&KeyCode::ShiftLeft) {
                    // The pad's latch: a keyboard handoff leaves it.
                    self.keys.press(KeyCode::ShiftLeft, Source::Pad);
                    self.keys.release(&KeyCode::ShiftLeft, Source::Keyboard);
                }
                self.key_pressed(KeyCode::ShiftLeft, KeyCode::ShiftLeft)
            }
            Route::Sneak => self.mapped_key(
                KeyCode::ShiftLeft,
                down,
                false,
                KeyCode::ShiftLeft,
                Source::Pad,
            ),
            Route::Screenshot => {
                if down {
                    self.screenshot_action();
                }
                Ok(())
            }
            Route::SmoothCamera => {
                if down {
                    self.smooth_camera_action();
                }
                Ok(())
            }
            Route::Mouse(button) => {
                if !down || (self.title.is_none() && self.screen == Screen::Playing) {
                    self.play_mouse(button, down)?;
                }
                Ok(())
            }
            Route::Key(key) => self.mapped_key(key, down, false, key, Source::Pad),
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
        self.key_pressed(KeyCode::Escape, KeyCode::Escape)
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
        // look_delta returns degrees; look_turn takes Entity.turn units (0.15 degrees).
        let (x, y) = self.assisted_turn(
            (f64::from(yaw / 0.15), f64::from(pitch / 0.15)),
            seconds,
            true,
        );
        self.device_look_turn(x, y, super::aim_assist::LookDevice::Controller);
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
    /// A controller came or went with no splitscreen router: say so on this view, in its one
    /// notice. Nothing here decides what the controller drives; a player who turned controllers
    /// off isn't told about them.
    fn pad_connection_notice(&mut self, change: Connection) {
        if !self.settings.controller.enabled {
            return;
        }
        let split =
            |key: &str| horizon_client::lang::get(&format!("horizon.splitscreen.prompt.{key}"));
        let notice = match change {
            Connection::Connected(family) => Notice::new(
                NoticeId::Connection,
                Priority::Info,
                split("controller_connected"),
                horizon_client::lang::get("horizon.pad.notice.connected.press"),
            )
            .with_glyph(Glyph {
                button: Button::South,
                style: self.settings.controller.glyphs.resolve(family),
            }),
            Connection::Disconnected { active } => Notice::new(
                NoticeId::Connection,
                if active {
                    Priority::Attention
                } else {
                    Priority::Info
                },
                split("controller_disconnected"),
                split("controller_disconnected.hint"),
            ),
        };
        self.split_hud.notice.raise(notice);
    }

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
                    "notice": self.split_hud.notice.showing().map(|(title, body)| {
                        json!({ "title": title, "body": body })
                    }),
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
            "connection" => {
                let change = match arg(2)? {
                    "connected" => Connection::Connected(GlyphStyle::PlayStation),
                    "disconnected" => Connection::Disconnected { active: true },
                    other => {
                        return Err(format!(
                            "connection must be connected or disconnected, not {other}"
                        )
                        .into());
                    }
                };
                self.pad.connections.push(change);
                Ok(json!({ "ok": true }))
            }
            "release" => {
                self.pad.menu.armed = None;
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
                let binding = gamepad::Binding::parse(name)
                    .ok_or_else(|| format!("unknown pad button or chord {name}; {}", HELP[0]))?;
                let down = match arg(2)? {
                    "down" | "tap" => true,
                    "up" => false,
                    other => {
                        return Err(
                            format!("pad action must be down, up or tap, not {other}").into()
                        );
                    }
                };
                for button in Button::ALL {
                    if binding.mask() & (1 << button as u32) == 0 {
                        continue;
                    }
                    self.pad.inject_button(button, down);
                    if arg(2)? == "tap" {
                        self.pad.taps.push(button);
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
