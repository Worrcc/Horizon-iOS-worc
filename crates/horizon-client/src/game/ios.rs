//! iOS: the app lifecycle the desktop game never sees (ios/README.md).
//!
//! - `run_app` never returns and `exit` does nothing (an app can't quit itself, QA1561):
//!   [`Session`] ends the run itself once the game asks to quit, as `run_app`'s caller does
//!   elsewhere, and shows a fatal error in an alert rather than on a terminal.
//! - No launcher prepares the game files: [`missing_resources`] says where they go before the
//!   game tries to load them.
//! - iOS can end a backgrounded app without a word: going to the background pauses the game
//!   and saves the world, as `/save-all` does.
use super::{Game, QUITTING};
use horizon_client::{Result, crash, logging};
use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::{DeviceEvent, DeviceId, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    platform::ios::{ScreenEdge, ValidOrientations, WindowAttributesExtIOS},
    window::{Window, WindowAttributes, WindowId},
};

/// The game's view: landscape, nothing of the system's over it, and the screen edges kept for
/// the game, so a swipe from the bottom while walking doesn't leave it (a second swipe does).
pub(super) fn window_attributes(attributes: WindowAttributes) -> WindowAttributes {
    attributes
        .with_valid_orientations(ValidOrientations::Landscape)
        .with_prefers_status_bar_hidden(true)
        .with_prefers_home_indicator_hidden(true)
        .with_preferred_screen_edges_deferring_system_gestures(ScreenEdge::ALL)
}

/// The window's sizes, in the log: what a wrong-sized picture comes down to.
pub(super) fn log_window(window: &Window) {
    let (inner, outer) = (window.inner_size(), window.outer_size());
    log::info!(
        "Horizon: iOS window {}x{} px (screen frame {}x{} px) at scale {}",
        inner.width,
        inner.height,
        outer.width,
        outer.height,
        window.scale_factor()
    );
}

/// The window's size when the surface (`surface`, in pixels) no longer has it: a `Resized` lost
/// around a rotation would otherwise leave the picture stretched over the screen.
pub(super) fn size_change(window: &Window, surface: [u32; 2]) -> Option<PhysicalSize<u32>> {
    let size = window.inner_size();
    if size.width == 0 || size.height == 0 || [size.width, size.height] == surface {
        return None;
    }
    log::info!(
        "Horizon: iOS window is {}x{} px, the surface {}x{}: resizing",
        size.width,
        size.height,
        surface[0],
        surface[1]
    );
    Some(size)
}

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

/// After a fatal error the game gets no more events; the loop only keeps the alert coming up.
fn wait_for_alert(event_loop: &ActiveEventLoop) {
    horizon_ios::poll_alert();
    event_loop.set_control_flow(ControlFlow::WaitUntil(
        Instant::now() + Duration::from_millis(100),
    ));
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
        if self.running() {
            super::account::update();
            horizon_ios::poll_alert();
        }
        if !self.running() {
            wait_for_alert(event_loop);
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

/// What to tell a player whose device has no game files at `resources` (relative to the game
/// folder, Documents), or `None` when they are there.
pub(super) fn missing_resources(resources: &Path) -> Option<String> {
    if resources.join("horizon-resources.json").is_file() {
        return None;
    }
    let place = "Files › On My iPhone (or iPad) › Horizon › resources";
    // Unzipped into a folder of the same name, or copied in under another name.
    let nested = resources
        .join("resources")
        .join("horizon-resources.json")
        .is_file();
    log::warn!(
        "Horizon: no game files at {} (horizon-resources.json is missing)",
        resources.display()
    );
    Some(if nested {
        format!(
            "Horizon found its game files, but one folder too deep. Move the inner \"resources\" \
             folder up, so the file horizon-resources.json is directly in {place}."
        )
    } else {
        format!(
            "Horizon needs the game files prepared from your own copy of Minecraft, and they \
             aren't on this device yet.\n\nMake them on a computer, from Minecraft: Java Edition \
             26.3, with the Horizon launcher (playhorizon.dev). Then copy its \"resources\" folder \
             to {place}, so the file horizon-resources.json is directly in it.\n\nThe \
             Horizon-iOS repository's ios/README.md has the steps."
        )
    })
}

/// Shows `message` and nothing else until the player closes the app: no game window, no
/// crash report (nothing crashed).
pub(super) fn show_error(title: &'static str, message: String) -> Result<()> {
    struct ErrorOnly {
        title: &'static str,
        message: String,
        shown: bool,
    }
    impl ApplicationHandler for ErrorOnly {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            self.about_to_wait(event_loop);
        }
        fn window_event(&mut self, _: &ActiveEventLoop, _: WindowId, _: WindowEvent) {}
        fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
            if !self.shown {
                self.shown = true;
                horizon_ios::fatal_alert(self.title, &self.message);
            }
            wait_for_alert(event_loop);
        }
    }
    logging::flush();
    EventLoop::new()?.run_app(&mut ErrorOnly {
        title,
        message,
        shown: false,
    })?;
    Ok(())
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
