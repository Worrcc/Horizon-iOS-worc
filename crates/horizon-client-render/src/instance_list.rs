//! Instances on iOS, where no separate launcher can start the game in another folder: separate
//! game folders, each with its own options, worlds, resource packs, shader packs and mods, as
//! Options > Horizon Settings > Instances lists them. The game folder (Documents) is the default
//! instance, "Horizon", as the desktop launcher's game home is (launcher/src/instances.rs); new
//! ones are `instances/<id>/` in it. The prepared game files (`resources/`) stay shared.
//!
//! `instances/instances.json` keeps the pick and the names, so renaming one never moves a folder.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The default instance's id: the game folder itself.
pub const DEFAULT: &str = "horizon";
/// Where new instances live, in the game folder.
pub const DIR: &str = "instances";
const FILE: &str = "instances.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// `horizon`, or the folder's name in `instances/`.
    pub id: String,
    pub name: String,
}

/// What the Instances page shows: listed when it opens, empty where the page has no place
/// (every platform but iOS: the launcher has instances there).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// The id of the instance playing now.
    pub current: String,
    /// The id picked to play at the next start, when it can't switch now (in a world).
    pub next: Option<String>,
}

impl Listing {
    pub fn read(home: &Path, current: &str) -> Self {
        let picked = selected(home);
        Self {
            entries: list(home),
            next: (picked != current).then_some(picked),
            current: current.to_owned(),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    selected: Option<String>,
    #[serde(default)]
    names: BTreeMap<String, String>,
}

fn read(home: &Path) -> Index {
    std::fs::read(home.join(DIR).join(FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn write(home: &Path, index: &Index) -> std::io::Result<()> {
    let dir = home.join(DIR);
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(index).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(FILE), json)
}

/// An id names a folder of `instances/`: lowercase letters, digits and `-`, never `horizon`.
fn valid_id(id: &str) -> bool {
    id != DEFAULT
        && !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// The instances: the default first, then the folders of `instances/` by id.
pub fn list(home: &Path) -> Vec<Entry> {
    let index = read(home);
    let mut ids: Vec<String> = std::fs::read_dir(home.join(DIR))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|id| valid_id(id))
        .collect();
    ids.sort();
    let name = |id: &str, fallback: &str| {
        index
            .names
            .get(id)
            .cloned()
            .unwrap_or_else(|| fallback.to_owned())
    };
    std::iter::once(Entry {
        id: DEFAULT.to_owned(),
        name: name(DEFAULT, "Horizon"),
    })
    .chain(ids.into_iter().map(|id| Entry {
        name: name(&id, &id),
        id,
    }))
    .collect()
}

/// The picked instance's id, or the default when the pick's folder is gone.
pub fn selected(home: &Path) -> String {
    read(home)
        .selected
        .filter(|id| valid_id(id) && home.join(DIR).join(id).is_dir())
        .unwrap_or_else(|| DEFAULT.to_owned())
}

/// The folder an instance plays in.
pub fn folder(home: &Path, id: &str) -> PathBuf {
    if valid_id(id) {
        home.join(DIR).join(id)
    } else {
        home.to_path_buf()
    }
}

/// The id of the instance whose folder is `cwd`: the default unless it is one of `instances/`.
pub fn current(home: &Path, cwd: &Path) -> String {
    cwd.strip_prefix(home.join(DIR))
        .ok()
        .and_then(|rest| rest.to_str())
        .filter(|id| valid_id(id))
        .map_or_else(|| DEFAULT.to_owned(), str::to_owned)
}

/// Picks `id` for this and every later start.
pub fn select(home: &Path, id: &str) -> std::io::Result<()> {
    let mut index = read(home);
    index.selected = valid_id(id).then(|| id.to_owned());
    write(home, &index)
}

/// Makes a new, empty instance, "Instance 2" in `instances/instance-2/` (the first number free),
/// and returns its id.
pub fn create(home: &Path) -> std::io::Result<String> {
    let mut index = read(home);
    let n = (2..)
        .find(|n| {
            let id = format!("instance-{n}");
            !home.join(DIR).join(&id).exists() && !index.names.contains_key(&id)
        })
        .expect("a free number");
    let id = format!("instance-{n}");
    std::fs::create_dir_all(home.join(DIR).join(&id))?;
    index.names.insert(id.clone(), format!("Instance {n}"));
    write(home, &index)?;
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder per test, removed when it ends.
    struct Home(PathBuf);
    impl Home {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for Home {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn home() -> Home {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("horizon-instances-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Home(dir)
    }

    #[test]
    fn the_game_folder_is_the_default() {
        let home = home();
        assert_eq!(
            list(home.path()),
            [Entry {
                id: DEFAULT.into(),
                name: "Horizon".into()
            }]
        );
        assert_eq!(selected(home.path()), DEFAULT);
        assert_eq!(folder(home.path(), DEFAULT), home.path());
        assert_eq!(current(home.path(), home.path()), DEFAULT);
    }

    #[test]
    fn new_instances_take_the_first_free_number() {
        let home = home();
        assert_eq!(create(home.path()).unwrap(), "instance-2");
        assert_eq!(create(home.path()).unwrap(), "instance-3");
        let names: Vec<_> = list(home.path()).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Horizon", "Instance 2", "Instance 3"]);
        assert!(folder(home.path(), "instance-3").is_dir());
        let cwd = folder(home.path(), "instance-3");
        assert_eq!(current(home.path(), &cwd), "instance-3");
        let listing = Listing::read(home.path(), "instance-3");
        assert_eq!(
            (listing.current.as_str(), listing.next),
            ("instance-3", Some(DEFAULT.into()))
        );
    }

    #[test]
    fn the_pick_is_kept_until_its_folder_goes() {
        let home = home();
        let id = create(home.path()).unwrap();
        select(home.path(), &id).unwrap();
        assert_eq!(selected(home.path()), id);
        std::fs::remove_dir(folder(home.path(), &id)).unwrap();
        assert_eq!(selected(home.path()), DEFAULT);
        select(home.path(), DEFAULT).unwrap();
        assert_eq!(selected(home.path()), DEFAULT);
    }

    #[test]
    fn folders_copied_in_are_instances_and_odd_names_are_not() {
        let home = home();
        for dir in ["modded", "Bad Name", "horizon"] {
            std::fs::create_dir_all(home.path().join(DIR).join(dir)).unwrap();
        }
        let ids: Vec<_> = list(home.path()).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, [DEFAULT, "modded"]);
        assert_eq!(folder(home.path(), "../x"), home.path());
    }
}
