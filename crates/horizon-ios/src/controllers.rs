//! Controllers through Apple's GameController framework: every connected pad with the extended
//! gamepad profile (Xbox, PlayStation, Switch Pro, MFi), read once a frame. Nothing elsewhere.

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

#[cfg(target_os = "ios")]
mod ios {
    use super::Gamepad;
    use objc2_game_controller::{GCController, GCControllerButtonInput, GCDevice};

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
