//! The UIKit calls of the iOS app that the game crate, which forbids unsafe code, can't make
//! itself. Everything here does nothing on other systems.
//!
//! Alerts: UIKit only presents over a view that is on screen, which the game's window may not be
//! yet (an error right at launch), so alerts get a window of their own above the game's and are
//! presented again until UIKit shows them. Call [`poll_alert`] on every event loop iteration.

pub mod controllers;
pub mod keychain;

/// An alert shown with [`show_alert`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlertId(u64);

/// Shows an alert with `buttons` (the first is the preferred one); [`alert_answer`] says which
/// was tapped. Replaces the alert shown before, unless that one is fatal.
pub fn show_alert(title: &str, message: &str, buttons: &[&str]) -> AlertId {
    #[cfg(target_os = "ios")]
    return ios::show(title, message, buttons, false);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (title, message, buttons);
        AlertId(0)
    }
}

/// The button the player tapped in `alert` (its index in `buttons`), once.
pub fn alert_answer(alert: AlertId) -> Option<usize> {
    #[cfg(target_os = "ios")]
    return ios::answer(alert);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = alert;
        None
    }
}

/// Takes `alert` off the screen if it is still up.
pub fn dismiss_alert(alert: AlertId) {
    #[cfg(target_os = "ios")]
    ios::dismiss(alert);
    #[cfg(not(target_os = "ios"))]
    let _ = alert;
}

/// Shows `message` in an alert whose only button ends the app: iOS has no terminal and no
/// launcher to show why the game stopped. Nothing replaces it.
pub fn fatal_alert(title: &str, message: &str) {
    #[cfg(target_os = "ios")]
    ios::show(title, message, &["Close Horizon"], true);
    #[cfg(not(target_os = "ios"))]
    {
        let _ = (title, message);
        std::process::exit(1);
    }
}

/// Presents the current alert if UIKit hasn't shown it yet, and puts the game's window back in
/// front once it is answered. Cheap: call it on every event loop iteration.
pub fn poll_alert() {
    #[cfg(target_os = "ios")]
    ios::poll();
}

/// Opens `url` in the system's browser.
pub fn open_url(url: &str) {
    #[cfg(target_os = "ios")]
    ios::open_url(url);
    #[cfg(not(target_os = "ios"))]
    let _ = url;
}

#[cfg(target_os = "ios")]
mod ios {
    use super::AlertId;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2_foundation::{MainThreadMarker, NSDictionary, NSString, NSURL};
    use objc2_ui_kit::{
        UIAlertAction, UIAlertActionStyle, UIAlertController, UIAlertControllerStyle,
        UIApplication, UIScreen, UIViewController, UIWindow, UIWindowLevelAlert,
    };
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    use std::ptr::NonNull;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    /// How long a refused presentation waits before the next try.
    const RETRY: Duration = Duration::from_millis(300);

    struct Current {
        id: u64,
        title: String,
        message: String,
        buttons: Vec<String>,
        fatal: bool,
        /// Set by the button's handler.
        tapped: Rc<Cell<Option<usize>>>,
        next_try: Instant,
    }

    thread_local! {
        static CURRENT: RefCell<Option<Current>> = const { RefCell::new(None) };
        static ANSWERS: RefCell<HashMap<u64, usize>> = RefCell::new(HashMap::new());
        static WINDOW: RefCell<Option<Retained<UIWindow>>> = const { RefCell::new(None) };
        static NEXT_ID: Cell<u64> = const { Cell::new(1) };
    }

    pub(super) fn show(title: &str, message: &str, buttons: &[&str], fatal: bool) -> AlertId {
        let id = NEXT_ID.get();
        NEXT_ID.set(id + 1);
        let replaced = CURRENT.with_borrow(|c| c.as_ref().map(|c| (c.id, c.fatal)));
        match replaced {
            // A fatal alert stays: the app is ending.
            Some((_, true)) => return AlertId(id),
            Some((old, false)) => dismiss(AlertId(old)),
            None => {}
        }
        CURRENT.set(Some(Current {
            id,
            title: title.to_owned(),
            message: message.to_owned(),
            buttons: buttons.iter().map(|b| (*b).to_owned()).collect(),
            fatal,
            tapped: Rc::new(Cell::new(None)),
            next_try: Instant::now(),
        }));
        poll();
        AlertId(id)
    }

    pub(super) fn answer(alert: AlertId) -> Option<usize> {
        poll();
        ANSWERS.with_borrow_mut(|answers| answers.remove(&alert.0))
    }

    pub(super) fn dismiss(alert: AlertId) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let ours = CURRENT.with_borrow(|c| c.as_ref().is_some_and(|c| c.id == alert.0 && !c.fatal));
        if !ours {
            return;
        }
        CURRENT.set(None);
        if let Some(root) = root(mtm) {
            // Safety: UIKit objects used on the main thread (`mtm`).
            unsafe { root.dismissViewControllerAnimated_completion(false, None) };
        }
        hide_window(mtm);
    }

    pub(super) fn poll() {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let answered =
            CURRENT.with_borrow(|c| c.as_ref().and_then(|c| Some((c.id, c.tapped.get()?))));
        if let Some((id, index)) = answered {
            ANSWERS.with_borrow_mut(|answers| answers.insert(id, index));
            CURRENT.set(None);
            hide_window(mtm);
            return;
        }
        CURRENT.with_borrow_mut(|current| {
            let Some(current) = current else {
                return;
            };
            let window = window(mtm);
            let Some(root) = window.rootViewController() else {
                return;
            };
            // Safety: UIKit objects used on the main thread (`mtm`).
            unsafe {
                // Set as soon as UIKit accepts the presentation, animation or not.
                if root.presentedViewController().is_some() {
                    return;
                }
                let now = Instant::now();
                if now < current.next_try {
                    return;
                }
                current.next_try = now + RETRY;
                if window.isHidden() {
                    window.makeKeyAndVisible();
                }
                let alert = UIAlertController::alertControllerWithTitle_message_preferredStyle(
                    Some(&NSString::from_str(&current.title)),
                    Some(&NSString::from_str(&current.message)),
                    UIAlertControllerStyle::Alert,
                    mtm,
                );
                for (i, title) in current.buttons.iter().enumerate() {
                    let tapped = current.tapped.clone();
                    let fatal = current.fatal;
                    let handler = RcBlock::new(move |_: NonNull<UIAlertAction>| {
                        if fatal {
                            std::process::exit(1);
                        }
                        tapped.set(Some(i));
                    });
                    let action = UIAlertAction::actionWithTitle_style_handler(
                        Some(&NSString::from_str(title)),
                        UIAlertActionStyle::Default,
                        Some(&handler),
                        mtm,
                    );
                    alert.addAction(&action);
                    if i == 0 {
                        alert.setPreferredAction(Some(&action));
                    }
                }
                // Refused (with only a console warning) while the window isn't on screen yet:
                // the next poll tries again.
                root.presentViewController_animated_completion(&alert, false, None);
            }
        });
    }

    /// The alerts' window, made the first time.
    fn window(mtm: MainThreadMarker) -> Retained<UIWindow> {
        WINDOW.with_borrow_mut(|w| w.get_or_insert_with(|| alert_window(mtm)).clone())
    }

    fn root(mtm: MainThreadMarker) -> Option<Retained<UIViewController>> {
        window(mtm).rootViewController()
    }

    /// A window above the game's, holding nothing but the controller alerts are presented on.
    fn alert_window(mtm: MainThreadMarker) -> Retained<UIWindow> {
        // winit makes no scenes, so the main screen is the screen.
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

    /// Takes the alerts' window away, so touches and keys reach the game's window again.
    fn hide_window(mtm: MainThreadMarker) {
        let Some(ours) = WINDOW.with_borrow(Clone::clone) else {
            return;
        };
        ours.setHidden(true);
        #[allow(deprecated)]
        let windows = UIApplication::sharedApplication(mtm).windows();
        let game = windows
            .iter()
            .find(|w| !std::ptr::eq(&**w, &*ours) && !w.isHidden());
        if let Some(game) = game {
            // Safety: a window of this app, on the main thread.
            unsafe { game.makeKeyWindow() };
        }
    }

    pub(super) fn open_url(url: &str) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        // Safety: UIKit and Foundation objects used on the main thread (`mtm`).
        unsafe {
            let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) else {
                return;
            };
            UIApplication::sharedApplication(mtm).openURL_options_completionHandler(
                &url,
                &NSDictionary::new(),
                None,
            );
        }
    }
}
