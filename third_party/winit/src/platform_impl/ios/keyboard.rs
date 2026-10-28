//! Horizon's patch (see `ios/README.md` in the Horizon tree): keyboards on iOS.
//!
//! - Hardware keyboards (iPad Magic Keyboard, Bluetooth): `pressesBegan`/`pressesEnded` on the
//!   view controller, with the key's USB HID usage turned into a [`KeyCode`], as the other
//!   backends report physical keys.
//! - The on-screen keyboard: `insertText:` gives characters only; each gets the [`KeyCode`] of
//!   the US layout key that types it, so code that reads physical keys still sees typing.
use objc2::rc::Retained;
use objc2_foundation::{MainThreadMarker, NSSet};
use objc2_ui_kit::{UIKey, UIKeyModifierFlags, UIPress};
use smol_str::SmolStr;

use super::app_state::{self, EventWrapper};
use super::window::WinitUIWindow;
use crate::event::{ElementState, Event, KeyEvent, Modifiers, WindowEvent};
use crate::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey};
use crate::platform_impl::platform::DEVICE_ID;
use crate::platform_impl::KeyEventExtra;
use crate::window::WindowId as RootWindowId;

/// The key of the US layout that types `c`, for the on-screen keyboard's characters. Other
/// characters get F35, which no game binds, so text fields still receive their text.
pub(crate) fn code_for_char(c: char) -> PhysicalKey {
    let code = match c.to_ascii_lowercase() {
        'a' => KeyCode::KeyA,
        'b' => KeyCode::KeyB,
        'c' => KeyCode::KeyC,
        'd' => KeyCode::KeyD,
        'e' => KeyCode::KeyE,
        'f' => KeyCode::KeyF,
        'g' => KeyCode::KeyG,
        'h' => KeyCode::KeyH,
        'i' => KeyCode::KeyI,
        'j' => KeyCode::KeyJ,
        'k' => KeyCode::KeyK,
        'l' => KeyCode::KeyL,
        'm' => KeyCode::KeyM,
        'n' => KeyCode::KeyN,
        'o' => KeyCode::KeyO,
        'p' => KeyCode::KeyP,
        'q' => KeyCode::KeyQ,
        'r' => KeyCode::KeyR,
        's' => KeyCode::KeyS,
        't' => KeyCode::KeyT,
        'u' => KeyCode::KeyU,
        'v' => KeyCode::KeyV,
        'w' => KeyCode::KeyW,
        'x' => KeyCode::KeyX,
        'y' => KeyCode::KeyY,
        'z' => KeyCode::KeyZ,
        '1' | '!' => KeyCode::Digit1,
        '2' | '@' => KeyCode::Digit2,
        '3' | '#' => KeyCode::Digit3,
        '4' | '$' => KeyCode::Digit4,
        '5' | '%' => KeyCode::Digit5,
        '6' | '^' => KeyCode::Digit6,
        '7' | '&' => KeyCode::Digit7,
        '8' | '*' => KeyCode::Digit8,
        '9' | '(' => KeyCode::Digit9,
        '0' | ')' => KeyCode::Digit0,
        ' ' => KeyCode::Space,
        '\n' | '\r' => KeyCode::Enter,
        '\t' => KeyCode::Tab,
        '-' | '_' => KeyCode::Minus,
        '=' | '+' => KeyCode::Equal,
        '[' | '{' => KeyCode::BracketLeft,
        ']' | '}' => KeyCode::BracketRight,
        '\\' | '|' => KeyCode::Backslash,
        ';' | ':' => KeyCode::Semicolon,
        '\'' | '"' => KeyCode::Quote,
        '`' | '~' => KeyCode::Backquote,
        ',' | '<' => KeyCode::Comma,
        '.' | '>' => KeyCode::Period,
        '/' | '?' => KeyCode::Slash,
        _ => KeyCode::F35,
    };
    PhysicalKey::Code(code)
}

/// The logical key of the on-screen keyboard's `c`.
pub(crate) fn logical_for_char(c: char, text: &SmolStr) -> Key {
    match c {
        '\n' | '\r' => Key::Named(NamedKey::Enter),
        '\t' => Key::Named(NamedKey::Tab),
        ' ' => Key::Named(NamedKey::Space),
        _ => Key::Character(text.clone()),
    }
}

/// A USB HID usage (page 7, keyboard) as a [`KeyCode`].
fn code_for_usage(usage: isize) -> PhysicalKey {
    let code = match usage {
        0x04 => KeyCode::KeyA,
        0x05 => KeyCode::KeyB,
        0x06 => KeyCode::KeyC,
        0x07 => KeyCode::KeyD,
        0x08 => KeyCode::KeyE,
        0x09 => KeyCode::KeyF,
        0x0A => KeyCode::KeyG,
        0x0B => KeyCode::KeyH,
        0x0C => KeyCode::KeyI,
        0x0D => KeyCode::KeyJ,
        0x0E => KeyCode::KeyK,
        0x0F => KeyCode::KeyL,
        0x10 => KeyCode::KeyM,
        0x11 => KeyCode::KeyN,
        0x12 => KeyCode::KeyO,
        0x13 => KeyCode::KeyP,
        0x14 => KeyCode::KeyQ,
        0x15 => KeyCode::KeyR,
        0x16 => KeyCode::KeyS,
        0x17 => KeyCode::KeyT,
        0x18 => KeyCode::KeyU,
        0x19 => KeyCode::KeyV,
        0x1A => KeyCode::KeyW,
        0x1B => KeyCode::KeyX,
        0x1C => KeyCode::KeyY,
        0x1D => KeyCode::KeyZ,
        0x1E => KeyCode::Digit1,
        0x1F => KeyCode::Digit2,
        0x20 => KeyCode::Digit3,
        0x21 => KeyCode::Digit4,
        0x22 => KeyCode::Digit5,
        0x23 => KeyCode::Digit6,
        0x24 => KeyCode::Digit7,
        0x25 => KeyCode::Digit8,
        0x26 => KeyCode::Digit9,
        0x27 => KeyCode::Digit0,
        0x28 => KeyCode::Enter,
        0x29 => KeyCode::Escape,
        0x2A => KeyCode::Backspace,
        0x2B => KeyCode::Tab,
        0x2C => KeyCode::Space,
        0x2D => KeyCode::Minus,
        0x2E => KeyCode::Equal,
        0x2F => KeyCode::BracketLeft,
        0x30 => KeyCode::BracketRight,
        0x31 | 0x32 => KeyCode::Backslash,
        0x33 => KeyCode::Semicolon,
        0x34 => KeyCode::Quote,
        0x35 => KeyCode::Backquote,
        0x36 => KeyCode::Comma,
        0x37 => KeyCode::Period,
        0x38 => KeyCode::Slash,
        0x39 => KeyCode::CapsLock,
        0x3A => KeyCode::F1,
        0x3B => KeyCode::F2,
        0x3C => KeyCode::F3,
        0x3D => KeyCode::F4,
        0x3E => KeyCode::F5,
        0x3F => KeyCode::F6,
        0x40 => KeyCode::F7,
        0x41 => KeyCode::F8,
        0x42 => KeyCode::F9,
        0x43 => KeyCode::F10,
        0x44 => KeyCode::F11,
        0x45 => KeyCode::F12,
        0x46 => KeyCode::PrintScreen,
        0x47 => KeyCode::ScrollLock,
        0x48 => KeyCode::Pause,
        0x49 => KeyCode::Insert,
        0x4A => KeyCode::Home,
        0x4B => KeyCode::PageUp,
        0x4C => KeyCode::Delete,
        0x4D => KeyCode::End,
        0x4E => KeyCode::PageDown,
        0x4F => KeyCode::ArrowRight,
        0x50 => KeyCode::ArrowLeft,
        0x51 => KeyCode::ArrowDown,
        0x52 => KeyCode::ArrowUp,
        0x53 => KeyCode::NumLock,
        0x54 => KeyCode::NumpadDivide,
        0x55 => KeyCode::NumpadMultiply,
        0x56 => KeyCode::NumpadSubtract,
        0x57 => KeyCode::NumpadAdd,
        0x58 => KeyCode::NumpadEnter,
        0x59 => KeyCode::Numpad1,
        0x5A => KeyCode::Numpad2,
        0x5B => KeyCode::Numpad3,
        0x5C => KeyCode::Numpad4,
        0x5D => KeyCode::Numpad5,
        0x5E => KeyCode::Numpad6,
        0x5F => KeyCode::Numpad7,
        0x60 => KeyCode::Numpad8,
        0x61 => KeyCode::Numpad9,
        0x62 => KeyCode::Numpad0,
        0x63 => KeyCode::NumpadDecimal,
        0x64 => KeyCode::IntlBackslash,
        0x65 => KeyCode::ContextMenu,
        0x67 => KeyCode::NumpadEqual,
        0x68 => KeyCode::F13,
        0x69 => KeyCode::F14,
        0x6A => KeyCode::F15,
        0x6B => KeyCode::F16,
        0x6C => KeyCode::F17,
        0x6D => KeyCode::F18,
        0x6E => KeyCode::F19,
        0x6F => KeyCode::F20,
        0x70 => KeyCode::F21,
        0x71 => KeyCode::F22,
        0x72 => KeyCode::F23,
        0x73 => KeyCode::F24,
        0x87 => KeyCode::IntlRo,
        0x89 => KeyCode::IntlYen,
        0x90 => KeyCode::Lang1,
        0x91 => KeyCode::Lang2,
        0xE0 => KeyCode::ControlLeft,
        0xE1 => KeyCode::ShiftLeft,
        0xE2 => KeyCode::AltLeft,
        0xE3 => KeyCode::SuperLeft,
        0xE4 => KeyCode::ControlRight,
        0xE5 => KeyCode::ShiftRight,
        0xE6 => KeyCode::AltRight,
        0xE7 => KeyCode::SuperRight,
        _ => return PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
    };
    PhysicalKey::Code(code)
}

fn named(code: KeyCode) -> Option<NamedKey> {
    Some(match code {
        KeyCode::Enter | KeyCode::NumpadEnter => NamedKey::Enter,
        KeyCode::Escape => NamedKey::Escape,
        KeyCode::Backspace => NamedKey::Backspace,
        KeyCode::Tab => NamedKey::Tab,
        KeyCode::Space => NamedKey::Space,
        KeyCode::CapsLock => NamedKey::CapsLock,
        KeyCode::F1 => NamedKey::F1,
        KeyCode::F2 => NamedKey::F2,
        KeyCode::F3 => NamedKey::F3,
        KeyCode::F4 => NamedKey::F4,
        KeyCode::F5 => NamedKey::F5,
        KeyCode::F6 => NamedKey::F6,
        KeyCode::F7 => NamedKey::F7,
        KeyCode::F8 => NamedKey::F8,
        KeyCode::F9 => NamedKey::F9,
        KeyCode::F10 => NamedKey::F10,
        KeyCode::F11 => NamedKey::F11,
        KeyCode::F12 => NamedKey::F12,
        KeyCode::PrintScreen => NamedKey::PrintScreen,
        KeyCode::ScrollLock => NamedKey::ScrollLock,
        KeyCode::Pause => NamedKey::Pause,
        KeyCode::Insert => NamedKey::Insert,
        KeyCode::Home => NamedKey::Home,
        KeyCode::PageUp => NamedKey::PageUp,
        KeyCode::Delete => NamedKey::Delete,
        KeyCode::End => NamedKey::End,
        KeyCode::PageDown => NamedKey::PageDown,
        KeyCode::ArrowRight => NamedKey::ArrowRight,
        KeyCode::ArrowLeft => NamedKey::ArrowLeft,
        KeyCode::ArrowDown => NamedKey::ArrowDown,
        KeyCode::ArrowUp => NamedKey::ArrowUp,
        KeyCode::NumLock => NamedKey::NumLock,
        KeyCode::ContextMenu => NamedKey::ContextMenu,
        KeyCode::ControlLeft | KeyCode::ControlRight => NamedKey::Control,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => NamedKey::Shift,
        KeyCode::AltLeft | KeyCode::AltRight => NamedKey::Alt,
        KeyCode::SuperLeft | KeyCode::SuperRight => NamedKey::Super,
        _ => return None,
    })
}

fn location(code: KeyCode) -> KeyLocation {
    match code {
        KeyCode::ControlLeft | KeyCode::ShiftLeft | KeyCode::AltLeft | KeyCode::SuperLeft => {
            KeyLocation::Left
        },
        KeyCode::ControlRight | KeyCode::ShiftRight | KeyCode::AltRight | KeyCode::SuperRight => {
            KeyLocation::Right
        },
        KeyCode::NumpadEnter
        | KeyCode::NumpadDivide
        | KeyCode::NumpadMultiply
        | KeyCode::NumpadSubtract
        | KeyCode::NumpadAdd
        | KeyCode::NumpadDecimal
        | KeyCode::NumpadEqual
        | KeyCode::Numpad0
        | KeyCode::Numpad1
        | KeyCode::Numpad2
        | KeyCode::Numpad3
        | KeyCode::Numpad4
        | KeyCode::Numpad5
        | KeyCode::Numpad6
        | KeyCode::Numpad7
        | KeyCode::Numpad8
        | KeyCode::Numpad9 => KeyLocation::Numpad,
        _ => KeyLocation::Standard,
    }
}

/// Keys the text system types with `insertText:`/`deleteBackward` while the on-screen
/// keyboard's view is first responder: the presses leave them to it, so nothing is typed twice.
fn types_text(code: PhysicalKey, characters: &str) -> bool {
    match code {
        PhysicalKey::Code(KeyCode::Backspace | KeyCode::Enter | KeyCode::NumpadEnter) => true,
        PhysicalKey::Code(KeyCode::Escape | KeyCode::Tab) => false,
        PhysicalKey::Code(_) => characters.chars().next().is_some_and(|c| !c.is_control()),
        PhysicalKey::Unidentified(_) => true,
    }
}

fn modifiers(flags: UIKeyModifierFlags) -> ModifiersState {
    let mut state = ModifiersState::empty();
    state.set(ModifiersState::SHIFT, flags.contains(UIKeyModifierFlags::UIKeyModifierShift));
    state.set(ModifiersState::CONTROL, flags.contains(UIKeyModifierFlags::UIKeyModifierControl));
    state.set(ModifiersState::ALT, flags.contains(UIKeyModifierFlags::UIKeyModifierAlternate));
    state.set(ModifiersState::SUPER, flags.contains(UIKeyModifierFlags::UIKeyModifierCommand));
    state
}

/// The key events of `presses`. `text_input`: the on-screen keyboard's view is first responder.
/// Returns whether any press was left to the text system.
pub(crate) fn handle_presses(
    mtm: MainThreadMarker,
    window: &WinitUIWindow,
    presses: &NSSet<UIPress>,
    state: ElementState,
    text_input: bool,
) -> bool {
    let window_id = RootWindowId(window.id());
    let mut left_to_text = false;
    let mut events = Vec::new();
    for press in presses.iter() {
        // Safety: a press of this event, on the main thread.
        let Some(key): Option<Retained<UIKey>> = (unsafe { press.key(mtm) }) else {
            left_to_text = true;
            continue;
        };
        let (usage, characters, unmodified, flags) = unsafe {
            (
                key.keyCode().0 as isize,
                key.characters().to_string(),
                key.charactersIgnoringModifiers().to_string(),
                key.modifierFlags(),
            )
        };
        let physical_key = code_for_usage(usage);
        if text_input && types_text(physical_key, &characters) {
            left_to_text = true;
            continue;
        }
        events.push(EventWrapper::StaticEvent(Event::WindowEvent {
            window_id,
            event: WindowEvent::ModifiersChanged(Modifiers::from(modifiers(flags))),
        }));
        let code = match physical_key {
            PhysicalKey::Code(code) => Some(code),
            PhysicalKey::Unidentified(_) => None,
        };
        let printable = characters.chars().next().filter(|c| !c.is_control());
        let logical_key = match code.and_then(named) {
            Some(named) => Key::Named(named),
            None => match unmodified.chars().next().filter(|c| !c.is_control()) {
                Some(_) => Key::Character(SmolStr::new(&unmodified)),
                None => Key::Unidentified(crate::keyboard::NativeKey::Unidentified),
            },
        };
        let text = match (state, code) {
            (ElementState::Pressed, Some(KeyCode::Enter | KeyCode::NumpadEnter)) => {
                Some(SmolStr::new("\r"))
            },
            (ElementState::Pressed, _) => printable.map(|_| SmolStr::new(&characters)),
            _ => None,
        };
        events.push(EventWrapper::StaticEvent(Event::WindowEvent {
            window_id,
            event: WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event: KeyEvent {
                    physical_key,
                    logical_key,
                    text,
                    location: code.map_or(KeyLocation::Standard, location),
                    state,
                    repeat: false,
                    platform_specific: KeyEventExtra {},
                },
                is_synthetic: false,
            },
        }));
        // The modifiers after a modifier key's own press or release.
        if code.and_then(named).is_some_and(|n| {
            matches!(n, NamedKey::Shift | NamedKey::Control | NamedKey::Alt | NamedKey::Super)
        }) {
            let mut after = modifiers(flags);
            let bit = match code {
                Some(KeyCode::ShiftLeft | KeyCode::ShiftRight) => ModifiersState::SHIFT,
                Some(KeyCode::ControlLeft | KeyCode::ControlRight) => ModifiersState::CONTROL,
                Some(KeyCode::AltLeft | KeyCode::AltRight) => ModifiersState::ALT,
                _ => ModifiersState::SUPER,
            };
            after.set(bit, state == ElementState::Pressed);
            events.push(EventWrapper::StaticEvent(Event::WindowEvent {
                window_id,
                event: WindowEvent::ModifiersChanged(Modifiers::from(after)),
            }));
        }
    }
    app_state::handle_nonuser_events(mtm, events);
    left_to_text
}
