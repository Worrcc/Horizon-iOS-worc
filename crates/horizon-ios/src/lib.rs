//! The UIKit calls of the iOS app that the game crate, which forbids unsafe code, can't make
//! itself. Everything here does nothing on other systems.

pub mod controllers;

/// Shows `message` in an alert whose only button ends the app: iOS has no terminal and no
/// launcher to show why the game stopped. Call it on the main thread, then [`poll_alert`] from
/// the event loop until the app ends: UIKit only presents over a view that is on screen, which
/// the game's window may not be yet (an error right at launch), so the alert gets a window of
/// its own and is presented again until UIKit shows it.
pub fn fatal_alert(title: &str, message: &str) {
    #[cfg(target_os = "ios")]
    ios::fatal_alert(title, message);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (title, message);
        std::process::exit(1);
    }
}

/// Presents the alert of [`fatal_alert`] if UIKit hasn't shown it yet. Cheap: call it on every
/// event loop iteration.
pub fn poll_alert() {
    #[cfg(target_os = "ios")]
    ios::poll_alert();
}

#[cfg(target_os = "ios")]
mod ios {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_foundation::{MainThreadMarker, NSString};
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle, UIScreen,
        UIViewController, UIWindow, UIWindowLevelAlert,
    };
    use std::cell::RefCell;
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    /// How long a refused presentation waits before the next try.
    const RETRY: Duration = Duration::from_millis(300);

    struct Pending {
        title: String,
        message: String,
        window: Option<Retained<UIWindow>>,
        next_try: Instant,
        shown: bool,
    }

    thread_local! {
        static PENDING: RefCell<Option<Pending>> = const { RefCell::new(None) };
    }

    pub(super) fn fatal_alert(title: &str, message: &str) {
        PENDING.with_borrow_mut(|pending| {
            // The first error is the one to show.
            if pending.is_none() {
                *pending = Some(Pending {
                    title: title.to_owned(),
                    message: message.to_owned(),
                    window: None,
                    next_try: Instant::now(),
                    shown: false,
                });
            }
        });
        poll_alert();
    }

    pub(super) fn poll_alert() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        PENDING.with_borrow_mut(|pending| {
            let Some(pending) = pending else {
                return;
            };
            if pending.shown {
                return;
            }
            let window = pending.window.get_or_insert_with(|| alert_window(mtm));
            let Some(root) = window.rootViewController() else {
                return;
            };
            // Safety: UIKit objects used on the main thread (`mtm`).
            unsafe {
                // Set as soon as UIKit accepts the presentation, animation or not.
                if root.presentedViewController().is_some() {
                    pending.shown = true;
                    return;
                }
                let now = Instant::now();
                if now < pending.next_try {
                    return;
                }
                pending.next_try = now + RETRY;
                let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
                    Some(&NSString::from_str(&pending.title)),
                    Some(&NSString::from_str(&pending.message)),
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
                // Refused (with only a console warning) while the window isn't on screen yet:
                // the next poll tries again.
                root.presentViewController_animated_completion(&alert, false, None);
            }
        });
    }

    /// A window above the game's, holding nothing but the controller the alert is presented on.
    fn alert_window(mtm: MainThreadMarker) -> Retained<UIWindow> {
        // The app has no scenes (winit makes none), so the main screen is the screen.
        #[allow(deprecated)]
        let frame = UIScreen::mainScreen(mtm).bounds();
        // Safety: made and shown on the main thread (`mtm`).
        unsafe {
            let window = UIWindow::initWithFrame(mtm.alloc(), frame);
            let root = UIViewController::init(mtm.alloc());
            window.setRootViewController(Some(&root));
            window.setWindowLevel(UIWindowLevelAlert);
            window.makeKeyAndVisible();
            window
        }
    }
}
