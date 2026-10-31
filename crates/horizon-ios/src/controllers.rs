//! Controllers through Apple's GameController framework: every connected pad with the extended
//! gamepad profile (Xbox, PlayStation, Switch Pro, MFi), read once a frame, and their vibration
//! through Core Haptics (`GCController.haptics`). Nothing elsewhere.

/// One pad's state. Sticks are -1..1 with up positive; triggers 0..1.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Gamepad {
    /// `productCategory`: "DualSense", "Xbox One", "Switch Pro Controller", "MFi"...
    pub category: String,
    pub name: String,
    /// A, B, X, Y: south, east, west, north.
    pub face: [bool; 4],
    pub shoulders: [bool; 2],
    pub triggers: [f32; 2],
    /// Menu (start), options (select).
    pub menu: bool,
    pub options: bool,
    pub thumbs: [bool; 2],
    /// Up, down, left, right.
    pub dpad: [bool; 4],
    pub left: [f32; 2],
    pub right: [f32; 2],
}

/// The connected pads.
pub fn gamepads() -> Vec<Gamepad> {
    #[cfg(target_os = "ios")]
    return ios::gamepads();
    #[cfg(not(target_os = "ios"))]
    Vec::new()
}

/// An adaptive trigger's mode (`GCDualSenseAdaptiveTrigger`). Positions along the travel and
/// strengths are 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trigger {
    Off,
    /// Even resistance from `start` to the end.
    Feedback {
        start: f32,
        strength: f32,
    },
    /// Resistance from `start` that gives way with a click at `end`.
    Weapon {
        start: f32,
        end: f32,
        strength: f32,
    },
    /// A buzz from `start` on.
    Vibration {
        start: f32,
        amplitude: f32,
        frequency: f32,
    },
}

/// Sets the lightbar (pads with a light: DualSense, DualShock 4) and the DualSense's adaptive
/// triggers, left and right, on every connected pad. A `None` lightbar leaves it as it is.
pub fn show(lightbar: Option<[u8; 3]>, triggers: [Trigger; 2]) {
    #[cfg(target_os = "ios")]
    ios::show(lightbar, triggers);
    #[cfg(not(target_os = "ios"))]
    let _ = (lightbar, triggers);
}

/// Plays a vibration on every connected pad that has haptics: each step's strong and weak motor
/// levels (0..1) for its milliseconds, one after another. `false` when no pad could play it.
pub fn vibrate(steps: &[(f32, f32, u32)]) -> bool {
    #[cfg(target_os = "ios")]
    return ios::vibrate(steps);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = steps;
        false
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use super::{Gamepad, Trigger};
    use objc2::rc::{Allocated, Retained};
    use objc2::runtime::{AnyObject, NSObjectProtocol};
    use objc2::{ClassType, class, msg_send, msg_send_id};
    use objc2_foundation::{NSArray, NSError, NSString};
    use objc2_game_controller::{
        GCColor, GCController, GCControllerButtonInput, GCDevice, GCDualSenseAdaptiveTrigger,
        GCDualSenseGamepad, GCHapticsLocalityDefault,
    };
    use std::cell::RefCell;

    #[link(name = "CoreHaptics", kind = "framework")]
    unsafe extern "C" {
        static CHHapticEventTypeHapticContinuous: &'static NSString;
        static CHHapticEventParameterIDHapticIntensity: &'static NSString;
        static CHHapticEventParameterIDHapticSharpness: &'static NSString;
    }

    thread_local! {
        /// Each pad's started `CHHapticEngine`, made the first time it vibrates.
        static ENGINES: RefCell<Vec<(Retained<GCController>, Retained<AnyObject>)>> =
            const { RefCell::new(Vec::new()) };
    }

    /// The pad's haptic engine, started; `None` for a pad without haptics.
    unsafe fn engine(controller: &GCController) -> Option<Retained<AnyObject>> {
        ENGINES.with_borrow_mut(|engines| {
            if let Some((_, engine)) = engines.iter().find(|(c, _)| **c == *controller) {
                return Some(engine.clone());
            }
            unsafe {
                let haptics = controller.haptics()?;
                let engine: Option<Retained<AnyObject>> =
                    msg_send_id![&haptics, createEngineWithLocality: GCHapticsLocalityDefault];
                let engine = engine?;
                let started: Result<(), Retained<NSError>> =
                    msg_send![&engine, startAndReturnError: _];
                started.ok()?;
                engines.push((controller.retain(), engine.clone()));
                Some(engine)
            }
        })
    }

    /// One `CHHapticEvent` parameter.
    unsafe fn parameter(id: &NSString, value: f32) -> Option<Retained<AnyObject>> {
        unsafe {
            let p: Allocated<AnyObject> = msg_send_id![class!(CHHapticEventParameter), alloc];
            msg_send_id![p, initWithParameterID: id, value: value]
        }
    }

    /// The steps as a `CHHapticPattern` of continuous events: the stronger motor sets the
    /// intensity, the weak (high-frequency) motor's share the sharpness.
    unsafe fn pattern(steps: &[(f32, f32, u32)]) -> Option<Retained<AnyObject>> {
        let mut events = Vec::new();
        let mut time = 0.0f64;
        for &(strong, weak, millis) in steps {
            let duration = f64::from(millis) / 1000.0;
            let at = time;
            time += duration;
            let intensity = strong.max(weak).clamp(0.0, 1.0);
            if intensity <= 0.0 || duration <= 0.0 {
                continue;
            }
            let sharpness = (weak / (strong + weak)).clamp(0.0, 1.0);
            unsafe {
                let parameters = NSArray::from_vec(vec![
                    parameter(CHHapticEventParameterIDHapticIntensity, intensity)?,
                    parameter(CHHapticEventParameterIDHapticSharpness, sharpness)?,
                ]);
                let e: Allocated<AnyObject> = msg_send_id![class!(CHHapticEvent), alloc];
                let event: Option<Retained<AnyObject>> = msg_send_id![
                    e,
                    initWithEventType: CHHapticEventTypeHapticContinuous,
                    parameters: &*parameters,
                    relativeTime: at,
                    duration: duration
                ];
                events.push(event?);
            }
        }
        if events.is_empty() {
            return None;
        }
        unsafe {
            let events = NSArray::from_vec(events);
            let none: Retained<NSArray<AnyObject>> = NSArray::new();
            let p: Allocated<AnyObject> = msg_send_id![class!(CHHapticPattern), alloc];
            let pattern: Result<Retained<AnyObject>, Retained<NSError>> =
                msg_send_id![p, initWithEvents: &*events, parameters: &*none, error: _];
            pattern.ok()
        }
    }

    fn set_trigger(trigger: &GCDualSenseAdaptiveTrigger, mode: Trigger) {
        // Safety: on the main thread, as the game runs.
        unsafe {
            match mode {
                Trigger::Off => trigger.setModeOff(),
                Trigger::Feedback { start, strength } => {
                    trigger.setModeFeedbackWithStartPosition_resistiveStrength(start, strength)
                }
                Trigger::Weapon {
                    start,
                    end,
                    strength,
                } => trigger.setModeWeaponWithStartPosition_endPosition_resistiveStrength(
                    start, end, strength,
                ),
                Trigger::Vibration {
                    start,
                    amplitude,
                    frequency,
                } => trigger.setModeVibrationWithStartPosition_amplitude_frequency(
                    start, amplitude, frequency,
                ),
            }
        }
    }

    pub(super) fn show(lightbar: Option<[u8; 3]>, triggers: [Trigger; 2]) {
        // Safety: GameController's objects are used on the main thread, as the game runs.
        unsafe {
            for controller in GCController::controllers().iter() {
                if let (Some([r, g, b]), Some(light)) = (lightbar, controller.light()) {
                    let level = |v: u8| f32::from(v) / 255.0;
                    let colour = GCColor::initWithRed_green_blue(
                        GCColor::alloc(),
                        level(r),
                        level(g),
                        level(b),
                    );
                    light.setColor(&colour);
                }
                let Some(pad) = controller.extendedGamepad() else {
                    continue;
                };
                if !pad.is_kind_of::<GCDualSenseGamepad>() {
                    continue;
                }
                let pad: Retained<GCDualSenseGamepad> = Retained::cast(pad);
                set_trigger(&pad.leftTrigger(), triggers[0]);
                set_trigger(&pad.rightTrigger(), triggers[1]);
            }
        }
    }

    pub(super) fn vibrate(steps: &[(f32, f32, u32)]) -> bool {
        // Safety: GameController and Core Haptics are used on the main thread, as the game runs.
        unsafe {
            let Some(pattern) = pattern(steps) else {
                return false;
            };
            let mut played = false;
            for controller in GCController::controllers().iter() {
                let Some(engine) = engine(controller) else {
                    continue;
                };
                let player: Result<Retained<AnyObject>, Retained<NSError>> =
                    msg_send_id![&engine, createPlayerWithPattern: &*pattern, error: _];
                let started = player.and_then(|player| {
                    let r: Result<(), Retained<NSError>> =
                        msg_send![&player, startAtTime: 0.0f64, error: _];
                    r
                });
                if started.is_ok() {
                    played = true;
                } else {
                    // Stopped (the app went to the background): a new engine next time.
                    ENGINES.with_borrow_mut(|e| e.retain(|(c, _)| **c != *controller));
                }
            }
            played
        }
    }

    pub(super) fn gamepads() -> Vec<Gamepad> {
        let pressed = |b: &GCControllerButtonInput| unsafe { b.isPressed() };
        // Safety: GameController's objects are read on the main thread, as the game polls.
        unsafe {
            GCController::controllers()
                .iter()
                .filter_map(|controller| {
                    let pad = controller.extendedGamepad()?;
                    let dpad = pad.dpad();
                    let (left, right) = (pad.leftThumbstick(), pad.rightThumbstick());
                    Some(Gamepad {
                        category: controller.productCategory().to_string(),
                        name: controller
                            .vendorName()
                            .map_or_else(String::new, |n| n.to_string()),
                        face: [
                            pressed(&pad.buttonA()),
                            pressed(&pad.buttonB()),
                            pressed(&pad.buttonX()),
                            pressed(&pad.buttonY()),
                        ],
                        shoulders: [pressed(&pad.leftShoulder()), pressed(&pad.rightShoulder())],
                        triggers: [pad.leftTrigger().value(), pad.rightTrigger().value()],
                        menu: pressed(&pad.buttonMenu()),
                        options: pad.buttonOptions().is_some_and(|b| pressed(&b)),
                        thumbs: [
                            pad.leftThumbstickButton().is_some_and(|b| pressed(&b)),
                            pad.rightThumbstickButton().is_some_and(|b| pressed(&b)),
                        ],
                        dpad: [
                            pressed(&dpad.up()),
                            pressed(&dpad.down()),
                            pressed(&dpad.left()),
                            pressed(&dpad.right()),
                        ],
                        left: [left.xAxis().value(), left.yAxis().value()],
                        right: [right.xAxis().value(), right.yAxis().value()],
                    })
                })
                .collect()
        }
    }
}
