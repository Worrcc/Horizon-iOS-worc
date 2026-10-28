//! The UIKit calls of the iOS app that the game crate, which forbids unsafe code, can't make
//! itself. Everything here does nothing on other systems.

pub mod controllers;

/// Shows `message` in an alert whose only button ends the app: iOS has no terminal and no
/// launcher to show why the game stopped. Returns at once; call it on the main thread, with
/// the event loop still running so the alert is drawn.
pub fn fatal_alert(title: &str, message: &str) {
    #[cfg(target_os = "ios")]
    ios::fatal_alert(title, message);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (title, message);
        std::process::exit(1);
    }
}

#[cfg(target_os = "ios")]
mod ios {
    use block2::RcBlock;
    use objc2_foundation::{MainThreadMarker, NSString};
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle, UIApplication,
    };
    use std::ptr::NonNull;

    pub(super) fn fatal_alert(title: &str, message: &str) {
        let Some(mtm) = MainThreadMarker::new() else {
            std::process::exit(1);
        };
        let app = UIApplication::sharedApplication(mtm);
        // winit's one window holds the game's view controller.
        #[allow(deprecated)]
        let root = app
            .windows()
            .iter()
            .find_map(|window| window.rootViewController());
        let Some(root) = root else {
            std::process::exit(1);
        };
        // Safety: UIKit objects made and presented on the main thread (`mtm`).
        unsafe {
            let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
                Some(&NSString::from_str(title)),
                Some(&NSString::from_str(message)),
                UIAlertControllerStyle::Alert,
                mtm,
            );
            let close = RcBlock::new(|_: NonNull<UIAlertAction>| std::process::exit(1));
            let action = UIAlertAction::actionWithTitle_style_handler(
                Some(&NSString::from_str("Close Horizon")),
                UIAlertActionStyle::Default,
                Some(&close),
                mtm,
            );
            alert.addAction(&action);
            root.presentViewController_animated_completion(&alert, true, None);
        }
    }
}
