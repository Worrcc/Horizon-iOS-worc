//! The real controllers on iOS: every pad Apple's GameController framework connects (Xbox,
//! PlayStation, Switch Pro and MFi pads), merged, as gilrs' active pad is elsewhere. Vibration
//! plays through Core Haptics on every pad that has it; the lightbar and the DualSense's adaptive
//! triggers show what the game is doing, as the DualSense's own output does on desktop.
use super::super::Screen;
use super::{super::Game, Pads};
use horizon_client::gamepad::{self, Button, GlyphStyle, PadState, Pulse, Rumble, TriggerFeel};
use horizon_dualsense::trigger::{self, Mode};
use horizon_ios::controllers::Trigger;
use serde_json::{Value, json};

/// The pads at the last read, for `pad status`.
#[derive(Default)]
pub(super) struct Devices {
    pads: Vec<horizon_ios::controllers::Gamepad>,
    /// What the lightbar and triggers were last set to.
    shown: Option<Shown>,
}

#[derive(Clone, Copy, PartialEq)]
struct Shown {
    lightbar: Option<[u8; 3]>,
    triggers: [Trigger; 2],
}

/// An effect in the DualSense's zones (0..9) and levels (1..8) as Apple's 0..1 positions and
/// strengths. Apple's frequency is 0..1 too: the pad's Hz over 255, its byte's range.
fn apple_trigger(effect: &horizon_dualsense::TriggerEffect) -> Trigger {
    let position = |zone: u8| f32::from(zone) / 9.0;
    let level = |l: u8| f32::from(l) / 8.0;
    match trigger::decode(effect) {
        Mode::Off => Trigger::Off,
        Mode::Feedback { start, strength } => Trigger::Feedback {
            start: position(start),
            strength: level(strength),
        },
        Mode::Weapon {
            start,
            end,
            strength,
        } => Trigger::Weapon {
            start: position(start),
            end: position(end),
            strength: level(strength),
        },
        Mode::Vibration {
            start,
            amplitude,
            frequency,
        } => Trigger::Vibration {
            start: position(start),
            amplitude: level(amplitude),
            frequency: f32::from(frequency) / 255.0,
        },
    }
}

fn glyphs(category: &str) -> GlyphStyle {
    if category.contains("DualSense") || category.contains("DualShock") {
        GlyphStyle::PlayStation
    } else if category.contains("Switch") || category.contains("Joy-Con") {
        GlyphStyle::Nintendo
    } else {
        GlyphStyle::Xbox
    }
}

impl Pads {
    pub(super) fn read(&mut self) -> PadState {
        self.devices.pads = horizon_ios::controllers::gamepads();
        let mut merged = PadState::default();
        for pad in &self.devices.pads {
            let mut state = PadState::default();
            let buttons = [
                (Button::South, pad.face[0]),
                (Button::East, pad.face[1]),
                (Button::West, pad.face[2]),
                (Button::North, pad.face[3]),
                (Button::LeftBumper, pad.shoulders[0]),
                (Button::RightBumper, pad.shoulders[1]),
                (Button::Start, pad.menu),
                (Button::Select, pad.options),
                (Button::LeftStick, pad.thumbs[0]),
                (Button::RightStick, pad.thumbs[1]),
                (Button::DPadUp, pad.dpad[0]),
                (Button::DPadDown, pad.dpad[1]),
                (Button::DPadLeft, pad.dpad[2]),
                (Button::DPadRight, pad.dpad[3]),
            ];
            for (button, down) in buttons {
                state.set(button, down);
            }
            state.left = pad.left;
            state.right = pad.right;
            state.triggers = pad.triggers;
            if state != PadState::default() {
                self.style = glyphs(&pad.category);
            }
            merged = merged.merge(&state);
        }
        merged
    }
}

impl Game<'_> {
    /// Controllers are turned off: the triggers go free.
    pub(super) fn pad_output_off(&mut self) {
        if self.pad.devices.shown.take().is_some() {
            horizon_ios::controllers::show(None, [Trigger::Off; 2]);
        }
    }

    /// After each poll, once a pad is in use: the lightbar follows the player and the triggers
    /// feel what their actions do while playing (free in menus or paused).
    pub(super) fn pad_output_frame(&mut self) {
        if !self.pad.in_use {
            return;
        }
        let playing = self.title.is_none() && self.screen == Screen::Playing && !self.pause;
        let (using, attacking) = if playing {
            self.trigger_feels()
        } else {
            (TriggerFeel::Free, TriggerFeel::Free)
        };
        let in_world = self.title.is_none();
        let lightbar = match self.player.as_mut().filter(|_| in_world) {
            Some(p) => {
                let max = p.attribute("minecraft:max_health").unwrap_or(20.0) as f32;
                let health = p.health.health() / max.max(1.0);
                self.settings
                    .controller
                    .lightbar
                    .colour(health, self.dim.id())
            }
            None => self.pad.devices.shown.and_then(|s| s.lightbar),
        };
        let lights = self.settings.controller.lightbar != gamepad::Lightbar::Off;
        let shown = Shown {
            lightbar: lightbar.filter(|_| lights),
            triggers: self
                .settings
                .controller
                .trigger_effects(using, attacking)
                .map(|e| apple_trigger(&e)),
        };
        if self.pad.devices.shown != Some(shown) {
            horizon_ios::controllers::show(shown.lightbar, shown.triggers);
            self.pad.devices.shown = Some(shown);
        }
    }
    pub(in crate::game) fn pad_close_output(&mut self) {}
    pub(super) fn pad_vibrate(&mut self, rumble: Rumble, _: Pulse) {
        let steps: Vec<_> = rumble
            .shape()
            .into_iter()
            .map(|step| (step.strong, step.weak, step.millis))
            .collect();
        horizon_ios::controllers::vibrate(&steps);
    }
    pub(super) fn pad_devices_status(&self) -> Value {
        let pads: Vec<Value> = self
            .pad
            .devices
            .pads
            .iter()
            .map(|p| json!({ "name": p.name, "category": p.category }))
            .collect();
        json!({ "backend": "GameController", "controllers": pads })
    }
}
