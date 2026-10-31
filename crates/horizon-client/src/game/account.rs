//! The Microsoft account on iOS, where no launcher signs the player in (`msa.rs` does the
//! requests). A button on the title screens starts the device-code sign-in: alerts show the
//! code, copy it and open the browser, and say how it ended. The account is kept in the
//! keychain and renewed at the next start, in the background, before anything joins a server.
//! The state lives here, not in `Game` or `Loading`, so a sign-in carries over from the startup
//! menu to the game.
// Only the iOS app signs in itself; elsewhere the launcher does.
#![cfg_attr(not(target_os = "ios"), allow(dead_code))]
use horizon_client::{
    clipboard::{Clipboard, SystemClipboard},
    hud::{HudAtlas, HudBatch},
    msa::{self, DeviceCode, Stored},
    touch_controls::Layout,
};
use horizon_ios::AlertId;
use std::cell::RefCell;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, TryRecvError, channel},
};

/// The keychain item holding the account (`Stored` as JSON).
const KEY: &str = "microsoft-account";

enum Step {
    Idle,
    /// Asking Microsoft for a code.
    Starting(Receiver<Result<DeviceCode, String>>),
    /// The code is up; the player signs in in the browser while a thread polls.
    Waiting {
        code: DeviceCode,
        alert: AlertId,
        cancel: Arc<AtomicBool>,
        done: Receiver<Result<Stored, String>>,
    },
    /// "Signed in as ...", with Sign out.
    SignedIn(AlertId),
    /// A message: nothing more to do once answered.
    Message(AlertId),
}

struct State {
    step: Step,
    /// The signed-in player's name, for the button.
    name: Option<String>,
    /// The renewal started at launch.
    renewing: Option<Receiver<Result<Stored, String>>>,
}

thread_local! {
    static STATE: RefCell<State> = const {
        RefCell::new(State {
            step: Step::Idle,
            name: None,
            renewing: None,
        })
    };
}

/// At launch: the kept account plays at once if its Minecraft token is still good, else it is
/// renewed in the background.
#[cfg(target_os = "ios")]
pub(super) fn start() {
    let Some(stored) = horizon_ios::keychain::get(KEY).and_then(|t| Stored::from_json(&t)) else {
        return;
    };
    STATE.with_borrow_mut(|s| s.name = Some(stored.name.clone()));
    if stored.fresh() {
        use_account(&stored);
        return;
    }
    let Some(client_id) = msa::client_id() else {
        return;
    };
    let (send, receive) = channel();
    let spawned = std::thread::Builder::new()
        .name("Microsoft sign-in".into())
        .spawn(move || {
            let _ = send.send(stored.renew(&client_id));
        });
    if spawned.is_ok() {
        STATE.with_borrow_mut(|s| s.renewing = Some(receive));
    }
}

/// The account plays from now on: online-mode servers see it. A session set earlier in this run
/// stays (`auth::set_current` takes the first).
fn use_account(stored: &Stored) {
    match stored.session() {
        Some(session) => {
            log::info!("Horizon: signed in as {}", stored.name);
            horizon_client::auth::set_current(session);
        }
        None => log::warn!("Horizon: the kept Microsoft account is not usable"),
    }
}

fn keep(stored: &Stored) {
    if let Err(e) = horizon_ios::keychain::set(KEY, &stored.to_json()) {
        log::warn!("Horizon: the Microsoft account is not kept for next time: {e}");
    }
}

/// The button's label.
fn label() -> String {
    STATE.with_borrow(|s| match &s.name {
        Some(name) => name.clone(),
        None => "Sign in".into(),
    })
}

fn button_rect(layout: &Layout, atlas: &HudAtlas) -> [f32; 4] {
    let w = HudBatch::text_width(atlas, &label()) + 10.0;
    [layout.width - w - 4.0, 4.0, w, 16.0]
}

/// Whether `at` (GUI pixels) is on the account button.
pub(super) fn hit(layout: &Layout, atlas: &HudAtlas, at: (f32, f32)) -> bool {
    let [x, y, w, h] = button_rect(layout, atlas);
    at.0 >= x - 2.0 && at.0 < x + w + 2.0 && at.1 >= y - 2.0 && at.1 < y + h + 2.0
}

pub(super) fn draw(batch: &mut HudBatch, atlas: &HudAtlas, layout: &Layout) {
    let busy = STATE.with_borrow(|s| !matches!(s.step, Step::Idle));
    super::touch::draw_button(batch, atlas, button_rect(layout, atlas), &label(), busy);
}

/// The account button was tapped.
pub(super) fn tapped() {
    STATE.with_borrow_mut(|s| {
        if !matches!(s.step, Step::Idle) {
            return;
        }
        if let Some(name) = &s.name {
            s.step = Step::SignedIn(horizon_ios::show_alert(
                "Microsoft account",
                &format!("Signed in as {name}. Online-mode servers see this account."),
                &["OK", "Sign out"],
            ));
            return;
        }
        let Some(client_id) = msa::client_id() else {
            s.step = Step::Message(horizon_ios::show_alert(
                "Microsoft sign-in",
                msa::NOT_CONFIGURED,
                &["OK"],
            ));
            return;
        };
        let (send, receive) = channel();
        let spawned = std::thread::Builder::new()
            .name("Microsoft sign-in".into())
            .spawn(move || {
                let _ = send.send(msa::device_code(&client_id));
            });
        s.step = match spawned {
            Ok(_) => Step::Starting(receive),
            Err(e) => Step::Message(horizon_ios::show_alert(
                "Microsoft sign-in",
                &format!("Could not start the sign-in: {e}"),
                &["OK"],
            )),
        };
    });
}

fn message(text: &str) -> Step {
    Step::Message(horizon_ios::show_alert("Microsoft sign-in", text, &["OK"]))
}

fn waiting_alert(code: &DeviceCode, first: bool) -> AlertId {
    let text = if first {
        format!(
            "Your code is {}\n\nTap Copy & Open: the code is copied and the sign-in page opens. \
             Paste the code there and sign in with the Microsoft account that owns Minecraft: \
             Java Edition, then come back to Horizon.",
            code.user_code
        )
    } else {
        format!(
            "Waiting for you to sign in at {}\n\nCode: {}\n\nThis closes by itself once you \
             are signed in.",
            code.verification_uri, code.user_code
        )
    };
    let open = if first { "Copy & Open" } else { "Open Again" };
    horizon_ios::show_alert("Sign in with Microsoft", &text, &[open, "Cancel"])
}

/// Once a frame: the sign-in's threads and alerts.
pub(super) fn update() {
    STATE.with_borrow_mut(|s| {
        if let Some(renewing) = &s.renewing {
            match renewing.try_recv() {
                Ok(Ok(stored)) => {
                    keep(&stored);
                    use_account(&stored);
                    s.name = Some(stored.name);
                    s.renewing = None;
                }
                Ok(Err(e)) => {
                    // Offline, or the sign-in expired: the button offers a new one.
                    log::warn!("Horizon: the Microsoft account was not renewed: {e}");
                    s.name = None;
                    s.renewing = None;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => s.renewing = None,
            }
        }
        let step = std::mem::replace(&mut s.step, Step::Idle);
        s.step = match step {
            Step::Idle => Step::Idle,
            Step::Starting(receive) => match receive.try_recv() {
                Ok(Ok(code)) => {
                    let cancel = Arc::new(AtomicBool::new(false));
                    let (send, done) = channel();
                    let thread_code = code.clone();
                    let thread_cancel = cancel.clone();
                    let client_id = msa::client_id().unwrap_or_default();
                    let spawned = std::thread::Builder::new()
                        .name("Microsoft sign-in".into())
                        .spawn(move || {
                            let cancelled = || thread_cancel.load(Ordering::Relaxed);
                            let _ = send.send(msa::finish(&client_id, &thread_code, &cancelled));
                        });
                    match spawned {
                        Ok(_) => Step::Waiting {
                            alert: waiting_alert(&code, true),
                            code,
                            cancel,
                            done,
                        },
                        Err(e) => message(&format!("Could not start the sign-in: {e}")),
                    }
                }
                Ok(Err(e)) => message(&e),
                Err(TryRecvError::Empty) => Step::Starting(receive),
                Err(TryRecvError::Disconnected) => message("The sign-in stopped."),
            },
            Step::Waiting {
                code,
                alert,
                cancel,
                done,
            } => match done.try_recv() {
                Ok(Ok(stored)) => {
                    horizon_ios::dismiss_alert(alert);
                    keep(&stored);
                    use_account(&stored);
                    let name = stored.name.clone();
                    s.name = Some(stored.name);
                    message(&format!(
                        "Signed in as {name}. You can join online-mode servers now."
                    ))
                }
                Ok(Err(e)) => {
                    horizon_ios::dismiss_alert(alert);
                    if cancel.load(Ordering::Relaxed) {
                        Step::Idle
                    } else {
                        message(&e)
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    horizon_ios::dismiss_alert(alert);
                    message("The sign-in stopped.")
                }
                Err(TryRecvError::Empty) => match horizon_ios::alert_answer(alert) {
                    Some(0) => {
                        SystemClipboard::default().set(&code.user_code);
                        horizon_ios::open_url(&code.verification_uri);
                        Step::Waiting {
                            alert: waiting_alert(&code, false),
                            code,
                            cancel,
                            done,
                        }
                    }
                    Some(_) => {
                        // Cancel: the thread ends at its next check.
                        cancel.store(true, Ordering::Relaxed);
                        Step::Idle
                    }
                    None => Step::Waiting {
                        code,
                        alert,
                        cancel,
                        done,
                    },
                },
            },
            Step::SignedIn(alert) => match horizon_ios::alert_answer(alert) {
                Some(1) => {
                    horizon_ios::keychain::remove(KEY);
                    s.name = None;
                    message(
                        "Signed out. This run still plays as the account; from the next start, \
                         Horizon plays offline until you sign in again.",
                    )
                }
                Some(_) => Step::Idle,
                None => Step::SignedIn(alert),
            },
            Step::Message(alert) => match horizon_ios::alert_answer(alert) {
                Some(_) => Step::Idle,
                None => Step::Message(alert),
            },
        };
    });
}
