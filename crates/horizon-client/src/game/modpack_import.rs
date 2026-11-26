//! Shared worker flow for both Mods pages and the control socket.
use super::{Game, Loading};
use horizon_client::{
    mod_list,
    options_ui::{OptionsUi, Page},
};
use horizon_mods::zip_import::{self, Report};
use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
};

#[derive(Clone, Copy)]
pub(super) enum Cleanup {
    Never,
    Success,
    Always,
}
enum Update {
    Progress(usize, usize),
    Finished(Result<Report, String>),
}
struct Pending {
    receiver: Receiver<Update>,
    home: PathBuf,
    enable: bool,
    reply: Option<Sender<String>>,
}
#[derive(Default)]
pub(super) struct Job {
    pending: Option<Pending>,
    /// When the Mods page last looked for dropped zips.
    scanned: Option<std::time::Instant>,
}
pub(super) struct Completed {
    home: PathBuf,
    result: Result<Report, String>,
    enable: bool,
    reply: Option<Sender<String>>,
}
impl Job {
    pub(super) fn start(
        &mut self,
        path: PathBuf,
        view: &mut OptionsUi,
        cleanup: Cleanup,
        enable: bool,
        reply: Option<Sender<String>>,
    ) -> Result<(), String> {
        if self.pending.is_some() {
            return Err("A modpack import is already running".into());
        }
        let home = Game::game_dir();
        let dir = home.join(mod_list::DIR);
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("Modpack import".into())
            .spawn(move || {
                let result = zip_import::import(&path, &dir, |done, total| {
                    let _ = sender.send(Update::Progress(done, total));
                });
                let installed = result.as_ref().is_ok_and(|r| r.ids().next().is_some());
                if matches!(cleanup, Cleanup::Always)
                    || matches!(cleanup, Cleanup::Success) && installed
                {
                    let _ = std::fs::remove_file(&path);
                }
                let _ = sender.send(Update::Finished(result));
            })
            .map_err(|e| e.to_string())?;
        self.pending = Some(Pending {
            receiver,
            home,
            enable,
            reply,
        });
        view.modpack.running = true;
        view.modpack.done = 0;
        view.modpack.total = 0;
        view.modpack.summary = None;
        view.modpack.skipped.clear();
        view.modpack.error = None;
        view.modpack.ids.clear();
        if self.pending.as_ref().is_some_and(|p| p.reply.is_none()) {
            view.show(Page::Modpack);
        }
        Ok(())
    }
    /// While the Mods page shows, its list of dropped zips follows the folder (a zip copied in
    /// from the Files app appears without leaving the page).
    fn rescan(&mut self, view: &mut OptionsUi) {
        if view.page() != Page::Mods
            || self
                .scanned
                .is_some_and(|at| at.elapsed() < std::time::Duration::from_secs(1))
        {
            return;
        }
        self.scanned = Some(std::time::Instant::now());
        view.modpack.scan(&Game::game_dir().join(mod_list::DIR));
    }
    pub(super) fn poll(&mut self, view: &mut OptionsUi) -> Option<Completed> {
        self.rescan(view);
        let p = self.pending.as_mut()?;
        let result = loop {
            match p.receiver.try_recv() {
                Ok(Update::Progress(done, total)) => {
                    view.modpack.done = done;
                    view.modpack.total = total;
                }
                Ok(Update::Finished(result)) => break result,
                Err(mpsc::TryRecvError::Empty) => return None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    break Err("Modpack import worker stopped".into());
                }
            }
        };
        let pending = self.pending.take()?;
        view.modpack.running = false;
        match &result {
            Ok(r) => {
                view.modpack.summary = Some((r.added.len(), r.replaced.len(), r.skipped.len()));
                view.modpack.skipped.clone_from(&r.skipped);
                view.modpack.ids = r.ids().map(str::to_owned).collect();
            }
            Err(e) => view.modpack.error = Some(e.clone()),
        }
        view.modpack.scan(&Game::game_dir().join(mod_list::DIR));
        Some(Completed {
            home: pending.home,
            result,
            enable: pending.enable,
            reply: pending.reply,
        })
    }
}
fn error(view: &mut OptionsUi, error: String) {
    view.modpack.error = Some(error);
    view.show(Page::Modpack);
}
/// Desktop deliberately uses the folder workflow; iOS presents the system ZIP picker.
pub(super) fn pick() {
    #[cfg(target_os = "ios")]
    horizon_ios::pick_modpack();
    #[cfg(not(target_os = "ios"))]
    {
        let dir = Game::game_dir().join(mod_list::DIR);
        let _ = std::fs::create_dir_all(&dir);
        super::packs::open_folder(&dir);
    }
}
fn selected_copy() -> Option<PathBuf> {
    horizon_ios::take_modpack()
}
fn archive(view: &OptionsUi, i: usize) -> Option<PathBuf> {
    view.modpack.archives.get(i).cloned()
}
fn reply(done: Completed) {
    if let Some(reply) = done.reply {
        let value = match done.result {
            Ok(r) => serde_json::to_value(r).unwrap_or_default(),
            Err(e) => serde_json::json!({"error": e}),
        };
        let _ = reply.send(value.to_string());
    }
}

impl Game<'_> {
    pub(super) fn import_archive(&mut self, i: usize) {
        if let Some(path) = archive(&self.ui.options, i)
            && let Err(e) =
                self.mods
                    .import
                    .start(path, &mut self.ui.options, Cleanup::Success, false, None)
        {
            error(&mut self.ui.options, e);
        }
    }
    pub(super) fn control_import_modpack(
        &mut self,
        path: &Path,
        enable: bool,
        reply: Sender<String>,
    ) -> Result<(), String> {
        self.mods.import.start(
            path.to_owned(),
            &mut self.ui.options,
            Cleanup::Never,
            enable,
            Some(reply),
        )
    }
    pub(super) fn enable_imported(&mut self) {
        let ids = self.ui.options.modpack.ids.clone();
        for id in ids {
            self.settings.mods.turn_on(&id);
        }
        self.settings.save(&Self::options_path());
        self.start_mods();
    }
    pub(super) fn poll_modpack(&mut self) {
        if let Some(path) = selected_copy()
            && let Err(e) =
                self.mods
                    .import
                    .start(path, &mut self.ui.options, Cleanup::Always, false, None)
        {
            error(&mut self.ui.options, e);
        }
        let Some(done) = self.mods.import.poll(&mut self.ui.options) else {
            return;
        };
        if done.home == Self::game_dir()
            && let Ok(r) = &done.result
        {
            let ids: Vec<_> = r.ids().collect();
            // Keep running/native/dev rows and add the new disk packages without starting any.
            for m in super::mods::installed_on_disk(&self.mods.dev)
                .into_iter()
                .filter(|m| ids.contains(&m.id.as_str()))
            {
                if let Some(old) = self
                    .settings
                    .mods
                    .installed
                    .iter_mut()
                    .find(|old| old.id == m.id)
                {
                    if !old.dev && old.native.is_none() {
                        *old = m;
                    }
                } else {
                    self.settings.mods.installed.push(m);
                }
            }
            if done.enable {
                self.enable_imported();
            }
        }
        reply(done);
    }
}
impl Loading {
    pub(super) fn menu_import_archive(&mut self, i: usize) {
        if let Some(path) = archive(&self.menu.options, i)
            && let Err(e) =
                self.menu
                    .import
                    .start(path, &mut self.menu.options, Cleanup::Success, false, None)
        {
            error(&mut self.menu.options, e);
        }
    }
    pub(super) fn menu_enable_imported(&mut self) {
        for id in &self.menu.options.modpack.ids {
            self.settings.mods.turn_on(id);
        }
        self.settings.save(&Game::options_path());
    }
    pub(super) fn poll_menu_modpack(&mut self) {
        if let Some(path) = selected_copy()
            && let Err(e) =
                self.menu
                    .import
                    .start(path, &mut self.menu.options, Cleanup::Always, false, None)
        {
            error(&mut self.menu.options, e);
        }
        if let Some(done) = self.menu.import.poll(&mut self.menu.options) {
            if done.home == Game::game_dir() && done.result.is_ok() {
                let dev = self.options.as_ref().map_or(&[][..], |o| &o.dev_mods[..]);
                self.settings.mods.installed = super::mods::installed_on_disk(dev);
            }
            reply(done);
        }
    }
}
