//! The client's input layer, below gameplay and screens: key bindings and their
//! names, toggle keys, mouse settings, controllers, the clipboard and window
//! options. horizon-client re-exports each module under its old path
//! (`horizon_client::keybinds`, ...).
// Unsafe only for the iOS clipboard (UIPasteboard through objc2).
#![cfg_attr(not(target_os = "ios"), forbid(unsafe_code))]
#![cfg_attr(target_os = "ios", deny(unsafe_code))]
pub mod clipboard;
pub mod gamepad;
pub mod input;
pub mod interaction_input;
pub mod key_names;
pub mod keybinds;
pub mod keymapping;
pub mod mouse;
pub mod window;
