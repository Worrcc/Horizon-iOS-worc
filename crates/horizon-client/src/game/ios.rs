//! iOS: the app lifecycle the desktop game never sees (ios/README.md).
//!
//! - `run_app` never returns and `exit` does nothing (an app can't quit itself, QA1561):
//!   [`Session`] ends the run itself once the game asks to quit, as `run_app`'s caller does
//!   elsewhere, and shows a fatal error in an alert rather than on a terminal.
//! - iOS can end a backgrounded app without a word: going to the background pauses the game
//!   and saves the world, as `/save-all` does.
use super::{Game, QUITTING};
use horizon_client::{Result, crash, logging};
use std::sync::atomic::Ordering;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::ActiveEventLoop,
    window::WindowId,
};

/// The application handler, and what runs once it quits.
pub(super) struct Session<A> {
    app: A,
    finish: Option<fn(&mut A) -> Result<()>>,
}

impl<A: ApplicationHandler> Session<A> {
    pub(super) fn new(app: A, finish: fn(&mut A) -> Result<()>) -> Self {
        Self {
            app,
            finish: Some(finish),
        }
    }
    fn running(&self) -> bool {
        self.finish.is_some()
    }
    /// After every event: once the game has quit, save (`exiting`), report, and end the process.
    fn check(&mut self, event_loop: &ActiveEventLoop) {
        if !QUITTING.load(Ordering::Relaxed) {
            return;
        }
        let Some(finish) = self.finish.take() else {
            return;
        };
        self.app.exiting(event_loop);
        match finish(&mut self.app) {
            Ok(()) => {
                logging::flush();
                std::process::exit(0);
            }
            Err(error) => {
                log::error!("Horizon: {error}");
                let home = logging::game_home();
                let report = crash::report_error(&home, "client", &*error);
                logging::flush();
                let mut message = error.to_string();
                if let Some(report) = report
                    && let Ok(relative) = report.strip_prefix(&home)
                {
                    message.push_str(&format!(
                        "\n\nThe crash report is in the Files app: Horizon/{}",
                        relative.display()
                    ));
                }
                // The window stays up behind the alert; its button ends the app.
                horizon_ios::fatal_alert("Horizon stopped", &message);
            }
        }
    }
}

impl<A: ApplicationHandler> ApplicationHandler for Session<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running() {
            self.app.resumed(event_loop);
            self.check(event_loop);
        }
    }
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        if self.running() {
            self.app.suspended(event_loop);
            self.check(event_loop);
        }
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        if self.running() {
            self.app.window_event(event_loop, id, event);
            self.check(event_loop);
        }
    }
    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        if self.running() {
            self.app.device_event(event_loop, id, event);
            self.check(event_loop);
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.running() {
            self.app.about_to_wait(event_loop);
            self.check(event_loop);
        }
    }
    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        log::warn!("Horizon: iOS is low on memory; a lower render distance helps");
        self.app.memory_warning(event_loop);
    }
    /// The player swiped the app away while it was in the foreground.
    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if self.finish.take().is_some() {
            self.app.exiting(event_loop);
        }
    }
}

impl Game<'_> {
    /// `applicationWillResignActive`: Control Center, a call, the app switcher, the home screen.
    pub(super) fn enter_background(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(id) = self.window.as_ref().map(|w| w.id()) {
            // iOS sends no Focused: a lost focus pauses (Gui.setPauseScreen) and lets go of
            // held buttons.
            self.window_event(event_loop, id, WindowEvent::Focused(false));
        }
        self.touch_release_all(event_loop);
        if self.title.is_some() || self.remote.is_some() || self.pending_save.is_some() {
            return;
        }
        let saved = match &self.integrated {
            // Single player's world is its server thread's.
            Some(server) => server
                .call(|g| g.control_save(false).map_err(|e| e.to_string()))
                .unwrap_or_else(|| Err("the integrated server stopped".into()))
                .map_err(Into::into),
            None => self.save_game(),
        };
        match saved {
            Ok(()) => log::info!("Horizon: saved the world on going to the background"),
            Err(e) => {
                log::warn!("Horizon: failed to save the world on going to the background: {e}")
            }
        }
    }

    /// `applicationDidBecomeActive`.
    pub(super) fn leave_background(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(id) = self.window.as_ref().map(|w| w.id()) {
            self.window_event(event_loop, id, WindowEvent::Focused(true));
        }
        // The world stays paused until the player taps Back to Game, as after a lost focus.
    }
}
