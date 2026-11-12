//! Horizon mods in the game (`docs/mods.md`): loading the ones turned on,
//! their tick, frame, chat and command hooks, their HUD panels and what
//! they print. With no mod on, `horizon_mods::Mods` holds no engine and
//! every hook here returns after one check.
//!
//! Code mods run only in builds with the `mods` feature (release builds):
//! without it the game links no wasmtime, and every hook does nothing.
//! Content mods (`content_root`) load in every build.
use super::Game;
use horizon_client::{
    command::{Dispatcher, vanilla},
    extras_hud::Corners,
    hud::HudBatch,
    mod_list::{self, Installed},
};
use std::{path::PathBuf, time::Duration};
#[cfg(feature = "mods")]
use {
    super::Screen,
    super::{execution_role::ExecutionRole, lan::LanHost, server_mods::ModEvent},
    horizon_client::{
        extras::Corner,
        extras_hud,
        items::ItemStack,
        keybinds,
        options_ui::Page,
        text::{self, Text},
        ui,
    },
    horizon_mods::{
        Clock, ModKey, Mods, Output, Player, Runs, ServerPlayer, Snapshot, Status, UiScreen,
        UiWidget,
    },
    std::time::Instant,
    winit::{event::MouseButton, keyboard::KeyCode},
};

#[derive(Default)]
pub(super) struct ModsState {
    pub(super) import: super::modpack_import::Job,
    #[cfg(feature = "mods")]
    pub(super) mods: Mods,
    /// Capability `input`: a claimed item's use while the button is down.
    #[cfg(feature = "mods")]
    pub(super) using: Option<super::mod_input::ModUse>,
    /// Capability `render`'s camera shakes.
    #[cfg(feature = "mods")]
    pub(super) shake: super::mod_render::Shake,
    /// `--dev-mod PATH`: always on, reloaded when rebuilt.
    pub(super) dev: Vec<PathBuf>,
    /// When dev mods were last checked for a rebuild.
    #[cfg(feature = "mods")]
    checked: Option<Instant>,
    /// The chat's command tree while a running mod owns commands.
    #[cfg(feature = "mods")]
    tree: Option<Dispatcher>,
    /// A server mod runs here: what players do is queued for it.
    #[cfg(feature = "mods")]
    pub(super) server: bool,
    /// The players server mods saw last tick, for joins and leaves.
    #[cfg(feature = "mods")]
    pub(super) players: Vec<ServerPlayer>,
    /// What players did this tick, for server mods at its end.
    #[cfg(feature = "mods")]
    pub(super) events: Vec<ModEvent>,
    /// A running server mod has capability `world`: its calls get the
    /// levels' chunks to read.
    #[cfg(feature = "mods")]
    pub(super) world: bool,
    /// Blocks server mods set, by dimension, made before that level's next
    /// tick.
    #[cfg(feature = "mods")]
    pub(super) block_writes: Vec<(String, horizon_mods::BlockChange)>,
    /// Server mods' `world.set-loot-table`: dimension, container, table, after the block writes.
    #[cfg(feature = "mods")]
    pub(super) loot_writes: Vec<(String, (i32, i32, i32), String)>,
    /// A running server mod has capability `entities`: its calls get the
    /// loaded mobs to read.
    #[cfg(feature = "mods")]
    pub(super) entities: bool,
    /// Entity order resource names cached for the current content root.
    #[cfg(feature = "mods")]
    pub(super) entity_resources: Option<(PathBuf, std::sync::Arc<horizon_mods::EntityResources>)>,
    /// Server mods' orders to mobs, by mob id, carried out before their
    /// level's next tick.
    #[cfg(feature = "mods")]
    pub(super) mob_orders: Vec<(u32, horizon_mods::EntityOrder)>,
    /// Server mods' `mount` (the player's UUID) and `dismount` (none) orders, by mob id,
    /// carried out after the tick's other orders.
    #[cfg(feature = "mods")]
    pub(super) seatings: Vec<(u32, Option<String>)>,
    /// Mob function writes, deferred until the mob's level ticks.
    #[cfg(feature = "mods")]
    pub(super) entity_operations: Vec<horizon_mods::Output>,
    /// Server mods' `entities.ride`: rider and vehicle (none: off), after the seatings.
    #[cfg(feature = "mods")]
    pub(super) rides: Vec<(u32, Option<u32>)>,
    /// Chunks first generated since server mods last ran, for `world.generated-chunks`.
    #[cfg(feature = "mods")]
    pub(super) generated_chunks: Vec<horizon_mods::ChunkPos>,
    /// Server mods' spawns: dimension, kind, position, yaw and data, made before that
    /// dimension's next tick.
    #[cfg(feature = "mods")]
    pub(super) mob_spawns: Vec<super::server_mods::run::ModSpawn>,
    /// Players' right clicks on content mods' mobs this tick: who, the mob, its kind and the
    /// item held, for capability `entities` at the tick's end.
    #[cfg(feature = "mods")]
    pub(super) interactions: Vec<horizon_mods::EntityInteraction>,
    /// Player actions collected until the end of this server tick.
    #[cfg(feature = "mods")]
    pub(super) item_uses: Vec<horizon_mods::PlayerItemUse>,
    /// A block/entity click can fall through to a second Item packet in the same tick.
    #[cfg(feature = "mods")]
    pub(super) use_fallbacks: std::collections::HashMap<(String, String), String>,
    #[cfg(feature = "mods")]
    pub(super) attacks: Vec<horizon_mods::PlayerAttack>,
    #[cfg(feature = "mods")]
    pub(super) hurt_events: Vec<horizon_mods::HurtEvent>,
    #[cfg(feature = "mods")]
    pub(super) deaths: Vec<horizon_mods::Death>,
    #[cfg(feature = "mods")]
    pub(super) player_damage: Vec<horizon_mods::PlayerDamage>,
    #[cfg(feature = "mods")]
    pub(super) last_hurt_events: std::sync::Arc<[horizon_mods::HurtEvent]>,
    #[cfg(feature = "mods")]
    pub(super) last_deaths: std::sync::Arc<[horizon_mods::Death]>,
    #[cfg(feature = "mods")]
    pub(super) last_player_damage: std::sync::Arc<[horizon_mods::PlayerDamage]>,
    /// Published actions remain visible to commands until the next tick.
    #[cfg(feature = "mods")]
    pub(super) last_item_uses: std::sync::Arc<[horizon_mods::PlayerItemUse]>,
    #[cfg(feature = "mods")]
    pub(super) last_attacks: std::sync::Arc<[horizon_mods::PlayerAttack]>,
    #[cfg(feature = "mods")]
    pub(super) block_tags: Option<
        std::sync::Arc<std::collections::HashMap<String, std::collections::HashSet<String>>>,
    >,
    /// World operations run after the tick's block writes, in request order.
    #[cfg(feature = "mods")]
    pub(super) world_operations: Vec<horizon_mods::Output>,
    #[cfg(feature = "mods")]
    pub(super) break_caller: Option<String>,
    /// Valid item ids and stack sizes for player inventory calls.
    #[cfg(feature = "mods")]
    pub(super) item_limits: Option<std::sync::Arc<std::collections::HashMap<String, u32>>>,
    /// Inventory overflow waiting for the player's dimension to tick.
    #[cfg(feature = "mods")]
    pub(super) item_drops: Vec<super::mod_players::ItemDrop>,
    /// `command.run` lines for this window's own world, run on the next tick:
    /// run at once, their chat lines would reach the mods while their output
    /// is still being shown, and a mod answering its own lines would recurse.
    #[cfg(feature = "mods")]
    commands: Vec<String>,
    /// `command.run-quiet` batches for this window's own world, by mod id,
    /// run on the next tick as `commands` are.
    #[cfg(feature = "mods")]
    quiet: Vec<(String, Vec<String>)>,
    #[cfg(feature = "mods")]
    mod_quiet: Vec<(String, Vec<String>)>,
    /// The server's command tree with the running mods' commands added.
    #[cfg(feature = "mods")]
    remote_tree: Option<Dispatcher>,
    /// A `ui` mod's screen or settings page while it is up.
    #[cfg(feature = "mods")]
    screen: Option<ModScreen>,
    /// The full-window canvas and its held pointer buttons.
    #[cfg(feature = "mods")]
    pub(super) canvas_screen: Option<super::mod_canvas_screen::CanvasScreenHost>,
    #[cfg(feature = "mods")]
    pub(super) container: super::mod_container::ContainerScreenHost,
    /// The HUD font's advances for `render.text-width`, made once.
    #[cfg(feature = "mods")]
    font_widths: std::sync::Arc<[f32]>,
    /// Capability `recipes`'s tables, made once a mod reads them.
    #[cfg(feature = "mods")]
    pub(super) recipes: Option<std::sync::Arc<horizon_mods::Recipes>>,
    /// Extras that are official mods now, being installed.
    #[cfg(feature = "mods")]
    pub(super) moved: super::moved_extras::MovedState,
    /// Playing at the last frame, for `mod_input_frame`.
    #[cfg(feature = "mods")]
    pub(super) playing: bool,
    /// Capability `render`'s canvas images, between frames.
    #[cfg(feature = "mods")]
    pub(super) canvas: super::mod_canvas::CanvasHost,
    /// Capability `world-read`'s change counters and caches, between lends.
    #[cfg(feature = "mods")]
    pub(super) client_world: super::mod_client_world::ClientWorldHost,
}

/// The mappings `input.mapping` answers for, with the default key their
/// binding is translated to (none: a mouse button, attack or use).
#[cfg(feature = "mods")]
const MAPPINGS: [(&str, Option<winit::keyboard::KeyCode>); 9] = {
    use winit::keyboard::KeyCode as K;
    [
        ("key.forward", Some(K::KeyW)),
        ("key.left", Some(K::KeyA)),
        ("key.back", Some(K::KeyS)),
        ("key.right", Some(K::KeyD)),
        ("key.jump", Some(K::Space)),
        ("key.sneak", Some(K::ShiftLeft)),
        ("key.sprint", Some(K::ControlLeft)),
        ("key.attack", None),
        ("key.use", None),
    ]
};
/// Characters `render.text-width` knows exactly: U+0000 to U+024F.
#[cfg(feature = "mods")]
const FONT_CHARS: u32 = 0x250;

/// A `ui` mod's own screen, or its settings page from Options > Mods: the
/// widgets the mod pushed, laid out and drawn with the menus' own widgets.
#[cfg(feature = "mods")]
struct ModScreen {
    /// The running mod's index in `mods.mods.entries()`.
    owner: usize,
    /// Opened from Options > Mods: Done and Esc return there.
    settings: bool,
    /// What the mod's screen replaced (Playing, Chat, ...), to restore.
    under: Screen,
    /// One scroll per `list` widget, in widget order.
    scrolls: Vec<ui::Scroll>,
    /// Keys and widgets share one scrollable column.
    scroll: ui::Scroll,
    /// A key row waiting for the key to bind: the mod key's name.
    binding: Option<String>,
}
impl ModsState {
    pub(super) fn screen_open(&self) -> bool {
        #[cfg(feature = "mods")]
        {
            self.screen.is_some()
        }
        #[cfg(not(feature = "mods"))]
        {
            false
        }
    }

    /// The chat's command tree: vanilla's, plus the running mods' commands.
    pub(super) fn dispatcher(&self) -> &Dispatcher {
        #[cfg(feature = "mods")]
        if let Some(tree) = &self.tree {
            return tree;
        }
        vanilla::game_dispatcher()
    }
    /// The server's tree plus the mods' commands, when a mod owns any.
    pub(super) fn remote_dispatcher(&self) -> Option<&Dispatcher> {
        #[cfg(feature = "mods")]
        if let Some(tree) = &self.remote_tree {
            return Some(tree);
        }
        None
    }
    /// Whether a mod runs: the chat keeps its lines for them.
    pub(super) fn running(&self) -> bool {
        #[cfg(feature = "mods")]
        if self.mods.is_running() {
            return true;
        }
        false
    }
    /// Whether a mod draws a HUD panel or a corner panel.
    pub(super) fn has_panels(&self) -> bool {
        #[cfg(feature = "mods")]
        if self.mods.panels().next().is_some()
            || self.mods.corner_panels().next().is_some()
            || self.mods.canvas_panels().next().is_some()
        {
            return true;
        }
        false
    }
}
/// How often dev mods are checked for a rebuild.
#[cfg(feature = "mods")]
const RELOAD_CHECK: Duration = Duration::from_secs(1);

/// A vanilla command name: mods may not take it.
#[cfg(feature = "mods")]
pub(super) fn vanilla_command(name: &str) -> bool {
    let d = vanilla::game_dispatcher();
    d.nodes[0]
        .children
        .iter()
        .any(|&c| d.nodes[c].name() == name)
}

/// The mods in `horizon-mods/` and the dev mods as the Mods page lists them, read from their
/// manifests: what the startup menu shows before any mod runs (iOS opens on it).
pub(super) fn installed_on_disk(dev: &[std::path::PathBuf]) -> Vec<mod_list::Installed> {
    let dir = Game::game_dir().join(mod_list::DIR);
    let mut paths = horizon_mods::find(&dir);
    paths.retain(|p| !dev.contains(p));
    paths.extend(dev.iter().cloned());
    paths
        .iter()
        .map(|path| match horizon_mods::Package::read(path) {
            Ok(package) => {
                let m = package.manifest;
                mod_list::Installed {
                    title: format!("{} {}", m.name, m.version),
                    description: m.description,
                    id: m.id,
                    problem: None,
                    dev: dev.contains(path),
                    has_settings: false,
                    content: m
                        .capabilities
                        .contains(&horizon_mods::Capability::Registries),
                    native: None,
                }
            }
            Err(e) => mod_list::Installed {
                id: String::new(),
                title: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                description: String::new(),
                problem: Some(e),
                dev: false,
                has_settings: false,
                content: false,
                native: None,
            },
        })
        .collect()
}

/// Freeze native component IDs before reading content or opening a connection.
/// With no mod registering any, nothing is installed, which every reader
/// treats as no custom components, and `registries.json` isn't read.
pub(super) fn register_components(root: &std::path::Path) -> horizon_client::Result<()> {
    let mut names = Vec::new();
    Game::native_component_types(&mut names);
    if names.is_empty() {
        return Ok(());
    }
    let path =
        horizon_protocol::registry::Builtin::find(root).ok_or("registries.json not found")?;
    let builtin = horizon_protocol::registry::Builtin::load(&path)?;
    let types = builtin
        .get("minecraft:data_component_type")
        .ok_or("missing data component registry")?;
    let first = i32::try_from(types.names.len())?;
    horizon_protocol::item::custom::install(first, names)
        .map_err(|error| format!("invalid native component registration: {error}"))?;
    Ok(())
}

/// The blocks and items native mods declare in code (`native_blocks`,
/// `native_items`): asked once, so every content root of this run, a pack
/// reload's included, has the same ones.
fn native_declared() -> &'static (
    Vec<horizon_mods::content::Block>,
    Vec<horizon_mods::content::Item>,
) {
    static DECLARED: std::sync::OnceLock<(
        Vec<horizon_mods::content::Block>,
        Vec<horizon_mods::content::Item>,
    )> = std::sync::OnceLock::new();
    DECLARED.get_or_init(|| {
        let (mut blocks, mut items) = (Vec::new(), Vec::new());
        Game::native_blocks(&mut blocks);
        Game::native_items(&mut items);
        (blocks, items)
    })
}
/// The native mods' standalone models (`native_standalone_models`), set once
/// before the resources first load.
pub(super) fn register_standalone_models() {
    let mut names = Vec::new();
    Game::native_standalone_models(&mut names);
    horizon_client::models::register_standalone(names);
}

/// `root` with the content mods that are on layered over it
/// (docs/modpacks.md): their blocks, items, data and assets. Read once at
/// start, before the resources load; a mod whose content is refused is left
/// out here and its row on the Mods page says why.
pub(super) fn content_root(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
) -> std::path::PathBuf {
    use horizon_client::mod_content::{self, ContentMod};
    use horizon_mods::{Capability, Content, Package};
    // Before every atlas is built: whether it keeps the mods' image page.
    super::mod_canvas::note_canvas_mods(enabled, dev);
    let here = std::env::current_dir().unwrap_or_default();
    let mut paths = horizon_mods::find(&here.join(mod_list::DIR));
    paths.retain(|p| !dev.contains(p));
    paths.extend(dev.iter().cloned());
    let mut known = None;
    let mut mods: Vec<ContentMod> = Vec::new();
    // The first load's list stays (`set_local`): a reload does not hash the files again.
    let announce = !horizon_protocol::mod_list::is_set();
    let mut announced = Vec::new();
    for path in &paths {
        let Ok(package) = Package::read(path) else {
            continue;
        };
        let m = &package.manifest;
        let on = enabled.contains(&m.id) || dev.contains(path);
        if !on
            || !m.capabilities.contains(&Capability::Registries)
            || mods.iter().any(|c| c.id == m.id)
        {
            continue;
        }
        // The items this mod's native code declares may be a file item's crafting remainder.
        let declared: Vec<String> = native_declared()
            .1
            .iter()
            .filter(|i| i.id.split_once(':').is_some_and(|(ns, _)| ns == m.id))
            .map(|i| i.id.clone())
            .collect();
        let content = match Content::read_declaring(path, m, &declared) {
            Ok(content) => content,
            Err(e) => {
                log::warn!("Horizon: mod {} content not loaded: {e}", m.id);
                continue;
            }
        };
        let known = match &mut known {
            Some(types) => types,
            None => match mod_content::known_components(root) {
                Ok(types) => known.insert(types),
                Err(e) => {
                    log::warn!("Horizon: content mods not loaded: {e}");
                    register_mob_effects(&[]);
                    return root.to_owned();
                }
            },
        };
        let candidate = ContentMod {
            id: m.id.clone(),
            content,
        };
        match mod_content::accept_mod(&mut mods, candidate, known) {
            Ok(_) if announce => announced.push(announced_entry(m, path)),
            Ok(_) => {}
            Err(e) => log::warn!("Horizon: mod {} content not loaded: {e}", m.id),
        }
    }
    let (blocks, items) = native_declared();
    if !blocks.is_empty() || !items.is_empty() {
        if known.is_none() {
            match mod_content::known_components(root) {
                Ok(types) => known = Some(types),
                Err(e) => log::warn!("Horizon: native mods' blocks and items not loaded: {e}"),
            }
        }
        if let Some(known) = &known {
            for e in mod_content::accept_declared(&mut mods, blocks, items, known) {
                log::warn!("Horizon: {e}");
            }
            // A file item's remainder named a code-declared item that was refused: that mod
            // alone is left out, not every content mod through the overlay's check.
            for e in mod_content::drop_dangling_remainders(&mut mods) {
                log::warn!("Horizon: {e}");
            }
        }
    }
    match mod_content::resource_root(root, &mods, &here.join(".horizon-cache")) {
        Ok(out) => {
            if announce {
                horizon_protocol::mod_list::set_local(announced);
            }
            if out != root {
                log::info!(
                    "Horizon: content from {} mod(s) in {}",
                    mods.len(),
                    out.display()
                );
            }
            register_mob_effects(&mods);
            register_entity_types(&mods);
            register_menu_types(&mods);
            out
        }
        Err(e) => {
            log::warn!("Horizon: content mods not loaded: {e}");
            register_mob_effects(&[]);
            register_entity_types(&[]);
            register_menu_types(&[]);
            root.to_owned()
        }
    }
}

/// A loaded mod as a server tells joining clients (`horizon_protocol::mod_list`): its manifest's
/// id, version and side, and the SHA-256 of its `.hmod` (none for a folder).
fn announced_entry(
    m: &horizon_mods::Manifest,
    path: &std::path::Path,
) -> horizon_protocol::mod_list::Entry {
    use horizon_mods::Side;
    use horizon_protocol::mod_list;
    use sha2::{Digest, Sha256};
    let sha256 = if path.is_file() {
        std::fs::read(path)
            .ok()
            .map(|bytes| Sha256::digest(bytes).into())
    } else {
        None
    };
    mod_list::Entry {
        id: m.id.clone(),
        version: m.version.clone(),
        sha256,
        side: match m.side {
            Side::Both => mod_list::Side::Both,
            Side::Server => mod_list::Side::Server,
            Side::Client => mod_list::Side::Client,
        },
    }
}

/// The mods' mob effects join vanilla's (`brewing::effect`), once the root
/// holding their network ids is built. The startup root's effects, none
/// included, stay until restart: a pack reload after a Mods switch builds a
/// root with others.
fn register_mob_effects(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::brewing::{self, Category};
    use horizon_mods::content::EffectCategory;
    let effects = horizon_client::mod_content::mob_effects(mods);
    let ids: Vec<String> = effects.iter().map(|e| e.id.clone()).collect();
    let set = brewing::register_mod_effects(effects.into_iter().map(|e| {
        let category = match e.category {
            EffectCategory::Beneficial => Category::Beneficial,
            EffectCategory::Harmful => Category::Harmful,
            EffectCategory::Neutral => Category::Neutral,
        };
        (e.id.clone(), e.color, category)
    }));
    if !set
        && brewing::mod_effects()
            .iter()
            .map(|e| e.id)
            .ne(ids.iter().map(String::as_str))
    {
        log::warn!("Horizon: the mods' mob effects changed; restart the game to use them");
    }
}

/// the mods' thrown entity types, as their mob effects: set once, from the
/// startup root. Living types are mobs, loaded with vanilla's from the
/// content root's `entity-types.json`.
fn register_entity_types(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::projectiles;
    let mut types = horizon_client::mod_content::entity_types(mods);
    types.retain(|t| t.item.is_some());
    let set = projectiles::register_thrown_types(
        types
            .iter()
            .filter_map(|t| Some((t.id.clone(), t.item.clone()?, t.damage))),
    );
    if !set
        && projectiles::thrown_types()
            .iter()
            .map(|t| t.id)
            .ne(types.iter().map(|t| t.id.as_str()))
    {
        log::warn!("Horizon: the mods' entity types changed; restart the game to use them");
    }
}

/// the mods' menu types, as their entity types: set once, from the startup
/// root.
fn register_menu_types(mods: &[horizon_client::mod_content::ContentMod]) {
    use horizon_client::containers;
    let menus = horizon_client::mod_content::menu_types(mods);
    let set = containers::register_mod_menu_types(menus.iter().map(|m| {
        let results = m.results.iter().map(|&r| usize::from(r)).collect();
        (m.id.clone(), m.slots.into(), m.data_slots.into(), results)
    }));
    if !set
        && containers::mod_menu_types()
            .iter()
            .map(|m| m.id)
            .ne(menus.iter().map(|m| m.id.as_str()))
    {
        log::warn!("Horizon: the mods' menu types changed; restart the game to use them");
    }
}

/// What the launcher writes beside a game it built with native mods: their ids under `mods`.
const BUILD_INFO: &str = "native-mods.json";

/// `native-mods.json`'s `mods` list is not empty: the game was built with mods.
fn build_lists_mods(built: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(built)
        .ok()
        .and_then(|info| info["mods"].as_array().map(|mods| !mods.is_empty()))
        .unwrap_or(false)
}

/// Whether the game playing was built with native mods: the launcher writes their ids into
/// [`BUILD_INFO`] beside the executable, and a release or Nightly has no such file.
pub(super) fn built_with_native_mods() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read_to_string(exe.with_file_name(BUILD_INFO)).ok())
        .is_some_and(|built| build_lists_mods(&built))
}

/// The native mods the launcher built this game with, `id version, id version`, for the crash
/// report (`built`: its [`BUILD_INFO`]). None for a release or Nightly, which have no mods.
fn native_mods_detail(built: &str) -> Option<String> {
    let info: serde_json::Value = serde_json::from_str(built).unwrap_or_default();
    let list = info["mods"]
        .as_array()?
        .iter()
        .map(|m| {
            let text = |key: &str| m[key].as_str().unwrap_or("?").to_owned();
            format!("{} {}", text("id"), text("version"))
        })
        .collect::<Vec<_>>()
        .join(", ");
    (!list.is_empty()).then_some(list)
}

/// A row for each native mod the launcher installed in `dir` (`<id>.hnmod`) that this game wasn't
/// built with (`built`: its [`BUILD_INFO`], empty for a release or Nightly) and that has no row
/// yet. The page said "No mods" before, and players copied the files into `horizon-mods` (#4770).
/// Its toggle is the `mod.<id>` line the launcher's next build follows.
fn not_built(dir: &std::path::Path, built: &str, rows: &[Installed]) -> Vec<Installed> {
    let info: serde_json::Value = serde_json::from_str(built).unwrap_or_default();
    let in_build = |id: &str| {
        info["mods"]
            .as_array()
            .is_some_and(|mods| mods.iter().any(|m| m["id"] == id))
    };
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.retain(|p| p.extension().is_some_and(|e| e == "hnmod"));
    paths.sort();
    let mut out: Vec<Installed> = Vec::new();
    for path in paths {
        let Ok(bundle) = horizon_mods::index::native::Bundle::read(&path) else {
            continue;
        };
        let m = bundle.manifest;
        let listed = rows.iter().chain(&out).any(|row| row.id == m.id);
        if !listed && !in_build(&m.id) {
            out.push(Installed {
                title: format!("{} {}", m.id, m.version),
                id: m.id,
                description: String::new(),
                problem: Some(mod_list::NOT_BUILT.to_owned()),
                dev: false,
                has_settings: false,
                content: false,
                native: None,
            });
        }
    }
    out
}

impl Game<'_> {
    fn list_mod_build_info(&mut self) {
        // A mod with a native version built in says so instead, and isn't news.
        self.list_native_mods();
        let built = std::env::current_exe()
            .ok()
            .and_then(|exe| std::fs::read_to_string(exe.with_file_name(BUILD_INFO)).ok())
            .unwrap_or_default();
        let built_mods = native_mods_detail(&built);
        let built_with_mods = built_mods.is_some();
        if let Some(list) = built_mods {
            horizon_client::crash::set_detail("Native Mods", list);
        }
        horizon_client::gfx::native_render::set_mods_built_in(
            built_with_mods
                || self
                    .settings
                    .mods
                    .installed
                    .iter()
                    .any(|m| m.native.is_some()),
        );
        let native_dir = Self::game_dir().join(mod_list::NATIVE_DIR);
        let unbuilt = not_built(&native_dir, &built, &self.settings.mods.installed);
        self.settings.mods.installed.extend(unbuilt);
    }
}

/// A build without the `mods` feature: no mod runs.
#[cfg(not(feature = "mods"))]
impl Game<'_> {
    /// Says why the installed and dev mods don't run.
    pub(super) fn start_mods(&mut self) {
        let dir = Self::game_dir().join(mod_list::DIR);
        let found = horizon_mods::find(&dir).len() + self.mods.dev.len();
        if found > 0 {
            log::warn!(
                "Horizon: {found} mod(s) not started: this build has no mod runtime (build with --features mods)"
            );
        }
        self.settings.mods.installed = installed_on_disk(&self.mods.dev);
        self.list_mod_build_info();
    }
    pub(super) fn merge_remote_tree(&mut self) {}
    pub(super) fn sync_mods(&mut self) {}
    pub(super) fn tick_mods(&mut self) {}
    pub(super) fn mods_frame(&mut self, _elapsed: Duration) {}
    pub(super) fn mod_command(&mut self, _command: &str) -> bool {
        false
    }
    pub(super) fn mods_chat(&mut self, _line: &str) {}
    pub(super) fn draw_mod_panels(&mut self, _batch: &mut HudBatch, _corners: &mut Corners) {}
    pub(super) fn open_mod_screen(&mut self, _owner: usize, _settings: bool) {}
    pub(super) fn close_wasm_mod_screen(&mut self, _tell: bool) {}
    pub(super) fn draw_mod_screen(&mut self, _batch: &mut HudBatch) {}
    pub(super) fn mod_screen_click(&mut self, _button: winit::event::MouseButton) {}
    pub(super) fn mod_screen_key(&mut self, _key: winit::keyboard::KeyCode) -> bool {
        false
    }
    pub(super) fn mod_screen_scroll(&mut self, _delta: f32) {}
    pub(super) fn mod_screen_drag(&mut self) {}
    pub(super) fn mod_screen_release(&mut self) {}
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// Reads `horizon-mods/` and the dev mods, and starts the ones turned on.
    pub(super) fn start_mods(&mut self) {
        let dir = Self::game_dir().join(mod_list::DIR);
        let dev = &self.mods.dev;
        let mut paths = horizon_mods::find(&dir);
        paths.retain(|p| !dev.contains(p));
        paths.extend(dev.iter().cloned());
        // A dev mod is on whatever the list says.
        let dev_ids: Vec<String> = dev
            .iter()
            .filter_map(|p| horizon_mods::Package::read(p).ok())
            .map(|p| p.manifest.id)
            .collect();
        // A native implementation built into this game takes over its own id.
        let mut native = Vec::new();
        self.native_mods(&mut native);
        let enabled = self.settings.mods.enabled.clone();
        let on = |id: &str| {
            (enabled.contains(id) || dev_ids.iter().any(|d| d == id))
                && !native.iter().any(|m| m.id == id)
        };
        self.return_client_world();
        self.mods.mods = Mods::default();
        // The mods' `init` may read the world (capability `world-read`).
        self.lend_client_world(true);
        let runs = self.mod_runs();
        self.mods.mods.load_into(runs, &paths, on, vanilla_command);
        self.return_client_world();
        self.forget_client_world();
        self.ensure_mod_canvas_page();
        self.mods.interactions.clear();
        self.mods.item_uses.clear();
        self.mods.use_fallbacks.clear();
        self.mods.attacks.clear();
        self.mods.last_item_uses = Default::default();
        self.mods.last_attacks = Default::default();
        let chosen = &self.settings.mods;
        self.mods.mods.set_bindings(|id| chosen.keys_of(id));
        // Entries were replaced wholesale: a screen's owner index is stale.
        self.mods.screen = None;
        self.mods.canvas_screen = None;
        // Content mods may have changed the items: remade on the next read.
        self.mods.recipes = None;
        self.list_mods();
        self.sync_mod_commands();
        self.show_mod_output();
    }
    /// Which mods this game runs: a server the server mods, the window
    /// client mods, and both when it simulates the world itself (in-frame).
    fn mod_runs(&self) -> Runs {
        match self.options.execution_role {
            ExecutionRole::IntegratedServer | ExecutionRole::DedicatedServer => Runs::Server,
            ExecutionRole::Client => Runs::Client,
        }
    }
    /// The Mods page's rows, from what loaded.
    fn list_mods(&mut self) {
        self.mods.server = self.mods.mods.runs_server();
        self.mods.world = self.mods.mods.wants_world();
        self.mods.entities = self.mods.mods.wants_entities();
        let dev = &self.mods.dev;
        self.settings.mods.installed = self
            .mods
            .mods
            .entries()
            .iter()
            .map(|e| {
                let file = e.path.file_name().unwrap_or_default().to_string_lossy();
                let problem = match &e.status {
                    Status::Failed(reason) => Some(format!("Stopped: {reason}")),
                    _ => None,
                };
                let Some(m) = &e.manifest else {
                    return Installed {
                        id: String::new(),
                        title: file.into_owned(),
                        description: String::new(),
                        problem,
                        dev: false,
                        has_settings: false,
                        content: false,
                        native: None,
                    };
                };
                // What it may do first: the row cuts long descriptions.
                let may: Vec<String> = m
                    .capabilities
                    .iter()
                    .map(|c| c.describe_with_storage(m.storage_mb))
                    .collect();
                let may = if may.is_empty() {
                    "Asks for nothing.".to_owned()
                } else {
                    format!("May {}.", may.join(", "))
                };
                Installed {
                    id: m.id.clone(),
                    title: format!("{} {}", m.name, m.version),
                    description: format!("{may} {}", m.description).trim().to_owned(),
                    problem,
                    dev: dev.contains(&e.path),
                    has_settings: e.has_ui_settings(),
                    content: m
                        .capabilities
                        .contains(&horizon_mods::Capability::Registries),
                    native: None,
                }
            })
            .collect();
        self.list_mod_build_info();
    }
    /// The chat's tree after the running mods changed.
    fn sync_mod_commands(&mut self) {
        // In-frame, the window runs server mods too: their names, free text
        // here (the server parses their arguments).
        let server = self.mods.mods.server_commands().map(|c| c.name.as_str());
        let mut names = self.mods.mods.commands().chain(server).peekable();
        self.mods.tree = names
            .peek()
            .is_some()
            .then(|| vanilla::game_dispatcher().with_mod_commands(names));
        self.merge_remote_tree();
    }

    /// The server's command tree with the mods' commands, for the chat.
    pub(super) fn merge_remote_tree(&mut self) {
        let names: Vec<String> = self.mods.mods.commands().map(str::to_owned).collect();
        self.mods.remote_tree = self
            .remote
            .as_ref()
            .map(|r| &r.chat_tree)
            .filter(|_| !names.is_empty())
            .map(|t| t.with_mod_commands(names.iter().map(String::as_str)));
    }

    /// After the options changed: restart the mods if the list of the ones
    /// turned on did.
    pub(super) fn sync_mods(&mut self) {
        let running: Vec<&str> = self
            .mods
            .mods
            .entries()
            .iter()
            .filter(|e| e.status != Status::Off)
            .filter_map(|e| Some(e.manifest.as_ref()?.id.as_str()))
            .collect();
        let wanted: Vec<&str> = self
            .settings
            .mods
            .installed
            .iter()
            .enumerate()
            .filter(|&(i, m)| {
                self.settings.mods.is_on(i)
                    && m.native.is_none()
                    && self
                        .mods
                        .mods
                        .entries()
                        .iter()
                        .any(|e| e.manifest.as_ref().is_some_and(|entry| entry.id == m.id))
            })
            .map(|(_, m)| m.id.as_str())
            .collect();
        if running != wanted {
            self.start_mods();
        }
    }
    /// What mods read this tick (or frame, for mods with a `frame` hook): the
    /// server's players as they were at the last tick's end.
    fn mod_snapshot(&mut self) -> Snapshot {
        let players = if self.mods.server {
            self.mods.players.clone()
        } else {
            Vec::new()
        };
        self.mod_snapshot_with(players)
    }
    /// What mods read, with these players on the server.
    pub(super) fn mod_snapshot_with(&mut self, players: Vec<ServerPlayer>) -> Snapshot {
        let player = self.player.as_ref().map(|p| Player {
            x: p.position.x,
            y: p.position.y,
            z: p.position.z,
            yaw: self.yaw,
            pitch: self.pitch,
            health: p.health.health(),
            food: p.health.food.food.max(0) as u32,
            dimension: self.dim.id().to_owned(),
        });
        let clock = player.is_some().then(|| Clock {
            game_time: self.world.game_time.max(0) as u64,
            day_time: self.world.day_time.max(0) as u64,
        });
        let (cps, mouse) = self.mouse_reading();
        // The movement and click mappings, as the keystroke overlay reads them:
        // keys by the default key their binding is translated to.
        let mappings = MAPPINGS
            .iter()
            .map(|&(name, key)| {
                let label = keybinds::display_name(self.settings.keys.get(name)).plain();
                let down = match key {
                    Some(k) => self.keys.contains(&k),
                    None => mouse[usize::from(name == "key.use")],
                };
                (name, label, down)
            })
            .collect();
        if self.mods.font_widths.is_empty() {
            let atlas = &self.gui.atlas;
            self.mods.font_widths = (0..FONT_CHARS)
                .map(|c| char::from_u32(c).map_or(6.0, |c| atlas.glyph_cell(c).advance))
                .collect();
        }
        let (recipes, known_recipes) = self.mod_recipes();
        Snapshot {
            tick: self.ticks,
            touch_active: self.mod_touch_active(),
            player,
            clock,
            gui_size: self.gui_size(),
            held: self.wasm_held(),
            frame: self.frame_reading(),
            mappings,
            cps,
            font_widths: self.mods.font_widths.clone(),
            players,
            game_rules: if self.mods.world {
                horizon_client::gamerules::RULES
                    .iter()
                    .filter_map(|r| {
                        Some((r.name.to_string(), self.ui.rules.get(r.name)?.to_string()))
                    })
                    .collect()
            } else {
                Default::default()
            },
            difficulty: horizon_client::title::DIFFICULTIES[usize::from(self.ui.difficulty) & 3]
                .into(),
            world_times: if self.mods.world {
                [
                    "minecraft:overworld",
                    "minecraft:the_nether",
                    "minecraft:the_end",
                ]
                .into_iter()
                .map(|dim| {
                    (
                        dim.to_owned(),
                        horizon_client::mod_world::time(&self.world, self.res.environment(), dim),
                    )
                })
                .collect()
            } else {
                Default::default()
            },
            entities: Default::default(),
            mob_kinds: Default::default(),
            vehicles: Default::default(),
            tamable_mobs: Default::default(),
            entity_resources: Default::default(),
            interactions: Default::default(),
            item_uses: self.mods.last_item_uses.clone(),
            attacks: self.mods.last_attacks.clone(),
            hurt_events: self.mods.last_hurt_events.clone(),
            deaths: self.mods.last_deaths.clone(),
            player_damage: self.mods.last_player_damage.clone(),
            effect_ids: horizon_client::brewing::EFFECTS
                .iter()
                .map(|e| format!("minecraft:{}", e.id))
                .chain(
                    horizon_client::brewing::mod_effects()
                        .iter()
                        .map(|e| e.id.to_owned()),
                )
                .collect::<std::collections::HashSet<_>>()
                .into(),
            player_looks: Default::default(),
            player_permissions: Default::default(),
            teleport_borders: Default::default(),
            world_spawn: None,
            generated_chunks: Default::default(),
            view_hit: self.mod_view_hit(),
            view_entity: self.mod_view_entity(),
            recipes,
            known_recipes,
            placing: self.mod_recipe_screen(),
            creative: self.creative,
            world_id: self.mod_world_id(),
        }
    }
    /// The first block along the player's view within
    /// [`horizon_mods::RAYCAST_MAX`] and its distance from the eye
    /// (`game.raycast`): through the loaded chunks as the crosshair's ray
    /// goes (fluids pass), then across the distant terrain's ground heights,
    /// a block at a time.
    fn mod_view_hit(&self) -> Option<(f32, horizon_mods::Hit)> {
        if !self.mods.mods.wants_raycast() {
            return None;
        }
        let (eye, dir) = self.aim()?;
        let reach = f64::from(horizon_mods::RAYCAST_MAX);
        let (pos, point, state) = match self.clip_ray(eye, dir, reach) {
            Some(hit) => (
                hit.pos,
                hit.point,
                self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]),
            ),
            None => self.wasm_lod_ray(eye, dir, reach)?,
        };
        let distance = (0..3).map(|i| (point[i] - eye[i]).powi(2)).sum::<f64>();
        let hit = horizon_mods::Hit {
            block_x: pos[0],
            block_y: pos[1],
            block_z: pos[2],
            x: point[0],
            y: point[1],
            z: point[2],
            block: self.res.facts().get(state).name.to_string(),
        };
        Some((distance.sqrt() as f32, hit))
    }
    /// Past the resident chunks: the view walked across the distant
    /// terrain's ground heights (`Lod::ground`), a block at a time. Columns in
    /// loaded chunks are left to `clip_ray`, which has already missed them.
    fn wasm_lod_ray(
        &self,
        eye: [f64; 3],
        dir: [f64; 3],
        reach: f64,
    ) -> Option<([i32; 3], [f64; 3], horizon_world::registry::BlockStateId)> {
        let lod = self.lod.as_ref()?;
        let mut t = 0.0;
        while t <= reach {
            let p = [0, 1, 2].map(|i| eye[i] + dir[i] * t);
            let (x, z) = (p[0].floor() as i32, p[2].floor() as i32);
            if !self.world.loaded(x, z)
                && let Some((height, state)) = lod.ground(x, z)
                && p[1] <= f64::from(height)
            {
                return Some(([x, height - 1, z], [p[0], f64::from(height), p[2]], state));
            }
            t += 1.0;
        }
        None
    }
    /// A mod's `command.run-quiet` lines (vanilla's commands only, as
    /// `command.run`): to a server that runs them quietly as one payload
    /// (`horizon_protocol::quiet`), to any other as chat commands, or for this
    /// window's own world next tick.
    fn mod_quiet_commands(&mut self, mod_id: String, lines: Vec<String>) {
        let lines: Vec<String> = lines
            .into_iter()
            .filter(|l| vanilla_command(l.split(' ').next().unwrap_or_default()))
            .collect();
        match self.remote.as_ref().map(|r| r.quiet_commands) {
            Some(true) => {
                for data in horizon_protocol::quiet::encode(&mod_id, &lines) {
                    self.send(horizon_protocol::packets::play::Serverbound::CustomPayload(
                        horizon_protocol::packets::configuration::CustomPayload {
                            channel: horizon_protocol::quiet::CHANNEL.into(),
                            data,
                        },
                    ));
                }
            }
            // The server checks the player's permissions; its feedback shows.
            Some(false) => {
                for line in lines {
                    self.send_chat(&format!("/{line}"));
                }
            }
            None => self.mods.quiet.push((mod_id, lines)),
        }
    }
    /// Server-mod commands use their own negotiated channel, with no chat fallback.
    fn mod_server_commands_quiet(&mut self, mod_id: String, lines: Vec<String>) {
        let lines: Vec<_> = lines
            .into_iter()
            .filter(|l| !vanilla_command(l.split(' ').next().unwrap_or_default()))
            .collect();
        match self.remote.as_ref().map(|r| r.mod_quiet_commands) {
            Some(true) => {
                for data in horizon_protocol::quiet::encode(&mod_id, &lines) {
                    self.send(horizon_protocol::packets::play::Serverbound::CustomPayload(
                        horizon_protocol::packets::configuration::CustomPayload {
                            channel: horizon_protocol::quiet::MOD_CHANNEL.into(),
                            data,
                        },
                    ));
                }
            }
            Some(false) => {} // No chat fallback: older/non-Horizon servers ignore this API.
            None => self.mods.mod_quiet.push((mod_id, lines)),
        }
    }
    /// A quiet vanilla batch in this window's own world, as a server runs one.
    fn run_quiet_batch(&mut self, mod_id: &str, lines: &[String]) {
        let permission = self.ui.registries.permission;
        let mut failed = 0;
        for line in lines {
            let ok = self.signable_arguments(permission, line).is_empty()
                && self.run_command_quietly(line);
            failed += usize::from(!ok);
        }
        log::info!(
            "[mod {mod_id}] ran {} quiet command(s): {failed} failed",
            lines.len()
        );
    }
    /// A game tick.
    pub(super) fn tick_mods(&mut self) {
        // Here, at the player's level: shown inside a console call (a control
        // socket command set the hook off), they would run as the console.
        for line in std::mem::take(&mut self.mods.commands) {
            self.run_command(&line);
        }
        for (mod_id, lines) in std::mem::take(&mut self.mods.quiet) {
            self.run_quiet_batch(&mod_id, &lines);
        }
        for (_, lines) in std::mem::take(&mut self.mods.mod_quiet) {
            for line in lines {
                self.local_server_mod_command_quiet(&line);
            }
        }
        if !self.mods.mods.is_running() {
            return;
        }
        self.tick_wasm_mod_use();
        let snapshot = self.mod_snapshot();
        self.mods.mods.set_snapshot(snapshot);
        self.lend_client_world(false);
        let ticks = self.ticks;
        self.with_mod_container(|mods| mods.tick(ticks));
        self.return_client_world();
        self.show_mod_output();
    }
    /// Restarts the dev mods rebuilt since the last check, once a second.
    pub(super) fn reload_dev_mods(&mut self) {
        if self.mods.dev.is_empty()
            || self
                .mods
                .checked
                .is_some_and(|t| t.elapsed() < RELOAD_CHECK)
        {
            return;
        }
        self.mods.checked = Some(Instant::now());
        self.lend_client_world(true);
        let reloaded = self.with_mod_container(|mods| mods.reload_changed(vanilla_command));
        self.return_client_world();
        self.forget_client_world();
        if reloaded {
            let chosen = &self.settings.mods;
            self.mods.mods.set_bindings(|id| chosen.keys_of(id));
            self.list_mods();
            self.sync_mod_commands();
            if self.options.execution_role.is_client() {
                self.system_message(Text::literal("Mods reloaded").with_color(text::GRAY));
            } else {
                log::info!("Horizon: mods reloaded");
            }
        }
    }
    /// Once per frame: the frame hook, and dev mods' rebuilds.
    pub(super) fn mods_frame(&mut self, elapsed: Duration) {
        self.reload_dev_mods();
        if !self.mods.mods.is_running() {
            return;
        }
        self.mod_input_frame();
        if self.mods.mods.wants_frames() {
            let snapshot = self.mod_snapshot();
            self.mods.mods.set_snapshot(snapshot);
        }
        self.lend_client_world(false);
        self.mod_container_lifecycle();
        self.with_mod_container(|mods| mods.frame(elapsed.as_secs_f32()));
        self.return_client_world();
        self.upload_mod_canvas();
        self.show_mod_output();
    }
    /// `/command` typed in chat: true when a mod owns it.
    pub(super) fn mod_command(&mut self, command: &str) -> bool {
        if !self.mods.mods.is_running() {
            return false;
        }
        // The command may follow a teleport in the same tick.
        let snapshot = self.mod_snapshot();
        self.mods.mods.set_snapshot(snapshot);
        self.lend_client_world(false);
        let owned = self.with_mod_container(|mods| mods.command(command));
        self.return_client_world();
        if !owned {
            return false;
        }
        self.show_mod_output();
        true
    }
    /// A chat line the player received.
    pub(super) fn mods_chat(&mut self, line: &str) {
        self.lend_client_world(false);
        self.with_mod_container(|mods| mods.chat(line));
        self.return_client_world();
        self.show_mod_output();
    }
    /// What mods showed: client mods' lines into the chat, server mods'
    /// to their players.
    pub(super) fn show_mod_output(&mut self) {
        self.route_mod_output(None);
    }
    /// [`Game::show_mod_output`], with the hub when it is out of `self.lan`.
    pub(super) fn route_mod_output(&mut self, mut host: Option<&mut LanHost>) {
        for output in self.mods.mods.drain() {
            let line = match output {
                Output::Chat { name, text } => {
                    let mut line = Text::literal(format!("[{name}] ")).with_color(text::GRAY);
                    line.push(Text::literal(text));
                    line
                }
                Output::Feedback { text, error: false } => Text::literal(text),
                Output::Feedback { text, error: true } => Text::literal(text).with_color(text::RED),
                // Vanilla's commands only: a mod running a mod command could loop.
                Output::Command { line } => {
                    let name = line.split(' ').next().unwrap_or_default();
                    if !vanilla_command(name) {
                        // Dropped.
                    } else if self.remote.is_some() {
                        // The server checks the player's permissions.
                        self.send_chat(&format!("/{line}"));
                    } else {
                        self.mods.commands.push(line);
                    }
                    continue;
                }
                Output::ModQuietCommands { mod_id, lines } => {
                    self.mod_server_commands_quiet(mod_id, lines);
                    continue;
                }
                Output::QuietCommands { mod_id, lines } => {
                    self.mod_quiet_commands(mod_id, lines);
                    continue;
                }
                Output::Stopped { name, reason } => {
                    // Its screen goes with it: the entry's run is gone, so
                    // `mod_ui_view` finds no widgets and the screen closes.
                    self.list_mods();
                    self.sync_mod_commands();
                    let line = format!("Mod {name} stopped: {reason}");
                    if self.options.execution_role == ExecutionRole::DedicatedServer {
                        super::dedicated::log("WARN", &line);
                    }
                    Text::literal(line).with_color(text::RED)
                }
                server @ (Output::Tell { .. }
                | Output::Broadcast { .. }
                | Output::Reply { .. }
                | Output::SetBlocks { .. }
                | Output::BreakBlocksAs { .. }
                | Output::SetLootTable { .. }
                | Output::SetContainer { .. }
                | Output::PlaceStructure { .. }
                | Output::SetSpawner { .. }
                | Output::Tame { .. }
                | Output::DamageItem { .. }
                | Output::Strike { .. }
                | Output::PlayerKnockback { .. }
                | Output::PlayerPush { .. }
                | Output::PlayerTeleport { .. }
                | Output::PlaySound { .. }
                | Output::MobEffect { .. }
                | Output::MobPush { .. }
                | Output::PlayerHeal { .. }
                | Output::SpawnItem { .. }
                | Output::Ride { .. }
                | Output::PlayerEffect { .. }
                | Output::Orders { .. }
                | Output::Spawn { .. }) => {
                    self.route_server_output(host.as_deref_mut(), server);
                    continue;
                }
                Output::OpenScreen { owner } => {
                    self.open_mod_screen(owner, false);
                    continue;
                }
                Output::OpenChat { owner, text } => {
                    self.open_mod_chat(owner, &text);
                    continue;
                }
                Output::Shake { strength, seconds } => {
                    self.mods.shake.add(strength, seconds);
                    continue;
                }
                Output::PlaceRecipe { book_id, all } => {
                    self.mod_place_recipe(book_id, all);
                    continue;
                }
                Output::Give {
                    item,
                    count,
                    components,
                } => {
                    self.mod_give(&item, count, &components);
                    continue;
                }
                Output::CloseScreen { owner } => {
                    if self.mods.screen.as_ref().is_some_and(|s| s.owner == owner)
                        || self
                            .mods
                            .canvas_screen
                            .as_ref()
                            .is_some_and(|s| s.owner == owner)
                    {
                        self.close_wasm_mod_screen(false);
                    }
                    continue;
                }
            };
            self.add_mod_chat_line(line);
        }
        // A mod may set or clear its settings page in any call, not only
        // while loading: its row's Settings button follows.
        let rows = self.settings.mods.installed.iter_mut();
        for (row, e) in rows.zip(self.mods.mods.entries()) {
            row.has_settings = e.has_ui_settings();
        }
    }
    /// An explicit prefill, using the normal chat screen and its automatic touch keyboard.
    /// Check first so ignored requests leave the mod's screen and any chat draft alone.
    fn open_mod_chat(&mut self, owner: usize, text: &str) {
        if self.player.is_none() || self.title.is_some() || !self.chat_abilities().send_commands {
            return;
        }
        if self.mods.screen.as_ref().is_some_and(|s| s.owner == owner)
            || self
                .mods
                .canvas_screen
                .as_ref()
                .is_some_and(|s| s.owner == owner)
        {
            self.close_wasm_mod_screen(false);
        }
        self.open_chat(text);
    }
    /// A mod's line in the window's chat; not through system_message: a mod
    /// must not hear its own lines.
    /// Written by this client, so the Chat option doesn't hide it.
    pub(super) fn add_mod_chat_line(&mut self, line: Text) {
        let focused = self.screen == Screen::Chat;
        self.ui.chat.add(
            &self.gui.atlas,
            line,
            None,
            horizon_client::chat::ChatSource::Client,
            self.ticks,
            focused,
        );
    }
    /// The mods' HUD panels, stacked with the extras in their corners.
    pub(super) fn draw_mod_panels(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        for panel in self.mods.mods.panels() {
            let corner = match panel.corner {
                horizon_mods::Corner::TopLeft => Corner::TopLeft,
                horizon_mods::Corner::TopRight => Corner::TopRight,
                horizon_mods::Corner::BottomLeft => Corner::BottomLeft,
                horizon_mods::Corner::BottomRight => Corner::BottomRight,
            };
            // F3 takes the top left, as it does from the extras.
            if self.debug && corner == Corner::TopLeft {
                continue;
            }
            let lines: Vec<(&str, u32)> = panel
                .lines
                .iter()
                .map(|l| (l.text.as_str(), l.color))
                .collect();
            extras_hud::draw_lines(batch, &self.gui.atlas, corners, corner, &lines);
        }
        // Capability `render`'s corner panels, placed the same way.
        for panel in self.mods.mods.corner_panels() {
            let corner = match panel.corner {
                horizon_mods::RenderCorner::TopLeft => Corner::TopLeft,
                horizon_mods::RenderCorner::TopRight => Corner::TopRight,
                horizon_mods::RenderCorner::BottomLeft => Corner::BottomLeft,
                horizon_mods::RenderCorner::BottomRight => Corner::BottomRight,
            };
            if self.debug && corner == Corner::TopLeft {
                continue;
            }
            let at = corners.place(corner, panel.width, panel.height);
            self.draw_shapes(batch, &panel.shapes, at);
        }
        // Capability `render`'s canvas boxes, after the corner panels.
        self.draw_canvas_panels(batch, corners);
    }

    /// A `ui` screen's title and widgets as the mod last pushed them; None
    /// when the surface is gone (the mod stopped or closed it). Borrows
    /// `mods` only, so `self.mods.screen` may stay borrowed for the scrolls.
    fn mod_ui_view<'a>(mods: &'a Mods, ms: &ModScreen) -> Option<(&'a str, &'a [UiWidget])> {
        if ms.settings {
            let widgets = mods.settings_widgets(ms.owner);
            if widgets.is_empty() && mods.keys_of(ms.owner).is_empty() {
                return None;
            }
            let name = mods
                .entries()
                .get(ms.owner)
                .and_then(|e| e.manifest.as_ref())
                .map_or("Mod", |m| m.name.as_str());
            Some((name, widgets))
        } else {
            let s: &UiScreen = mods.screen(ms.owner)?;
            Some((s.title.as_str(), s.widgets.as_slice()))
        }
    }
    /// Rows under a settings page's key rows: Reset Keys, then Edit Touch
    /// Layout where the touch controls can show.
    fn mod_page_extra(keys: usize) -> usize {
        if keys == 0 {
            0
        } else {
            1 + usize::from(super::touch::enabled())
        }
    }
    /// The keys a settings page lists first: the mod's own (`input`).
    fn mod_page_keys<'a>(mods: &'a Mods, ms: &ModScreen) -> &'a [ModKey] {
        if ms.settings {
            mods.keys_of(ms.owner)
        } else {
            &[]
        }
    }
    /// A key row's label as Key Binds shows one, "Zoom: Z", marked
    /// "> Zoom: Z <" in yellow while it waits for the new key.
    fn mod_key_label(k: &ModKey, waiting: bool) -> Text {
        let value = format!("{}: {}", k.label, keybinds::display_name(&k.key).plain());
        if waiting {
            Text::literal(format!("> {value} <")).with_color(text::YELLOW)
        } else {
            Text::literal(value)
        }
    }
    /// A running mod's id, by its index.
    fn mod_id(&self, owner: usize) -> Option<String> {
        self.mods
            .mods
            .entries()
            .get(owner)
            .and_then(|e| e.manifest.as_ref())
            .map(|m| m.id.clone())
    }
    /// Puts one of a mod's keys (or all of them) back on the key the mod
    /// registered, and forgets the player's choice (`horizon-mods.txt`).
    fn reset_mod_keys(&mut self, owner: usize, only: Option<&str>) {
        let Some(id) = self.mod_id(owner) else {
            return;
        };
        let names: Vec<String> = self
            .mods
            .mods
            .keys_of(owner)
            .iter()
            .map(|k| k.name.clone())
            .filter(|n| only.is_none_or(|o| o == n))
            .collect();
        for name in &names {
            self.mods.mods.reset_key(owner, name);
            if let Some(chosen) = self.settings.mods.keys.get_mut(&id) {
                chosen.remove(name);
            }
        }
        self.settings.save(&Self::options_path());
    }
    /// The player bound the key row that waited to `key`: the mod's key
    /// follows it now and at the next start (`horizon-mods.txt`).
    fn bind_mod_key(&mut self, key: String) {
        let Some(ms) = self.mods.screen.as_mut() else {
            return;
        };
        let Some(name) = ms.binding.take() else {
            return;
        };
        let owner = ms.owner;
        if !self.mods.mods.rebind(owner, &name, &key) {
            return;
        }
        let Some(id) = self
            .mods
            .mods
            .entries()
            .get(owner)
            .and_then(|e| e.manifest.as_ref())
            .map(|m| m.id.clone())
        else {
            return;
        };
        self.settings
            .mods
            .keys
            .entry(id)
            .or_default()
            .insert(name, key);
        self.settings.save(&Self::options_path());
    }
    /// A key while a mod screen is up: true when a key row waited for
    /// it (Key Binds: Escape binds none).
    pub(super) fn mod_screen_key(&mut self, key: KeyCode) -> bool {
        if self
            .mods
            .screen
            .as_ref()
            .is_none_or(|ms| ms.binding.is_none())
        {
            return false;
        }
        let value = match key {
            KeyCode::Escape => None,
            k => keybinds::key_name(k),
        };
        self.bind_mod_key(value.unwrap_or("key.keyboard.unknown").to_owned());
        true
    }
    /// The layout the mod screen's widgets get: (top,
    /// height), the column being `COLUMN` wide down the middle.
    fn mod_ui_layout(top: f32, widgets: &[UiWidget]) -> Vec<(f32, f32)> {
        let mut out = Vec::with_capacity(widgets.len());
        let mut y = top;
        for w in widgets {
            let wh = match w {
                UiWidget::Text(_) => 12.0,
                UiWidget::Item(_) | UiWidget::Button(_) => 20.0,
                UiWidget::List(l) => (l.rows.len().min(6) as f32 * 12.0) + 8.0,
            };
            out.push((y, wh));
            y += wh + 6.0;
        }
        out
    }
    /// A mod pushes its screen (`ui.open-screen`), or Options > Mods opens
    /// its settings page (`settings`, the mod's `installed` index).
    pub(super) fn open_mod_screen(&mut self, owner: usize, settings: bool) {
        if self.native_mod_screen.is_some() || (self.title.is_some() && !settings) {
            return;
        }
        if !settings && self.mods.mods.canvas_screen(owner).is_some() {
            let under = self.mods.screen.as_ref().map_or(self.screen, |s| s.under);
            self.mods.screen = None;
            self.open_mod_canvas_screen(owner, under);
            return;
        }
        if let Some(canvas) = self.mods.canvas_screen.take() {
            self.screen = canvas.under;
        }
        if settings {
            self.mods.mods.ui_settings_opened(owner);
            let keys = self.mods.mods.keys_of(owner);
            if self.mods.mods.settings_widgets(owner).is_empty() && keys.is_empty() {
                self.show_mod_output();
                return;
            }
        }
        self.mods.screen = Some(ModScreen {
            owner,
            settings,
            under: self.screen,
            scrolls: Vec::new(),
            scroll: ui::Scroll::default(),
            binding: None,
        });
        self.screen = Screen::Mod;
        if self.player.is_some() {
            self.capture(false);
        }
        self.show_mod_output();
    }
    /// Leaves the mod screen: back to Options for a settings page, to what
    /// it covered for a mod's own. `tell` is Esc/Done, heard as `closed`.
    pub(super) fn close_wasm_mod_screen(&mut self, tell: bool) {
        if self.close_mod_canvas_screen(tell) {
            return;
        }
        let Some(ms) = self.mods.screen.take() else {
            return;
        };
        if ms.settings {
            self.open_options_at(self.options_from_title, Page::Mods);
        } else {
            self.screen = ms.under;
            if self.screen == Screen::Playing && self.player.is_some() {
                self.capture(true);
            }
        }
        if tell {
            self.mods.mods.ui_closed(ms.owner);
            self.show_mod_output();
        }
    }
    /// The mod's screen or settings page: its title over widgets stacked
    /// down the middle, a Done button, Esc to leave (vanilla's menus).
    pub(super) fn draw_mod_screen(&mut self, batch: &mut HudBatch) {
        if self.draw_mod_canvas_screen(batch) {
            return;
        }
        let touch = self.mod_touch_active();
        let Some(ms) = self.mods.screen.as_mut() else {
            // The screen was dropped while it was up: back to the world,
            // mouse held again like any return to Playing.
            self.screen = Screen::Playing;
            if self.player.is_some() {
                self.capture(true);
            }
            return;
        };
        let Some((title, widgets)) = Self::mod_ui_view(&self.mods.mods, ms) else {
            // The mod's surface is gone (it stopped or cleared it): leave
            // the screen like Done did, without `closed` — the mod ended it.
            self.close_wasm_mod_screen(false);
            return;
        };
        // Item icons draw in a second pass: `draw_item` needs `&mut self`,
        // so `atlas` may not stay borrowed across it.
        let mut items: Vec<(String, f32, f32)> = Vec::new();
        let atlas = &self.gui.atlas;
        let in_world = self.player.is_some();
        ui::menu_background(batch, atlas, 0.0, 0.0, batch.width, batch.height, in_world);
        ui::centered_text(
            batch,
            atlas,
            &Text::literal(title),
            0.0,
            15.0,
            batch.width,
            u32::MAX,
        );
        let cx = (batch.width / 2.0).floor();
        let x = cx - MOD_UI_COLUMN / 2.0;
        // A settings page starts with the mod's keys, as Key Binds rows.
        let keys = Self::mod_page_keys(&self.mods.mods, ms);
        let page = ModPageLayout::new(
            batch.height,
            keys.len() + Self::mod_page_extra(keys.len()),
            widgets,
            &mut ms.scroll,
        );
        let cursor = page.cursor(self.cursor);
        let start = batch.vertices.len();
        let widgets_mark = horizon_client::widgets::widgets_mark();
        let targets_mark = horizon_client::widgets::targets_mark();
        let owner = ms.owner;
        let mod_id = self
            .mods
            .mods
            .entries()
            .get(owner)
            .and_then(|e| e.manifest.as_ref())
            .map_or(String::new(), |m| m.id.clone());
        for (i, k) in keys.iter().enumerate() {
            let wy = page.key_y(i);
            let hover = ui::hovered(cursor, x, wy, MOD_UI_COLUMN, 20.0);
            let waiting = ms.binding.as_deref() == Some(k.name.as_str());
            // On the touch screen a row shows and hides the key's on-screen
            // button (a tap would only bind the left mouse button).
            let label = if touch {
                let shown = self.touch.mod_key_shown(&self.mods.mods, owner, &mod_id, k);
                let key = if shown {
                    "horizon.touch.key_button_on"
                } else {
                    "horizon.touch.key_button_off"
                };
                Text::tr(key, &[&k.label])
            } else {
                Self::mod_key_label(k, waiting)
            };
            ui::button(
                batch,
                atlas,
                x,
                wy,
                MOD_UI_COLUMN,
                20.0,
                &label,
                true,
                hover,
            );
        }
        for j in 0..Self::mod_page_extra(keys.len()) {
            let wy = page.key_y(keys.len() + j);
            let hover = ui::hovered(cursor, x, wy, MOD_UI_COLUMN, 20.0);
            let label = Text::tr(["horizon.mods.reset_keys", "horizon.touch.edit"][j], &[]);
            ui::button(
                batch,
                atlas,
                x,
                wy,
                MOD_UI_COLUMN,
                20.0,
                &label,
                true,
                hover,
            );
        }
        let mut list_i = 0usize;
        for (w, &(wy, wh)) in widgets.iter().zip(&page.widgets) {
            match w {
                UiWidget::Button(b) => {
                    let hover = ui::hovered(cursor, x, wy, MOD_UI_COLUMN, 20.0);
                    let label = Text::literal(b.label.clone());
                    ui::button(
                        batch,
                        atlas,
                        x,
                        wy,
                        MOD_UI_COLUMN,
                        20.0,
                        &label,
                        true,
                        hover,
                    );
                }
                UiWidget::Text(t) => {
                    let color = if t.color == 0 {
                        u32::MAX
                    } else {
                        t.color | 0xFF00_0000
                    };
                    ui::centered_text(
                        batch,
                        atlas,
                        &Text::literal(t.text.clone()),
                        x,
                        wy + 1.0,
                        MOD_UI_COLUMN,
                        color,
                    );
                }
                UiWidget::Item(i) => {
                    if self.res.items().get(&i.item).is_some() {
                        items.push((i.item.clone(), x + 2.0, wy + 2.0));
                    } else {
                        batch.text(atlas, "?", x + 6.0, wy + 6.0, u32::MAX, true);
                    }
                    let name = if i.name.is_empty() {
                        i.item.rsplit(':').next().unwrap_or(&i.item).to_string()
                    } else {
                        i.name.clone()
                    };
                    batch.text_styled(
                        atlas,
                        &Text::literal(name),
                        x + 22.0,
                        wy + 6.0,
                        u32::MAX,
                        true,
                    );
                }
                UiWidget::List(l) => {
                    if ms.scrolls.len() <= list_i {
                        ms.scrolls.resize_with(list_i + 1, ui::Scroll::default);
                    }
                    let content = l.rows.len() as f32 * 12.0;
                    ms.scrolls[list_i].clamp(wy + 4.0, wy + wh - 4.0, content);
                    let scroll = ms.scrolls[list_i];
                    list_i += 1;
                    let rows_top = wy + 4.0;
                    let rows_bottom = wy + wh - 4.0;
                    batch.fill(atlas, x, wy, MOD_UI_COLUMN, wh, 0x8000_0000);
                    for (b, yy) in [(x, wy), (x, wy + wh - 1.0)] {
                        batch.fill(atlas, b, yy, MOD_UI_COLUMN, 1.0, 0xFF50_5050);
                    }
                    for (xx, yy) in [(x, wy), (x + MOD_UI_COLUMN - 1.0, wy)] {
                        batch.fill(atlas, xx, yy, 1.0, wh, 0xFF50_5050);
                    }
                    draw_list_rows(
                        batch,
                        atlas,
                        cursor,
                        &l.rows,
                        l.selected,
                        scroll.amount,
                        (x, wy, wh),
                    );
                    ui::scrollbar(
                        batch,
                        atlas,
                        x + MOD_UI_COLUMN - 5.0,
                        rows_top,
                        rows_bottom,
                        content,
                        scroll.amount,
                    );
                }
            }
        }
        horizon_client::widgets::clip_widgets(widgets_mark, MOD_UI_TOP, page.bottom);
        horizon_client::widgets::clip_targets(targets_mark, MOD_UI_TOP, page.bottom);
        batch.clip_since(start, 0.0, MOD_UI_TOP, batch.width, page.bottom);
        ui::scrollbar(
            batch,
            atlas,
            x + MOD_UI_COLUMN + 4.0,
            MOD_UI_TOP,
            page.bottom,
            page.content,
            ms.scroll.amount,
        );
        // Item geometry is drawn in a second pass, inside the same viewport.
        let start = batch.vertices.len();
        for (id, ix, iy) in items {
            let stack = ItemStack::new(id.as_str(), 1);
            self.draw_item(batch, &stack, ix, iy);
        }
        batch.clip_since(start, 0.0, MOD_UI_TOP, batch.width, page.bottom);
        let atlas = &self.gui.atlas;
        let hover = ui::hovered(self.cursor, cx - 100.0, batch.height - 27.0, 200.0, 20.0);
        ui::button(
            batch,
            atlas,
            cx - 100.0,
            batch.height - 27.0,
            200.0,
            20.0,
            &Text::literal("Done"),
            true,
            hover,
        );
    }
    /// A click on the mod screen: Done leaves it, a `list` row is the mod's
    /// `ui-events::select`.
    pub(super) fn mod_screen_click(&mut self, button: MouseButton) {
        let touch = self.mod_touch_active();
        // Key Binds: a mouse button pressed while waiting binds it.
        if self
            .mods
            .screen
            .as_ref()
            .is_some_and(|ms| ms.binding.is_some())
        {
            self.bind_mod_key(keybinds::mouse_name(button));
            return;
        }
        // On the touch screen a long press arrives as the right button: it
        // resets the key row under it.
        if button != MouseButton::Left && !(touch && button == MouseButton::Right) {
            return;
        }
        let long_press = button == MouseButton::Right;
        let (w, h) = self.gui_size();
        let cursor = self.cursor;
        if !long_press && ui::hovered(cursor, (w / 2.0).floor() - 100.0, h - 27.0, 200.0, 20.0) {
            self.audio.play_button_click();
            self.close_wasm_mod_screen(true);
            return;
        }
        let Some(ms) = self.mods.screen.as_mut() else {
            return;
        };
        let Some((_, widgets)) = Self::mod_ui_view(&self.mods.mods, ms) else {
            return;
        };
        let owner = ms.owner;
        let x = (w / 2.0).floor() - MOD_UI_COLUMN / 2.0;
        // A key row waits for the key to bind.
        let keys = Self::mod_page_keys(&self.mods.mods, ms);
        let page = ModPageLayout::new(
            h,
            keys.len() + Self::mod_page_extra(keys.len()),
            widgets,
            &mut ms.scroll,
        );
        if !page.contains(cursor) {
            return;
        }
        if ms.scroll.press(
            cursor,
            x + MOD_UI_COLUMN + 4.0,
            MOD_UI_TOP,
            page.bottom,
            page.content,
        ) {
            return;
        }
        let rows = keys.len() + Self::mod_page_extra(keys.len());
        let picked = (0..rows).find(|&i| {
            let wy = page.key_y(i);
            ui::hovered(cursor, x, wy, MOD_UI_COLUMN, 20.0)
        });
        if let Some(i) = picked {
            let key = keys.get(i).cloned();
            let count = keys.len();
            self.audio.play_button_click();
            match (key, i - count.min(i)) {
                (Some(k), _) if long_press => self.reset_mod_keys(owner, Some(&k.name)),
                (Some(k), _) if touch => {
                    let Some(id) = self.mod_id(owner) else { return };
                    let shown = self.touch.mod_key_shown(&self.mods.mods, owner, &id, &k);
                    self.touch.set_mod_key_shown(&id, &k.name, !shown);
                    self.save_touch_layout();
                }
                (Some(k), _) => {
                    if let Some(ms) = self.mods.screen.as_mut() {
                        ms.binding = Some(k.name);
                    }
                }
                (None, 0) if !long_press => self.reset_mod_keys(owner, None),
                (None, 1) if !long_press => {
                    self.close_wasm_mod_screen(true);
                    if let Err(e) = self.open_touch_editor() {
                        log::warn!("Horizon: {e}");
                    }
                }
                _ => {}
            }
            return;
        }
        if long_press {
            return;
        }
        let mut list_i = 0usize;
        // The `ui_select` call needs `&mut`: find the pick first.
        let mut hit = None;
        for (w, &(wy, wh)) in widgets.iter().zip(&page.widgets) {
            // A button is `select(id, 0)`.
            if let UiWidget::Button(b) = w
                && ui::hovered(cursor, x, wy, MOD_UI_COLUMN, 20.0)
            {
                self.audio.play_button_click();
                let owner = self.mods.screen.as_ref().map_or(0, |ms| ms.owner);
                self.mods.mods.ui_select(owner, b.id, 0);
                self.show_mod_output();
                return;
            }
            let UiWidget::List(l) = w else { continue };
            if ms.scrolls.len() <= list_i {
                ms.scrolls.resize_with(list_i + 1, ui::Scroll::default);
            }
            ms.scrolls[list_i].clamp(wy + 4.0, wy + wh - 4.0, l.rows.len() as f32 * 12.0);
            let scroll = ms.scrolls[list_i];
            list_i += 1;
            let row = ((cursor.1 - (wy + 4.0) + scroll.amount) / 12.0) as i64;
            if ui::hovered(cursor, x + 1.0, wy + 1.0, MOD_UI_COLUMN - 7.0, wh - 2.0)
                && row >= 0
                && (row as usize) < l.rows.len()
            {
                hit = Some((l.id, row as u32));
                break;
            }
        }
        if let Some((id, row)) = hit {
            self.audio.play_button_click();
            self.mods.mods.ui_select(owner, id, row);
            self.show_mod_output();
        }
    }
    /// The wheel over a `list` scrolls it; elsewhere it scrolls the page.
    pub(super) fn mod_screen_scroll(&mut self, delta: f32) {
        let (w, h) = self.gui_size();
        let cursor = self.cursor;
        let Some(ms) = self.mods.screen.as_mut() else {
            return;
        };
        let Some((_, widgets)) = Self::mod_ui_view(&self.mods.mods, ms) else {
            return;
        };
        let x = (w / 2.0).floor() - MOD_UI_COLUMN / 2.0;
        let keys = Self::mod_page_keys(&self.mods.mods, ms);
        let page = ModPageLayout::new(
            h,
            keys.len() + Self::mod_page_extra(keys.len()),
            widgets,
            &mut ms.scroll,
        );
        page.wheel(ms, widgets, cursor, x, delta);
    }

    pub(super) fn mod_screen_drag(&mut self) {
        let (_, h) = self.gui_size();
        let Some(ms) = self.mods.screen.as_mut() else {
            return;
        };
        let Some((_, widgets)) = Self::mod_ui_view(&self.mods.mods, ms) else {
            return;
        };
        let keys = Self::mod_page_keys(&self.mods.mods, ms);
        let page = ModPageLayout::new(
            h,
            keys.len() + Self::mod_page_extra(keys.len()),
            widgets,
            &mut ms.scroll,
        );
        ms.scroll
            .drag(self.cursor.1, MOD_UI_TOP, page.bottom, page.content);
    }

    pub(super) fn mod_screen_release(&mut self) {
        if let Some(ms) = self.mods.screen.as_mut() {
            ms.scroll.release();
        }
    }
}

/// Shared coordinates for painting, hit testing and wheel routing.
#[cfg(feature = "mods")]
struct ModPageLayout {
    widgets: Vec<(f32, f32)>,
    key_top: f32,
    bottom: f32,
    content: f32,
}

#[cfg(feature = "mods")]
impl ModPageLayout {
    fn new(h: f32, keys: usize, widgets: &[UiWidget], scroll: &mut ui::Scroll) -> Self {
        let top = MOD_UI_TOP + keys as f32 * MOD_UI_KEY_ROW;
        let mut layout = Game::mod_ui_layout(top, widgets);
        let end = layout.last().map_or(top, |&(y, h)| y + h);
        let content = (end - MOD_UI_TOP).max(0.0);
        let bottom = (h - 34.0).max(MOD_UI_TOP);
        scroll.clamp(MOD_UI_TOP, bottom, content);
        for (y, _) in &mut layout {
            *y -= scroll.amount;
        }
        Self {
            widgets: layout,
            key_top: MOD_UI_TOP - scroll.amount,
            bottom,
            content,
        }
    }

    fn key_y(&self, i: usize) -> f32 {
        self.key_top + i as f32 * MOD_UI_KEY_ROW
    }

    fn contains(&self, cursor: (f32, f32)) -> bool {
        cursor.1 >= MOD_UI_TOP && cursor.1 < self.bottom
    }

    fn cursor(&self, cursor: (f32, f32)) -> (f32, f32) {
        if self.contains(cursor) {
            cursor
        } else {
            (f32::NAN, f32::NAN)
        }
    }

    fn wheel(
        &self,
        ms: &mut ModScreen,
        widgets: &[UiWidget],
        cursor: (f32, f32),
        x: f32,
        delta: f32,
    ) {
        if !self.contains(cursor) {
            return;
        }
        let mut list_i = 0usize;
        for (widget, &(wy, wh)) in widgets.iter().zip(&self.widgets) {
            let UiWidget::List(list) = widget else {
                continue;
            };
            if ms.scrolls.len() <= list_i {
                ms.scrolls.resize_with(list_i + 1, ui::Scroll::default);
            }
            if ui::hovered(cursor, x, wy, MOD_UI_COLUMN, wh) {
                ms.scrolls[list_i].wheel(
                    delta,
                    12.0,
                    wy + 4.0,
                    wy + wh - 4.0,
                    list.rows.len() as f32 * 12.0,
                );
                return;
            }
            list_i += 1;
        }
        ms.scroll
            .wheel(delta, MOD_UI_KEY_ROW, MOD_UI_TOP, self.bottom, self.content);
    }
}

/// A mod screen's `list` rows in its frame at (`x`, `wy`), `wh` high, scrolled by `scroll`: the
/// rows in view, the hovered one lit and the selected one marked. As AbstractSelectionList draws
/// its entries inside `enableScissor`, they are cut at the frame's inside: the row a scroll shows
/// in part, and the one below a full view, never draw over the frame or below it.
#[cfg(feature = "mods")]
fn draw_list_rows(
    batch: &mut HudBatch,
    atlas: &horizon_client::hud::HudAtlas,
    cursor: (f32, f32),
    rows: &[String],
    selected: Option<u32>,
    scroll: f32,
    (x, wy, wh): (f32, f32, f32),
) {
    let rows_top = wy + 4.0;
    let rows_bottom = wy + wh - 4.0;
    let first = (scroll / 12.0) as usize;
    let last = (first + ((rows_bottom - rows_top) / 12.0).ceil() as usize + 1).min(rows.len());
    let start = batch.vertices.len();
    for (vi, ri) in (first..last).enumerate() {
        let ry = rows_top + vi as f32 * 12.0 - (scroll % 12.0);
        if ui::hovered(cursor, x, ry, MOD_UI_COLUMN - 6.0, 12.0) {
            batch.fill(atlas, x + 1.0, ry, MOD_UI_COLUMN - 8.0, 12.0, 0x20FF_FFFF);
        }
        if selected == Some(ri as u32) {
            batch.fill(atlas, x + 1.0, ry, 3.0, 12.0, 0xFFFF_FFFF);
        }
        batch.text_styled(
            atlas,
            &Text::literal(rows[ri].clone()),
            x + 6.0,
            ry + 2.0,
            u32::MAX,
            true,
        );
    }
    batch.clip_since(
        start,
        x + 1.0,
        wy + 1.0,
        x + MOD_UI_COLUMN - 1.0,
        wy + wh - 1.0,
    );
}

#[cfg(feature = "mods")]
const MOD_UI_TOP: f32 = 40.0;
#[cfg(feature = "mods")]
const MOD_UI_COLUMN: f32 = 200.0;
/// A key row's height with its gap, as the widgets below it space out.
#[cfg(feature = "mods")]
const MOD_UI_KEY_ROW: f32 = 26.0;

#[cfg(all(test, feature = "mods"))]
mod list_clip_tests {
    use super::*;
    use horizon_client::hud::HudAtlas;

    fn screen() -> ModScreen {
        ModScreen {
            owner: 0,
            settings: true,
            under: Screen::Options,
            scrolls: Vec::new(),
            scroll: ui::Scroll::default(),
            binding: None,
        }
    }

    fn buttons() -> Vec<UiWidget> {
        (0..19)
            .map(|id| {
                UiWidget::Button(horizon_mods::ButtonWidget {
                    id,
                    label: format!("Button {id}"),
                })
            })
            .collect()
    }

    #[test]
    fn crowded_settings_reach_the_last_button_and_clamp_after_resize() {
        let widgets = buttons();
        let mut ms = screen();
        let page = ModPageLayout::new(240.0, 6, &widgets, &mut ms.scroll);
        assert_eq!(page.widgets.len(), 19);
        assert_eq!(page.widgets[0], (196.0, 20.0));
        page.wheel(&mut ms, &widgets, (110.0, 100.0), 100.0, -100.0);
        let page = ModPageLayout::new(240.0, 6, &widgets, &mut ms.scroll);
        assert_eq!(page.widgets[18], (186.0, 20.0));
        assert!(!ui::hovered(
            page.cursor((110.0, 207.0)),
            100.0,
            186.0,
            200.0,
            26.0
        ));
        assert!(ui::hovered(
            page.cursor((110.0, 196.0)),
            100.0,
            186.0,
            200.0,
            20.0
        ));
        assert!(page.key_y(5) < 40.0);
        ModPageLayout::new(900.0, 6, &widgets, &mut ms.scroll);
        assert_eq!(ms.scroll.amount, 0.0);
    }

    #[test]
    fn wheel_over_a_scrolled_list_keeps_the_page_still() {
        let mut widgets = buttons();
        widgets.insert(
            0,
            UiWidget::List(horizon_mods::ListWidget {
                id: 99,
                rows: (0..22).map(|i| format!("Row {i}")).collect(),
                selected: None,
            }),
        );
        let mut ms = screen();
        let page = ModPageLayout::new(240.0, 6, &widgets, &mut ms.scroll);
        // Scroll outside the column to bring the list above the first button.
        page.wheel(&mut ms, &widgets, (90.0, 100.0), 100.0, -6.0);
        assert_eq!(ms.scroll.amount, 156.0);
        let page = ModPageLayout::new(240.0, 6, &widgets, &mut ms.scroll);
        assert_eq!(page.widgets[0], (40.0, 80.0));
        page.wheel(&mut ms, &widgets, (110.0, 60.0), 100.0, -2.0);
        assert_eq!(ms.scrolls[0].amount, 24.0);
        assert_eq!(ms.scroll.amount, 156.0);
        // Neither title nor footer can scroll a hidden list or the page.
        for y in [20.0, 220.0] {
            page.wheel(&mut ms, &widgets, (110.0, y), 100.0, -1.0);
        }
        assert_eq!(ms.scrolls[0].amount, 24.0);
        assert_eq!(ms.scroll.amount, 156.0);
        page.wheel(&mut ms, &widgets, (110.0, 150.0), 100.0, -1.0);
        assert_eq!(ms.scroll.amount, 182.0);
    }

    #[test]
    fn each_list_keeps_its_scroll_across_wheel_events() {
        let widgets: Vec<_> = (0..2)
            .map(|id| {
                UiWidget::List(horizon_mods::ListWidget {
                    id,
                    rows: (0..22).map(|i| format!("Row {i}")).collect(),
                    selected: None,
                })
            })
            .collect();
        let mut ms = screen();
        let page = ModPageLayout::new(400.0, 0, &widgets, &mut ms.scroll);
        for y in [140.0, 60.0, 140.0] {
            page.wheel(&mut ms, &widgets, (110.0, y), 100.0, -1.0);
        }
        assert_eq!(ms.scrolls[0].amount, 12.0);
        assert_eq!(ms.scrolls[1].amount, 24.0);
        assert_eq!(ms.scroll.amount, 0.0);
    }

    #[test]
    fn list_rows_stay_inside_the_frame_scrolled_or_not() {
        let atlas =
            HudAtlas::build(std::path::Path::new("/nonexistent/horizon-test-assets")).unwrap();
        // #3237: 22 rows in a frame of 6 (mod_ui_layout's height for a long list). The 7th row
        // (index 6), drawn for a scroll that shows it in part, is selected and under the
        // cursor, so its fills reach past the frame unless they are cut.
        let rows: Vec<String> = (0..22).map(|i| format!("Row {i}")).collect();
        let (x, wy, wh) = (100.0, 40.0, 6.0 * 12.0 + 8.0);
        for scroll in [0.0, 6.0, 30.0] {
            let mut batch = HudBatch::new([640, 480]);
            let seventh_row = wy + 4.0 + 6.0 * 12.0 - scroll % 12.0;
            draw_list_rows(
                &mut batch,
                &atlas,
                (x + 10.0, seventh_row + 2.0),
                &rows,
                Some(6 + (scroll / 12.0) as u32),
                scroll,
                (x, wy, wh),
            );
            assert!(!batch.vertices.is_empty(), "scroll {scroll}: rows drawn");
            let (top, bottom) = batch
                .vertices
                .iter()
                .map(|v| v.pos[1])
                .fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
            assert!(
                top >= wy + 1.0 && bottom <= wy + wh - 1.0,
                "scroll {scroll}: rows span {top}..{bottom}, the frame's inside is {}..{}",
                wy + 1.0,
                wy + wh - 1.0
            );
        }
    }
}

/// Mob types loaded from a scratch root whose `entity-types.json` has a
/// zombie, a villager, and the rows a content root writes for a mod's types
/// (`mod_content`): `ruby:ghoul` (a zombie with 40 health), `ruby:settler` (a
/// villager as it is) and `ruby:bolt` (a thrown snowball, not a mob).
#[cfg(test)]
pub(super) fn living_mod_types()
-> std::collections::HashMap<String, std::sync::Arc<horizon_client::mobs::MobType>> {
    let zombie =
        r#""width":0.6,"height":1.95,"eye_height":1.74,"category":"monster","fire_immune":false"#;
    let villager =
        r#""width":0.6,"height":1.95,"eye_height":1.62,"category":"misc","fire_immune":false"#;
    let trader =
        r#""width":0.6,"height":1.95,"eye_height":1.62,"category":"creature","fire_immune":false"#;
    let table = format!(
        r#"{{"minecraft:zombie":{{{zombie},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.23000000417232513}}}},
        "minecraft:villager":{{{villager},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.5}}}},
        "minecraft:wandering_trader":{{{trader},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.7}}}},
        "minecraft:snowball":{{"width":0.25,"height":0.25,"eye_height":0.0,"category":"misc","fire_immune":false}},
        "ruby:ghoul":{{{zombie},"attributes":{{"minecraft:max_health":40.0,"minecraft:movement_speed":0.23000000417232513}},"template":"minecraft:zombie"}},
        "ruby:settler":{{{villager},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.5}},"template":"minecraft:villager"}},
        "ruby:merchant":{{{trader},"attributes":{{"minecraft:max_health":20.0,"minecraft:movement_speed":0.7}},"template":"minecraft:wandering_trader"}},
        "ruby:bolt":{{"width":0.25,"height":0.25,"eye_height":0.0,"category":"misc","fire_immune":false,"template":"minecraft:snowball"}}}}"#
    );
    // A folder per call: tests run in parallel in one process, and each removes its own.
    static CALLS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let call = CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "horizon-living-mod-types-{}-{call}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("entity-types.json"), table).unwrap();
    let types = horizon_client::mobs::MobTypes::load_for_server(&root).unwrap();
    let _ = std::fs::remove_dir_all(&root);
    types
        .types
        .into_iter()
        .map(|(id, t)| (id, std::sync::Arc::new(t)))
        .collect()
}

impl Game<'_> {
    /// Stable profile UUID for WASM player events and inventory ownership.
    pub(super) fn player_uuid_string(&self) -> String {
        let uuid = self
            .level
            .player_uuid
            .map(horizon_client::lan::playerdata::ints_uuid)
            .unwrap_or_else(|| horizon_protocol::crypto::offline_uuid(&self.options.player_name()));
        let h = format!("{uuid:032x}");
        format!(
            "{}-{}-{}-{}-{}",
            &h[..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A content mod's living type is a mob of its template's species under
    /// its own id, with its own attributes: it saves its id with every field
    /// its template's class saves, loads back as itself, and rolls its own
    /// loot table. A thrown type stays a projectile.
    #[test]
    fn a_mod_living_type_is_its_templates_mob_with_its_own_attributes() {
        use horizon_client::mobs::{Mob, Species, drops::death_loot_table};
        use horizon_core::geometry::Vec3;
        let types = living_mod_types();
        let (zombie, ghoul) = (&types["minecraft:zombie"], &types["ruby:ghoul"]);
        assert_eq!(ghoul.species, Species::Zombie);
        assert_eq!(ghoul.class_id(), "minecraft:zombie");
        assert_eq!(types["ruby:settler"].species, Species::Villager);
        assert!(!types.contains_key("ruby:bolt"), "a thrown type is no mob");
        assert_eq!((zombie.max_health, ghoul.max_health), (20.0, 40.0));
        assert_eq!(ghoul.speed.to_bits(), zombie.speed.to_bits());
        assert_eq!(
            (ghoul.sounds, ghoul.texture.as_str()),
            (zombie.sounds, zombie.texture.as_str())
        );

        let at = Vec3::new(0.5, 64.0, 0.5);
        let (zombie, ghoul) = (
            Mob::new(1, zombie.clone(), at, 0.0),
            Mob::new(2, ghoul.clone(), at, 0.0),
        );
        assert_eq!(ghoul.health, 40.0);
        let saved = |m: &Mob| horizon_client::entity_nbt::mob_to_nbt(m, [0, 0, 0, 2], None, None);
        let keys = |tag: &horizon_world::nbt::Tag| -> Vec<String> {
            let horizon_world::nbt::Tag::Compound(map) = tag else {
                panic!("a mob saves a compound");
            };
            map.keys().map(|k| k.to_string_lossy()).collect()
        };
        let (zombie_tag, ghoul_tag) = (saved(&zombie), saved(&ghoul));
        let Some(horizon_world::nbt::Tag::String(id)) = ghoul_tag.get("id") else {
            panic!("a saved mob has its id");
        };
        assert_eq!(id.to_string_lossy(), "ruby:ghoul");
        assert_eq!(
            keys(&ghoul_tag),
            keys(&zombie_tag),
            "it saves what a zombie saves"
        );
        for key in ["IsBaby", "CanBreakDoors", "DrownedConversionTime"] {
            assert!(keys(&ghoul_tag).iter().any(|k| k == key), "{key}");
        }
        let loaded = horizon_client::entity_nbt::mob_from_nbt(&ghoul_tag, &types, 3, None)
            .expect("it loads back");
        assert_eq!(
            (loaded.kind.id.as_str(), loaded.health),
            ("ruby:ghoul", 40.0)
        );
        assert_eq!(
            death_loot_table(&ghoul, true).as_deref(),
            Some("ruby:entities/ghoul")
        );
        assert_eq!(
            death_loot_table(&zombie, true).as_deref(),
            Some("minecraft:entities/zombie")
        );
    }

    /// A content mod's type from the wandering trader, the neutral template
    /// whose behaviour is all goals: a trader under its own id, with a
    /// trader's goals, that saves what a trader saves and loads back.
    #[test]
    fn a_mod_trader_type_is_a_wandering_trader_under_its_own_id() {
        use horizon_client::mobs::{Mob, Species};
        use horizon_world::nbt::Tag;
        let types = living_mod_types();
        let merchant = &types["ruby:merchant"];
        assert_eq!(merchant.species, Species::WanderingTrader);
        assert_eq!(merchant.class_id(), "minecraft:wandering_trader");
        let at = horizon_core::geometry::Vec3::new(0.5, 64.0, 0.5);
        let trader = Mob::new(1, types["minecraft:wandering_trader"].clone(), at, 0.0);
        let merchant = Mob::new(2, merchant.clone(), at, 0.0);
        assert!(merchant.ai.goals.has("Float") && trader.ai.goals.has("Float"));
        let saved = |m: &Mob| horizon_client::entity_nbt::mob_to_nbt(m, [0, 0, 0, 2], None, None);
        let keys = |tag: &Tag| -> Vec<String> {
            let Tag::Compound(map) = tag else {
                panic!("a mob saves a compound");
            };
            map.keys().map(|k| k.to_string_lossy()).collect()
        };
        let tag = saved(&merchant);
        assert_eq!(
            keys(&tag),
            keys(&saved(&trader)),
            "it saves what a trader saves"
        );
        let loaded =
            horizon_client::entity_nbt::mob_from_nbt(&tag, &types, 3, None).expect("it loads back");
        assert_eq!(
            (loaded.kind.id.as_str(), loaded.species()),
            ("ruby:merchant", Species::WanderingTrader)
        );
    }

    /// Only a build whose `native-mods.json` lists a mod counts as built with mods.
    #[test]
    fn only_a_listed_mod_makes_a_build_a_mod_build() {
        assert!(!build_lists_mods(""));
        assert!(!build_lists_mods("{}"));
        assert!(!build_lists_mods(r#"{"mods":[]}"#));
        assert!(!build_lists_mods(r#"{"mods":"worldmap"}"#));
        assert!(build_lists_mods(r#"{"mods":[{"id":"worldmap"}]}"#));
    }

    /// A minimal `.hnmod` the registry accepts: its manifest and its mixin's.
    fn hnmod(dir: &std::path::Path, id: &str) {
        let horizon = "a".repeat(40);
        let manifest = serde_json::json!({
            "schema": 1, "id": id, "version": "0.3.0", "game": "26.3",
            "horizon": horizon, "mixins": "b".repeat(40),
        });
        let mixin = format!(
            "[mod]\nid = \"{id}\"\nversion = \"0.3.0\"\n[compatibility]\nhorizon_revision = \"{horizon}\"\n"
        );
        let file = std::fs::File::create(dir.join(format!("{id}.hnmod"))).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("horizon-native.json", options).unwrap();
        zip.write_all(manifest.to_string().as_bytes()).unwrap();
        zip.start_file(format!("mixins/{id}/horizon-mixin.toml"), options)
            .unwrap();
        zip.write_all(mixin.as_bytes()).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn the_crash_report_lists_the_builds_native_mods() {
        let built = r#"{"mods":[{"id":"machine_sample","version":"0.1.0","sha256":null},{"id":"pipes","version":"2.0"}]}"#;
        assert_eq!(
            native_mods_detail(built).as_deref(),
            Some("machine_sample 0.1.0, pipes 2.0")
        );
        // A release has no file; a build without mods has an empty list.
        assert_eq!(native_mods_detail(""), None);
        assert_eq!(native_mods_detail(r#"{"mods":[]}"#), None);
    }

    #[test]
    fn native_mods_installed_but_not_built_get_a_row_saying_so() {
        let dir = std::env::temp_dir().join(format!("horizon-not-built-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for id in ["veinminer", "zoom", "minimap"] {
            hnmod(&dir, id);
        }
        std::fs::write(dir.join("notes.txt"), "not a mod").unwrap();
        std::fs::write(dir.join("broken.hnmod"), "not a zip").unwrap();
        // A release: none is built in; the page lists them, sorted, instead of "No mods".
        let rows = not_built(&dir, "", &[]);
        let ids: Vec<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["minimap", "veinminer", "zoom"]);
        assert_eq!(rows[1].title, "veinminer 0.3.0");
        assert_eq!(rows[1].problem.as_deref(), Some(mod_list::NOT_BUILT));
        assert!(rows.iter().all(|r| r.native.is_none() && !r.dev));
        // A native build with zoom, and minimap's row already listed by its own hook.
        let built =
            r#"{"base":"nightly-2026-10-30-1da354b","mods":[{"id":"zoom","version":"0.3.0"}]}"#;
        let listed = Installed {
            id: "minimap".into(),
            ..rows[0].clone()
        };
        let rows = not_built(&dir, built, &[listed]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "veinminer");
        // No folder: no rows.
        assert!(not_built(&dir.join("missing"), "", &[]).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A trait mod's `register_goals` (`Mob.registerGoals`) gets every new
    /// mob: the goals of its type replace its template's, its `Float` goal
    /// floats the navigation as FloatGoal's constructor does, and the
    /// template's other types keep their goals. The only test here that
    /// installs the client library's hooks.
    #[test]
    fn a_mods_register_goals_replaces_its_types_goals() {
        use horizon_client::mobs::{
            Mob,
            goals::{self, Cx, Goal},
        };
        use horizon_client::mod_api::{Hooks, RegisterGoalsEvent, dispatch_register_goals};
        use horizon_client::native_client_hooks::{NativeClientHooks, install};
        use horizon_core::geometry::Vec3;
        // docs/native-mods.md's example.
        #[derive(Clone, Debug)]
        struct ToPost {
            post: [f64; 3],
        }
        impl Goal for ToPost {
            fn flags(&self) -> u8 {
                goals::MOVE
            }
            fn can_use(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
                let [x, y, z] = self.post;
                let (dx, dy, dz) = (m.position.x - x, m.position.y - y, m.position.z - z);
                dx * dx + dy * dy + dz * dz > 16.0
            }
            fn can_continue(&mut self, m: &mut Mob, _: &mut Cx) -> bool {
                !m.ai.nav.is_done()
            }
            fn start(&mut self, m: &mut Mob, cx: &mut Cx) {
                m.nav_move_to(cx.w, self.post, 1.0);
            }
            fn box_clone(&self) -> Box<dyn Goal> {
                Box::new(self.clone())
            }
        }
        struct Guards;
        impl Hooks for Guards {
            fn register_goals(&self, event: &mut RegisterGoalsEvent<'_>) {
                if event.mob.kind.id == "ruby:guard" {
                    let ai = &mut event.mob.ai;
                    ai.goals.clear();
                    ai.targets.clear();
                    ai.goals.add(0, goals::Float);
                    ai.goals.add(
                        1,
                        ToPost {
                            post: [0.5, 64.0, 0.5],
                        },
                    );
                }
            }
        }
        static MODS: std::sync::LazyLock<Vec<Box<dyn Hooks>>> =
            std::sync::LazyLock::new(|| vec![Box::new(Guards)]);
        let installed = install(NativeClientHooks {
            menu_slot_rules: |_, _, _, _, _| {},
            living_heal: |_, _, _| {},
            player_chat: |_, _, _, _, _| {},
            living_breathe: |_, _, _, _, _| {},
            living_drown: |_, _, _, _, _, _| {},
            living_fall: |_, _, _, _, _| {},
            living_jump: |_, _| {},
            register_goals: |mob| dispatch_register_goals(&MODS, mob),
            ingredient: |_, _, _| {},
            ingredient_test: |_, _, _, _| {},
        });
        assert!(installed.is_ok());

        let types = living_mod_types();
        let mut guard = (*types["ruby:ghoul"]).clone();
        guard.id = "ruby:guard".into();
        let at = Vec3::new(0.5, 64.0, 0.5);
        let guard = Mob::new(1, std::sync::Arc::new(guard), at, 0.0);
        let ghoul = Mob::new(2, types["ruby:ghoul"].clone(), at, 0.0);
        assert!(guard.ai.goals.has("Float") && guard.ai.goals.has("ToPost"));
        assert!(!guard.ai.goals.has("RandomLookAround"));
        assert!(!guard.ai.targets.has("NearestTarget"));
        assert!(guard.ai.nav.can_float);
        assert!(ghoul.ai.goals.has("RandomLookAround") && !ghoul.ai.goals.has("ToPost"));
        assert!(ghoul.ai.targets.has("NearestTarget"));
        assert!(!ghoul.ai.nav.can_float, "a zombie has no FloatGoal");
    }
}
