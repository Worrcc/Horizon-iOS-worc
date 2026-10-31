//! Options > Horizon Settings > Instances (iOS): the game folders the player switches between
//! (`horizon_client::instance_list`). On the title with no world open a pick takes effect at
//! once: the game moves into the folder and reloads its settings, worlds and packs. In a world
//! it takes effect at the next start.

use super::{Game, Loading};
use horizon_client::instance_list::{self, Listing};
use horizon_client::options_ui::Event;

pub(super) fn listing() -> Listing {
    let home = horizon_client::logging::game_home();
    let cwd = std::env::current_dir().unwrap_or_default();
    Listing::read(&home, &instance_list::current(&home, &cwd))
}

/// Saves the pick for every later start; `false` when it couldn't be saved.
fn pick(id: &str) -> bool {
    let home = horizon_client::logging::game_home();
    instance_list::select(&home, id)
        .map_err(|e| log::warn!("Horizon: instance {id} not picked: {e}"))
        .is_ok()
}

/// Makes a new instance; the player picks it to play it.
fn create() {
    if let Err(e) = instance_list::create(&horizon_client::logging::game_home()) {
        log::warn!("Horizon: no new instance: {e}");
    }
}

impl Game<'_> {
    /// The Instances page's events, in a world or on the title after one.
    pub(super) fn instance_event(&mut self, event: Event) {
        match event {
            // On the title with no world open, a pick switches now; from a world, next start.
            Event::PickInstance(id)
                if pick(&id)
                    && id != self.settings.instances.current
                    && self.title.is_some()
                    && self.player.is_none() =>
            {
                self.switch_instance(&id);
            }
            Event::PickInstance(_) => {}
            Event::NewInstance => create(),
            _ => {}
        }
        self.settings.instances = listing();
    }
}

impl Loading {
    /// The Instances page's events on the startup title: a pick switches folders now.
    pub(super) fn menu_instance_event(&mut self, event: Event) {
        match event {
            Event::PickInstance(id) if pick(&id) && id != self.settings.instances.current => {
                self.switch_instance(&id);
            }
            Event::NewInstance => create(),
            _ => {}
        }
        self.settings.instances = listing();
    }
}

impl Game<'_> {
    /// Plays in `id`'s folder from here on (no world open): its options.txt, saves and packs.
    fn switch_instance(&mut self, id: &str) {
        self.settings.save(&Self::options_path());
        let folder = instance_list::folder(&horizon_client::logging::game_home(), id);
        if let Err(e) =
            std::fs::create_dir_all(&folder).and_then(|()| std::env::set_current_dir(&folder))
        {
            log::warn!("Horizon: can't play in {}: {e}", folder.display());
            return;
        }
        log::info!("Horizon: playing instance {id} ({})", folder.display());
        self.settings = super::Settings::load(&Self::options_path());
        self.apply_settings();
        let mut title =
            horizon_client::title::Title::new(folder.join("saves"), &self.options.resources);
        title.last_server = self.settings.file.string("lastServer");
        self.title = Some(title);
        self.reload_packs();
    }
}
