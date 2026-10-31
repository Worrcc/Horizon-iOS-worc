//! The real controllers on iOS: every pad Apple's GameController framework connects (Xbox,
//! PlayStation, Switch Pro and MFi pads), merged, as gilrs' active pad is elsewhere. Vibration
//! plays through Core Haptics on every pad that has it; no DualSense lights or triggers yet.
use super::{super::Game, Pads};
use horizon_client::gamepad::{Button, GlyphStyle, PadState, Pulse, Rumble};
use serde_json::{Value, json};

/// The pads at the last read, for `pad status`.
#[derive(Default)]
pub(super) struct Devices {
    pads: Vec<horizon_ios::controllers::Gamepad>,
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
    pub(super) fn pad_output_off(&mut self) {}
    pub(super) fn pad_output_frame(&mut self) {}
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
