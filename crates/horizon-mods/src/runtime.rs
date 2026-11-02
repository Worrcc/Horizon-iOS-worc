//! Loading and calling mods with wasmtime's component model.
use crate::manifest::{Manifest, Package, Side};
use server_bindings::{
    HorizonServerMod,
    exports::horizon::mods::server_events::Registration as ServerRegistration,
    horizon::mods::{entities, players, world as mod_world},
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime},
};
use wasmtime::{
    Config, Engine, Store, StoreLimits, StoreLimitsBuilder, Trap,
    component::{Component, HasSelf, Linker, Val},
};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-mod",
    });
}
/// The server mods' world: `log` and `storage` are the client world's.
mod server_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "horizon-server-mod",
        additional_derives: [PartialEq, serde::Deserialize],
        with: {
            "horizon:mods/log": super::bindings::horizon::mods::log,
            "horizon:mods/storage": super::bindings::horizon::mods::storage,
        },
    });
}
/// Capability `ui`'s world beyond `horizon-mod` (docs/mods.md): a second
/// world, so mods built before it (no `ui-events` export) still instantiate
/// as `horizon-mod` and simply hear nothing of their widgets.
mod ui_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "ui-host",
    });
}
pub use bindings::horizon::mods::{
    game::{Clock, EntityHit, Hit, Player},
    hud::{Corner, Line},
};
use bindings::{
    HorizonMod,
    exports::horizon::mods::events::Registration,
    horizon::mods::{chat, command, game, hud, log as mod_log, storage},
};
pub use server_bindings::{
    exports::horizon::mods::server_events::{
        Argument, ArgumentKind, ArgumentValue, BlockPos, ServerCommand,
    },
    horizon::mods::entities::{
        Area as EntityArea, AttributeChange as EntityAttributeChange, Death, EffectChange,
        Entity as ModEntity, HurtEvent, Interaction as EntityInteraction, Mount as EntityMount,
        Order as EntityOrder, Particles as EntityParticles, Point as EntityPoint,
        Rider as EntityRider, Sound as EntitySound, StrikeHit, Target as EntityTarget,
        Walk as EntityWalk,
    },
    horizon::mods::players::{
        Attack as PlayerAttack, BlockHit as PlayerBlockHit, EquipmentSlots, GameMode,
        ItemUse as PlayerItemUse, Look as PlayerLook, Player as ServerPlayer, PlayerDamage,
        Point as PlayerPoint,
    },
    horizon::mods::world::{
        BlockChange, ChunkPos, ItemStack as ContainerItemStack, Point as WorldPoint, WorldTime,
    },
};
type ClientEvents = bindings::exports::horizon::mods::events::Guest;
type ServerEvents = server_bindings::exports::horizon::mods::server_events::Guest;
/// The widget model a `ui` screen or settings page shows (WIT `interface
/// ui`), for the game to draw with its menus' own widgets.
pub use ui_bindings::horizon::mods::ui::{
    ButtonWidget, ItemWidget, ListWidget, Screen as UiScreen, TextWidget, Widget as UiWidget,
};
use ui_bindings::{UiHost, horizon::mods::ui};
/// Capability `render`'s imports (docs/mods.md): the game draws the shapes.
mod render_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "render-host",
    });
}
use render_bindings::horizon::mods::input;
/// Capability `input`'s records (WIT `interface input`).
pub use render_bindings::horizon::mods::input::{Event as InputEvent, HeldItem, ItemUse};
/// What capability `render` asks the game to draw (WIT `interface render`).
pub use render_bindings::horizon::mods::render::{
    Corner as RenderCorner, FarTerrain as ModFarTerrain, FillShape, FrameRate, ItemShape,
    Minimap as ModMinimap, Point as RenderPoint, QuadShape, Rect as RenderRect, Shape, SpriteShape,
    TextShape, Zoom as ModZoom,
};
use render_bindings::{RenderHost, horizon::mods::render};
/// Capability `recipes`'s imports (docs/mods.md): the game's recipes and items.
mod recipes_bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "recipes-host",
    });
}
/// Capability `ui`'s recipe viewer panel (WIT `interface browser`).
pub use recipes_bindings::horizon::mods::browser::{
    Click as BrowserClick, Event as BrowserEvent, ItemKey as BrowserItemKey,
    Layout as BrowserLayout, RecipeView as BrowserRecipe, SlotClick as BrowserSlotClick,
    Stack as BrowserStack,
};
/// What capability `recipes` reads (WIT `interface recipes`).
pub use recipes_bindings::horizon::mods::recipes::{
    Item as RecipeItem, Kind as RecipeKind, Recipe, Stack as RecipeStack,
};
use recipes_bindings::{RecipesHost, horizon::mods::browser, horizon::mods::recipes};

/// A mod's linear memory cap.
const MEMORY: usize = 64 << 20;
/// Fuel (about one unit per WebAssembly instruction) per call: a bound on
/// runaway loops, far above the hooks' time budgets.
const FUEL_INIT: u64 = 2_000_000_000;
const FUEL_TICK: u64 = 50_000_000;
const FUEL_FRAME: u64 = 10_000_000;
const FUEL_EVENT: u64 = 100_000_000;
/// `world.generated-chunks` lists at most this many chunks.
pub const GENERATED_CHUNKS: usize = 256;
/// Item uses and landed melee attacks retained per tick.
pub const PLAYER_ACTIONS: usize = 256;
/// Largest block container accepted by the plain-slot API.
pub const CONTAINER_SLOTS: usize = 256;
/// Blocks a mod may set per `world.set-blocks` call.
pub const BLOCKS_PER_CALL: usize = 4096;
/// Orders a mod may give per `entities.give-orders` call, mobs `entities.mobs` and
/// `entities.near` return, and `near`'s and an area's largest radius.
pub const ORDERS_PER_CALL: usize = 1024;
const OWN_ENTITIES: usize = 1024;
const NEAR_ENTITIES: usize = 256;
const NEAR_RADIUS: f64 = 64.0;
const AREA_RADIUS: f64 = 16.0;
/// `entities.spawn` calls one hook call may make.
const SPAWNS_PER_CALL: usize = 64;
/// `entities.ride`: at most this many per hook call, within this many blocks.
const RIDES_PER_CALL: usize = 64;
const RIDE_RANGE: f64 = 16.0;
/// `set-data`'s bounds: keys a mob keeps, a key's and a value's length.
const DATA_KEYS: usize = 32;
const DATA_KEY_CHARS: usize = 64;
const DATA_VALUE_CHARS: usize = 1024;
/// Chat lines a mod may show per call, HUD lines and their length.
const CHAT_PER_CALL: usize = 8;
/// `command.run-quiet` lines one call of a mod may queue (a quiet payload's
/// cap, `horizon_protocol::quiet::MAX_LINES`).
const QUIET_PER_CALL: usize = 256;
/// How far `game.raycast` sees, in blocks.
pub const RAYCAST_MAX: f32 = 1024.0;
const HUD_LINES: usize = 16;
const LINE_CHARS: usize = 256;
/// Total bytes a mod's `storage` may hold on disk, across all its keys.
const STORAGE_QUOTA: u64 = 4 << 20;
/// Distinct keys a mod's `storage` may hold: `STORAGE_QUOTA` alone doesn't
/// bound a flood of zero- or near-zero-byte files (each still a file name,
/// a directory entry and an inode).
const STORAGE_MAX_KEYS: usize = 64;
/// Bytes `storage::set` may write across one hook call: fuel bounds a
/// mod's CPU time per call, not the time a synchronous disk write takes, so
/// a cheap-in-fuel loop replacing one key with large payloads needs its own
/// cap, separate from the on-disk quota it would otherwise stay under.
const STORAGE_WRITE_BUDGET: u64 = 1 << 20;

/// Resource names accepted by entity orders, shared across ticks until resources reload.
#[derive(Clone, Debug, Default)]
pub struct EntityResources {
    pub textures: std::collections::HashSet<String>,
    pub sounds: std::collections::HashSet<String>,
    /// Only vanilla particle codecs with no options, by network id.
    pub particles: std::collections::HashMap<String, i32>,
}

/// What mods may read of the game, copied once per tick by the game.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub player: Option<Player>,
    pub clock: Option<Clock>,
    /// The screen in GUI pixels (capability `render`'s `gui-size`).
    pub gui_size: (f32, f32),
    /// The main hand's stack (capability `input`'s `held`).
    pub held: Option<HeldItem>,
    /// The game's frame counter: fps, min, 1% low (`render.frame-stats`).
    pub frame: (u32, u32, u32),
    /// The movement and click mappings `input.mapping` answers for:
    /// (mapping, the bound key's label, held).
    pub mappings: Vec<(&'static str, String, bool)>,
    /// Attack and use clicks in the last second.
    pub cps: (u32, u32),
    /// The HUD font's advance per character from U+0000
    /// (`render.text-width`); empty: 6 each.
    pub font_widths: Arc<[f32]>,
    /// The players on the server, for server mods.
    pub players: Vec<ServerPlayer>,
    /// Current server rules and difficulty, for read-only capability `world` queries.
    pub game_rules: std::collections::BTreeMap<String, String>,
    pub difficulty: String,
    /// The dimension-specific daylight and weather, with the server's shared clock.
    pub world_times: std::collections::BTreeMap<String, WorldTime>,
    /// The loaded mobs, for capability `entities`; the game fills it only
    /// while a running mod has it ([`Mods::wants_entities`]).
    pub entities: Arc<[ModEntity]>,
    /// The content mods' mob types, for `entities.spawn`; filled with [`Snapshot::entities`].
    pub mob_kinds: Arc<[String]>,
    /// Loaded mobs riding something: (rider, vehicle), for `entities.vehicle`
    /// and `entities.ride`; filled with [`Snapshot::entities`].
    pub vehicles: Arc<[(u32, u32)]>,
    /// Loaded mobs whose runtime species can be tamed (including content templates).
    pub tamable_mobs: Arc<[u32]>,
    /// Loaded resources used to check texture, sound and particle orders.
    pub entity_resources: Arc<EntityResources>,
    /// Players' right clicks on content mods' mobs during the last tick, for
    /// `entities.interactions`.
    pub interactions: Arc<[EntityInteraction]>,
    /// Item right clicks and landed melee hits during the last tick.
    pub item_uses: Arc<[PlayerItemUse]>,
    pub attacks: Arc<[PlayerAttack]>,
    pub hurt_events: Arc<[HurtEvent]>,
    pub deaths: Arc<[Death]>,
    pub player_damage: Arc<[PlayerDamage]>,
    /// Status effects from the game registry, including for players-only mods.
    pub effect_ids: Arc<std::collections::HashSet<String>>,
    /// Where each online server player looks, by UUID, for `players.facing`.
    pub player_looks: Arc<[(String, PlayerLook)]>,
    /// The chunks first generated during the last tick, for `world.generated-chunks`.
    pub generated_chunks: Arc<[ChunkPos]>,
    /// The first block along the player's view within [`RAYCAST_MAX`] and
    /// its distance from the eye, for `game.raycast`; the game works it out
    /// only while a mod calls it ([`Mods::wants_raycast`]).
    pub view_hit: Option<(f32, Hit)>,
    /// Nearest living entity along the view and its distance from the eye.
    pub view_entity: Option<(f32, EntityHit)>,
    /// Every item and recipe, for capability `recipes`; the game makes them
    /// once, and only while a mod reads them ([`Mods::wants_recipes`]).
    pub recipes: Option<Arc<Recipes>>,
    /// The `book-id`s of the recipes the player's book holds.
    pub known_recipes: Arc<[i32]>,
    /// The open screen takes recipes (`recipes.place`).
    pub placing: bool,
    /// The player is in creative mode (`recipes.give`).
    pub creative: bool,
}

/// Capability `recipes`'s tables: `items` in the registry's order, which
/// each [`RecipeStack::item`] indexes.
#[derive(Clone, Debug, Default)]
pub struct Recipes {
    pub items: Vec<RecipeItem>,
    pub recipes: Vec<Recipe>,
}

/// Which mods a game starts: the window's game the client mods, the game
/// simulating the world the server mods, single player in the frame both.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Runs {
    Client,
    Server,
    Both,
}
impl Runs {
    fn starts(self, side: Side) -> bool {
        match self {
            Runs::Both => true,
            Runs::Client => side != Side::Server,
            Runs::Server => side == Side::Server,
        }
    }
}

/// What mods asked the game to show since the last [`Mods::drain`].
#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// `chat.show`: a line after the mod's name.
    Chat {
        name: String,
        text: String,
    },
    /// A mod command's feedback, red when `error`.
    Feedback {
        text: String,
        error: bool,
    },
    /// `command.run`: a vanilla command line, without its slash, to run as
    /// the player.
    Command {
        line: String,
    },
    /// `command.run-quiet`: vanilla command lines to run as the player with
    /// nothing in the chat (`horizon_protocol::quiet` on a server).
    QuietCommands {
        mod_id: String,
        lines: Vec<String>,
    },
    /// The mod was turned off: it trapped, overran or failed to start.
    Stopped {
        name: String,
        reason: String,
    },
    /// `players.tell`: a line after the mod's name, to the player with this id.
    Tell {
        name: String,
        player: String,
        text: String,
    },
    /// `players.broadcast`: a line after the mod's name, to every player.
    Broadcast {
        name: String,
        text: String,
    },
    /// A server mod command's feedback, to the player who typed it.
    Reply {
        player: String,
        text: String,
        error: bool,
    },
    /// `world.set-blocks`: changes to make in `dimension`, in order, each
    /// state checked by the [`WorldView`] lent for the call.
    SetBlocks {
        name: String,
        dimension: String,
        blocks: Vec<BlockChange>,
    },
    /// `world.set-loot-table`: a container's loot table, after the tick's block writes.
    SetLootTable {
        dimension: String,
        pos: (i32, i32, i32),
        table: String,
    },
    /// `players.add-effect`: a change sent to the player's client too.
    PlayerEffect {
        player: String,
        effect: EffectChange,
    },
    /// `world.set-container`: plain slots replaced after the tick's block writes.
    SetContainer {
        dimension: String,
        pos: (i32, i32, i32),
        slots: Vec<Option<ContainerItemStack>>,
    },
    /// `world.place-structure`: a template loaded after the tick's block writes.
    PlaceStructure {
        dimension: String,
        id: String,
        pos: (i32, i32, i32),
        rotation: u32,
        mirror: bool,
    },
    MobEffect {
        mob: u32,
        change: EffectChange,
    },
    MobPush {
        mob: u32,
        velocity: EntityPoint,
    },
    PlayerHeal {
        player: String,
        amount: f32,
    },
    SpawnItem {
        dimension: String,
        at: EntityPoint,
        item: String,
        count: u32,
    },
    Tame {
        mob: u32,
        owner: Option<String>,
    },
    DamageItem {
        player: String,
        hand: String,
        amount: u32,
    },
    Strike {
        mob: u32,
        hit: StrikeHit,
    },
    PlayerKnockback {
        player: String,
        strength: f32,
        dx: f64,
        dz: f64,
    },
    PlayerPush {
        player: String,
        velocity: EntityPoint,
    },
    PlaySound {
        dimension: String,
        at: EntityPoint,
        sound: EntitySound,
    },
    /// `entities.ride`: a content mod's mob onto another mob, or off (none), with the
    /// tick's orders.
    Ride {
        rider: u32,
        vehicle: Option<u32>,
    },
    /// `world.set-spawner`: a monster spawner's mob, after the tick's block writes.
    SetSpawner {
        dimension: String,
        pos: (i32, i32, i32),
        entity: String,
    },
    /// `entities.give-orders`: orders to content mods' mobs, by id, in order, each
    /// checked against the tick's [`Snapshot::entities`].
    Orders {
        name: String,
        orders: Vec<(u32, EntityOrder)>,
    },
    /// `entities.spawn`: a content mod's mob to spawn, persistent, with data stored on it.
    Spawn {
        name: String,
        kind: String,
        dimension: String,
        at: [f64; 3],
        yaw: f32,
        data: Vec<(String, String)>,
    },
    /// `ui.open-screen`: the mod's entry in `entries()`; what it shows is
    /// [`Mods::screen`].
    OpenScreen {
        owner: usize,
    },
    /// `ui.close-screen`: the mod closed its own screen (no `closed` back).
    CloseScreen {
        owner: usize,
    },
    /// `render.shake`: strength 0 to 1, fading out over `seconds` (≤ 10).
    Shake {
        strength: f32,
        seconds: f32,
    },
    /// `recipes.place`: the recipe book's click on this display id, all of
    /// it when `all`.
    PlaceRecipe {
        book_id: i32,
        all: bool,
    },
    /// `recipes.give`: a stack for a creative player.
    Give {
        item: String,
        count: u32,
        components: String,
    },
}

/// The game's world as capability `world` reads it during a call: the
/// game lends one with [`Mods::lend_world`] and takes it back after.
pub trait WorldView: std::any::Any + Send {
    fn block_has_tag(&self, _block: &str, _tag: &str) -> bool {
        false
    }
    /// The block state at a position, `minecraft:oak_log[axis=y]`; `None`
    /// where no chunk of that dimension is loaded.
    fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String>;
    /// The biome at a position, `minecraft:plains`; `None` as for blocks.
    fn biome(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String>;
    /// Sky light 15 at the position; false when no chunk can be read.
    fn can_see_sky(&self, _dimension: &str, _x: i32, _y: i32, _z: i32) -> bool {
        false
    }
    /// One block's plain container slots (components are not carried).
    fn container(
        &self,
        _dimension: &str,
        _x: i32,
        _y: i32,
        _z: i32,
    ) -> Option<Vec<Option<ContainerItemStack>>> {
        None
    }
    /// Check the item registry and that a nonempty stack fits its maximum size.
    fn check_item_stack(&self, item: &str, count: u32) -> Result<(), String> {
        if resource_id(item) && (1..=99).contains(&count) {
            Ok(())
        } else {
            Err(format!("invalid item stack {item} x{count}"))
        }
    }
    /// Whether a structure template exists in the loaded vanilla/mod resources.
    fn check_structure(&self, id: &str) -> Result<(), String> {
        Err(format!("no structure template {id}"))
    }
    /// Whether `dimension` is one the server has.
    fn has_dimension(&self, dimension: &str) -> bool;
    /// `Err` (why) for a state string that names no block state.
    fn check_state(&self, state: &str) -> Result<(), String>;
}

/// The view lent for the current calls, shared by every mod's store.
type WorldSlot = Arc<Mutex<Option<Box<dyn WorldView>>>>;

/// The real server inventories lent during mod hooks, shared between all mods.
/// Implementations validate item ids and preserve stack components on removal.
pub trait PlayerInventories: std::any::Any + Send {
    fn equipment(&self, _player: &str) -> Option<EquipmentSlots> {
        None
    }
    fn take_item(&mut self, player: &str, item: &str, count: u32) -> Result<u32, String>;
    fn give_item(&mut self, player: &str, item: &str, count: u32) -> Result<(), String>;
}
type InventoriesSlot = Arc<Mutex<Option<Box<dyn PlayerInventories>>>>;

/// A mod's text panel, drawn every frame until it changes.
#[derive(Clone, Debug)]
pub struct Panel {
    pub corner: Corner,
    pub lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Installed and turned off by the player.
    Off,
    Running,
    /// Turned on, and run by another game: a server mod in the window while
    /// the world runs on its server thread.
    Elsewhere,
    /// Could not load or start, or stopped while running.
    Failed(String),
}

/// One installed mod, for the Mods page.
pub struct Entry {
    pub path: PathBuf,
    /// None when the manifest could not be read (`status` says why).
    pub manifest: Option<Manifest>,
    pub status: Status,
    /// The mod's average time per call of its busiest hook.
    pub cost: Duration,
    run: Option<Running>,
    modified: Option<SystemTime>,
}
impl Entry {
    /// The running mod has a settings page (capability `ui` set widgets
    /// with `ui.set-settings`, or `input` registered keys the player may
    /// bind there): its row in Options > Mods gains a button.
    pub fn has_ui_settings(&self) -> bool {
        self.run.as_ref().is_some_and(|r| {
            let d = r.store.data();
            !d.ui.settings.is_empty() || !d.input.keys.is_empty()
        })
    }
}

struct Running {
    store: Store<State>,
    /// The mod's `ui-events` export; none for a mod built before `ui`, or a
    /// server mod.
    ui: Option<UiHost>,
    hooks: Hooks,
    /// It imports `game.raycast`.
    raycasts: bool,
    /// It imports `game.raycast-entity`.
    entity_raycasts: bool,
    /// It imports capability `recipes`.
    reads_recipes: bool,
}

/// A running mod's exports and the hooks its `init` asked for.
enum Hooks {
    Client(HorizonMod, Registration),
    Server(HorizonServerMod, ServerRegistration),
}
impl Hooks {
    fn client(&self) -> Option<&Registration> {
        match self {
            Hooks::Client(_, r) => Some(r),
            Hooks::Server(..) => None,
        }
    }
    fn server(&self) -> Option<&ServerRegistration> {
        match self {
            Hooks::Server(_, r) => Some(r),
            Hooks::Client(..) => None,
        }
    }
    fn commands(&self) -> Vec<&str> {
        match self {
            Hooks::Client(_, r) => r.commands.iter().map(String::as_str).collect(),
            Hooks::Server(_, r) => r.commands.iter().map(|c| c.name.as_str()).collect(),
        }
    }
}

/// What capability `ui` pushed since the hooks last ran.
#[derive(Default)]
struct UiState {
    /// The screen `ui.open-screen` last pushed, until `ui.close-screen`.
    screen: Option<UiScreen>,
    /// The widgets `ui.set-settings` last pushed (the mod's page in
    /// Options > Mods).
    settings: Vec<UiWidget>,
    /// The calls since the last hook ended, for [`Output`].
    open: bool,
    close: bool,
}

/// Capability `ui`'s panel beside container screens (`browser`).
#[derive(Default)]
struct BrowserState {
    items: Vec<BrowserStack>,
    /// Which `set-items` or `set-recipes` call the panel is from, unique
    /// across mods and reloads, so the game converts them once.
    version: u64,
    /// `set-recipes`' title and recipes, shown instead of the items.
    recipes: Option<RecipePage>,
    events: std::collections::VecDeque<BrowserEvent>,
}
/// What `browser.set-recipes` shows: a title, tabs (`tab` the selected
/// one) and the recipes.
#[derive(Clone, Debug)]
pub struct RecipePage {
    pub title: String,
    pub tabs: Vec<BrowserStack>,
    pub tab: u32,
    pub recipes: Vec<BrowserRecipe>,
}
static BROWSER_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// Items a panel may hold, and events kept for `browser.poll`.
const BROWSER_ITEMS: usize = 8192;
/// `recipes.place` and `give` calls a hook call may make.
const RECIPE_ACTIONS: usize = 36;
/// Recipes `set-recipes` may show, slots in one, and stacks a slot cycles.
const BROWSER_RECIPES: usize = 256;
const RECIPE_SLOTS: usize = 9;
const RECIPE_TABS: usize = 16;
const SLOT_STACKS: usize = 64;
const BROWSER_EVENTS: usize = 256;
impl BrowserState {
    fn push(&mut self, event: BrowserEvent) {
        if self.events.len() == BROWSER_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

struct State {
    name: String,
    id: String,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    panel: Option<Panel>,
    ui: UiState,
    /// Capability `render`'s HUD shapes, drawn until replaced.
    shapes: Vec<Shape>,
    /// Capability `render`'s corner panel.
    corner: Option<CornerPanel>,
    /// Capability `render`'s zoom, when the mod zooms.
    zoom: Option<ModZoom>,
    /// Capability `render`'s minimap, when the mod shows one.
    minimap: Option<ModMinimap>,
    /// Capability `render`'s far terrain, when the mod draws one.
    far_terrain: Option<ModFarTerrain>,
    input: InputState,
    browser: BrowserState,
    /// Capability `storage`'s folder; made on first `set`.
    data_dir: PathBuf,
    /// Bytes `storage::set` may still write before this hook call ends
    /// (reset per call in [`Mods::call`]): fuel bounds a mod's CPU time but
    /// not disk I/O time, so a loop of small-fuel, large-payload `set`s
    /// needs its own budget.
    storage_budget: u64,
    /// Tick of the last item spawn, and successful requests in that tick.
    item_spawn_budget: (Option<u64>, u32),
    limits: StoreLimits,
    world: WorldSlot,
    inventories: InventoriesSlot,
}

/// The installed mods and the engine running them. `Mods::default()` is
/// the state with none: no engine, and every hook a no-op.
#[derive(Default)]
pub struct Mods {
    engine: Option<(Engine, Linker<State>)>,
    entries: Vec<Entry>,
    snapshot: Arc<Snapshot>,
    out: Vec<Output>,
    world: WorldSlot,
    inventories: InventoriesSlot,
    /// Moves on whenever a mod starts or stops: the game resends the server
    /// mods' commands to players when it changes.
    generation: u64,
}

impl Mods {
    /// Changes whenever the running mods (so their commands) change.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Whether a running mod may use capability `world`: only then is a
    /// view worth lending.
    pub fn wants_world(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::World))
        })
    }
    /// Whether lending inventories for capability `players` is useful.
    pub fn wants_players(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::Players))
        })
    }
    pub fn lend_inventories(&mut self, inventories: Box<dyn PlayerInventories>) {
        *self
            .inventories
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(inventories);
    }
    pub fn take_inventories(&mut self) -> Option<Box<dyn PlayerInventories>> {
        self.inventories
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
    /// Whether a running mod may use capability `entities`: only then is
    /// [`Snapshot::entities`] worth filling.
    pub fn wants_entities(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::Entities))
        })
    }
    /// Lends the game's world to capability `world` for the calls until
    /// [`Mods::take_world`]; with none lent, `get-block` finds nothing and
    /// `set-blocks` checks only its count.
    pub fn lend_world(&mut self, view: Box<dyn WorldView>) {
        if let Ok(mut slot) = self.world.lock() {
            *slot = Some(view);
        }
    }
    /// The view [`Mods::lend_world`] lent, back to the game.
    pub fn take_world(&mut self) -> Option<Box<dyn WorldView>> {
        self.world.lock().ok()?.take()
    }

    /// Reads every mod at `paths` and starts those `enabled` says are on,
    /// client and server mods alike. A command that `reserved` (vanilla's
    /// commands) or an earlier mod already uses is refused.
    pub fn load(
        paths: &[PathBuf],
        enabled: impl Fn(&str) -> bool,
        reserved: impl Fn(&str) -> bool,
    ) -> Mods {
        Mods::load_for(Runs::Both, paths, enabled, reserved)
    }
    /// [`Mods::load`], starting only the mods `runs` says this game runs; the
    /// others that are on are [`Status::Elsewhere`].
    pub fn load_for(
        runs: Runs,
        paths: &[PathBuf],
        enabled: impl Fn(&str) -> bool,
        reserved: impl Fn(&str) -> bool,
    ) -> Mods {
        let mut mods = Mods::default();
        for path in paths {
            let modified = modified(path);
            let mut entry = Entry {
                path: path.clone(),
                manifest: None,
                status: Status::Off,
                cost: Duration::ZERO,
                run: None,
                modified,
            };
            match Package::read(path) {
                Err(e) => entry.status = Status::Failed(e),
                Ok(package) => {
                    let on = enabled(&package.manifest.id);
                    entry.manifest = Some(package.manifest.clone());
                    if on && !runs.starts(package.manifest.side) {
                        entry.status = Status::Elsewhere;
                    } else if on {
                        // Content is read before the world loads; a mod whose
                        // content is refused stays off, with the reason.
                        let content = crate::Content::read(path, &package.manifest);
                        entry.status =
                            match content.and_then(|_| mods.start(path, &package, &reserved)) {
                                Ok(run) => {
                                    entry.run = run;
                                    Status::Running
                                }
                                Err(e) => Status::Failed(e),
                            };
                    }
                }
            }
            if let (Status::Failed(reason), Some(m)) = (&entry.status, &entry.manifest) {
                log::warn!("Horizon: mod {} did not start: {reason}", m.id);
            }
            mods.entries.push(entry);
        }
        mods
    }

    fn engine(&mut self) -> Result<&(Engine, Linker<State>), String> {
        if self.engine.is_none() {
            let mut config = Config::new();
            config.wasm_component_model(true).consume_fuel(true);
            // iOS: Pulley bytecode in place of machine code, which only a JIT entitlement (or a
            // debugger attached) could run; bounds checks in place of guard pages and signal
            // handlers, and memories that grow by copying, not inside a 4 GiB reservation an
            // app's address space has no room for.
            #[cfg(target_os = "ios")]
            config
                .target("pulley64")
                .map_err(|e| e.to_string())?
                .signals_based_traps(false)
                .memory_reservation(0)
                .memory_guard_size(0);
            let engine = Engine::new(&config).map_err(|e| e.to_string())?;
            let mut linker = Linker::new(&engine);
            HorizonMod::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            players::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            mod_world::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            entities::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            // Orders are imported by mods. Their variant grows over time, but
            // Wasmtime's typed bindings require an identical case list. Decode
            // by case name so components built against earlier subsets keep
            // their own discriminants and still reach the same validation.
            linker.allow_shadowing(true);
            linker
                .instance("horizon:mods/entities@0.1.0")
                .map_err(|e| e.to_string())?
                .func_new("give-orders", |mut store, _, params, results| {
                    let result = decode_orders(params)
                        .and_then(|orders| entities::Host::give_orders(store.data_mut(), orders));
                    let [output] = results else {
                        return Err(wasmtime::Error::msg("give-orders returns one result"));
                    };
                    *output = Val::Result(
                        result
                            .map(|()| None)
                            .map_err(|error| Some(Box::new(Val::String(error)))),
                    );
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            linker.allow_shadowing(false);
            UiHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            RenderHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            RecipesHost::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s)
                .map_err(|e| e.to_string())?;
            self.engine = Some((engine, linker));
        }
        Ok(self.engine.as_ref().expect("made above"))
    }

    /// Starts a mod's code; `Ok(None)` for a content-only mod, which has
    /// nothing to run (the game reads its content before the world loads).
    /// `path` is the mod's own `.hmod` or folder, for capability `storage`'s
    /// per-mod directory.
    fn start(
        &mut self,
        path: &Path,
        package: &Package,
        reserved: &dyn Fn(&str) -> bool,
    ) -> Result<Option<Running>, String> {
        let m = &package.manifest;
        let Some(wasm) = &package.wasm else {
            return Ok(None);
        };
        // Two running mods with the same id would share one `data_dir`
        // (capability `storage`): the first loaded keeps the id.
        if self
            .entries
            .iter()
            .any(|e| e.run.is_some() && e.manifest.as_ref().is_some_and(|other| other.id == m.id))
        {
            return Err(format!("another running mod already uses id {}", m.id));
        }
        let server = m.side == Side::Server;
        let taken: Vec<String> = self
            .entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .filter(|r| r.hooks.server().is_some() == server)
            .flat_map(|r| r.hooks.commands())
            .map(str::to_owned)
            .collect();
        let snapshot = self.snapshot.clone();
        let world = self.world.clone();
        let inventories = self.inventories.clone();
        let (engine, linker) = self.engine()?;
        let component =
            Component::new(engine, wasm).map_err(|e| format!("not a component: {e}"))?;
        let ty = component.component_type();
        let imports: Vec<String> = ty
            .imports(engine)
            .map(|(name, _)| name.to_string())
            .collect();
        m.check_imports(imports.iter().map(String::as_str))?;
        // A function this Horizon doesn't have: say so, not wasmtime's linking error.
        let functions = crate::api_level::imports(wasm)?;
        let (level, function) = crate::api_level::needed(functions.iter().map(String::as_str));
        if let Some(why) = crate::api_level::too_new(level, function) {
            return Err(why);
        }
        let data_dir = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(crate::manifest::STORAGE_DIR)
            .join(&m.id);
        let state = State {
            name: m.name.clone(),
            id: m.id.clone(),
            snapshot,
            out: Vec::new(),
            panel: None,
            ui: UiState::default(),
            shapes: Vec::new(),
            corner: None,
            zoom: None,
            minimap: None,
            far_terrain: None,
            input: InputState::default(),
            browser: BrowserState::default(),
            data_dir,
            storage_budget: STORAGE_WRITE_BUDGET,
            item_spawn_budget: (None, 0),
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY)
                .instances(16)
                .build(),
            world,
            inventories,
        };
        let mut store = Store::new(engine, state);
        store.limiter(|s| &mut s.limits);
        store.set_fuel(FUEL_INIT).map_err(|e| e.to_string())?;
        let instance = linker
            .instantiate(&mut store, &component)
            .map_err(|e| reason(&e))?;
        let (mut hooks, ui) = if server {
            let exports = HorizonServerMod::new(&mut store, &instance).map_err(|e| reason(&e))?;
            let r = exports
                .horizon_mods_server_events()
                .call_init(&mut store)
                .map_err(|e| reason(&e))??;
            (Hooks::Server(exports, r), None)
        } else {
            let exports = HorizonMod::new(&mut store, &instance).map_err(|e| reason(&e))?;
            // `ui-events` is a mod built since `ui`'s; older mods have no export
            // and stay callable through `events` alone.
            let ui = UiHost::new(&mut store, &instance).ok();
            let r = exports
                .horizon_mods_events()
                .call_init(&mut store)
                .map_err(|e| reason(&e))??;
            (Hooks::Client(exports, r), ui)
        };
        let free = |c: &str| {
            let free = !reserved(c) && !taken.iter().any(|t| t == c);
            if !free {
                log::warn!("Horizon: mod {} cannot take /{c}: already a command", m.id);
            }
            free
        };
        match &mut hooks {
            Hooks::Client(_, r) => r.commands.retain(|c| free(c)),
            Hooks::Server(_, r) => {
                r.commands.retain(|c| free(&c.name));
                for c in &mut r.commands {
                    if let Err(e) = check_arguments(&c.arguments) {
                        log::warn!(
                            "Horizon: mod {}'s /{}: {e}; it takes free text",
                            m.id,
                            c.name
                        );
                        c.arguments.clear();
                    }
                }
            }
        }
        self.generation += 1;
        self.out.append(&mut store.data_mut().out);
        Ok(Some(Running {
            store,
            ui,
            hooks,
            raycasts: functions
                .iter()
                .any(|f| f == "game.raycast" || f == "game.raycast-entity"),
            entity_raycasts: functions.iter().any(|f| f == "game.raycast-entity"),
            reads_recipes: functions.iter().any(|f| f.starts_with("recipes.")),
        }))
    }

    /// True when at least one mod runs: the game skips its mod work otherwise.
    pub fn is_running(&self) -> bool {
        self.entries.iter().any(|e| e.run.is_some())
    }
    /// True when a server mod runs: the server skips its mod work otherwise.
    pub fn runs_server(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.hooks.server().is_some()))
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    /// What mods read until the next snapshot.
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = Arc::new(snapshot);
    }

    /// Calls `f` on every running mod whose hooks `wants`, with `fuel`.
    fn each(
        &mut self,
        fuel: u64,
        wants: impl Fn(&Hooks) -> bool,
        mut f: impl FnMut(&mut Running) -> wasmtime::Result<()>,
    ) {
        for i in 0..self.entries.len() {
            if self.entries[i]
                .run
                .as_ref()
                .is_some_and(|r| wants(&r.hooks))
            {
                self.call(i, fuel, &mut f);
            }
        }
    }
    /// [`Mods::each`] over the client mods.
    fn each_client(
        &mut self,
        fuel: u64,
        wants: impl Fn(&Registration) -> bool,
        mut f: impl FnMut(&ClientEvents, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        self.each(
            fuel,
            |h| h.client().is_some_and(&wants),
            |run| match &run.hooks {
                Hooks::Client(m, _) => f(m.horizon_mods_events(), &mut run.store),
                Hooks::Server(..) => Ok(()),
            },
        );
    }
    /// [`Mods::each`] over the server mods.
    fn each_server(
        &mut self,
        fuel: u64,
        wants: impl Fn(&ServerRegistration) -> bool,
        mut f: impl FnMut(&ServerEvents, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        self.each(
            fuel,
            |h| h.server().is_some_and(&wants),
            |run| match &run.hooks {
                Hooks::Server(m, _) => f(m.horizon_mods_server_events(), &mut run.store),
                Hooks::Client(..) => Ok(()),
            },
        );
    }

    /// One call into mod `i`; a trap or an overrun turns the mod off.
    fn call<R>(
        &mut self,
        i: usize,
        fuel: u64,
        f: impl FnOnce(&mut Running) -> wasmtime::Result<R>,
    ) -> Option<R> {
        let entry = &mut self.entries[i];
        let run = entry.run.as_mut()?;
        run.store.data_mut().snapshot = self.snapshot.clone();
        run.store.data_mut().storage_budget = STORAGE_WRITE_BUDGET;
        let started = Instant::now();
        let result = run.store.set_fuel(fuel).and_then(|()| f(run));
        // An average over about the last 64 calls.
        entry.cost = (entry.cost * 63 + started.elapsed()) / 64;
        let data = run.store.data_mut();
        let (open, close) = (
            std::mem::take(&mut data.ui.open),
            std::mem::take(&mut data.ui.close),
        );
        self.out.append(&mut data.out);
        if open {
            self.out.push(Output::OpenScreen { owner: i });
        }
        if close {
            self.out.push(Output::CloseScreen { owner: i });
        }
        match result {
            Ok(r) => Some(r),
            Err(e) => {
                let reason = reason(&e);
                log::warn!("Horizon: mod {} stopped: {reason}", run.store.data().id);
                let name = run.store.data().name.clone();
                self.out.push(Output::Stopped {
                    name,
                    reason: reason.clone(),
                });
                entry.status = Status::Failed(reason);
                entry.run = None;
                self.generation += 1;
                None
            }
        }
    }

    /// A game tick.
    pub fn tick(&mut self, tick: u64) {
        self.each_client(FUEL_TICK, |h| h.tick, |m, s| m.call_tick(s, tick));
    }
    /// A rendered frame, `seconds` after the last.
    pub fn frame(&mut self, seconds: f32) {
        self.each_client(FUEL_FRAME, |h| h.frame, |m, s| m.call_frame(s, seconds));
    }
    /// A chat line the player received, as plain text.
    pub fn chat(&mut self, text: &str) {
        self.each_client(FUEL_EVENT, |h| h.chat, |m, s| m.call_chat(s, text));
    }
    /// Whether a running client mod owns `/name`.
    pub fn has_command(&self, name: &str) -> bool {
        self.command_owner(false, name).is_some()
    }
    fn command_owner(&self, server: bool, name: &str) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.run.as_ref().is_some_and(|r| {
                r.hooks.server().is_some() == server && r.hooks.commands().contains(&name)
            })
        })
    }
    /// Runs `line` (without its slash) if a client mod owns its command;
    /// false when none does.
    pub fn command(&mut self, line: &str) -> bool {
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let Some(i) = self.command_owner(false, name) else {
            return false;
        };
        let result = self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Client(m, _) => m
                .horizon_mods_events()
                .call_command(&mut run.store, name, args),
            Hooks::Server(..) => Ok(Ok(None)),
        });
        match result {
            Some(Ok(Some(text))) => self.out.push(Output::Feedback { text, error: false }),
            Some(Err(text)) => self.out.push(Output::Feedback { text, error: true }),
            Some(Ok(None)) | None => {}
        }
        true
    }
    /// The commands running client mods own, for suggestions.
    pub fn commands(&self) -> impl Iterator<Item = &str> {
        self.owned_commands(false)
    }
    fn owned_commands(&self, server: bool) -> impl Iterator<Item = &str> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .filter(move |r| r.hooks.server().is_some() == server)
            .flat_map(|r| r.hooks.commands())
    }

    /// A server tick.
    pub fn server_tick(&mut self, tick: u64) {
        self.each_server(FUEL_TICK, |h| h.tick, |m, s| m.call_tick(s, tick));
    }
    /// `player` joined the server (they are in the snapshot's players).
    pub fn player_joined(&mut self, player: &ServerPlayer) {
        self.each_server(
            FUEL_EVENT,
            |h| h.joins,
            |m, s| m.call_player_joined(s, player),
        );
    }
    /// `player` left the server (they are no longer in the snapshot's players).
    pub fn player_left(&mut self, player: &ServerPlayer) {
        self.each_server(
            FUEL_EVENT,
            |h| h.joins,
            |m, s| m.call_player_left(s, player),
        );
    }
    /// A chat message `player` sent, as plain text.
    pub fn player_chat(&mut self, player: &ServerPlayer, text: &str) {
        self.each_server(FUEL_EVENT, |h| h.chat, |m, s| m.call_chat(s, player, text));
    }
    /// `player` broke `state` at `pos`.
    pub fn block_broken(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str) {
        self.each_server(
            FUEL_EVENT,
            |h| h.blocks,
            |m, s| m.call_block_broken(s, player, pos, state),
        );
    }
    /// `player` placed `state` at `pos`.
    pub fn block_placed(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str) {
        self.each_server(
            FUEL_EVENT,
            |h| h.blocks,
            |m, s| m.call_block_placed(s, player, pos, state),
        );
    }
    /// The commands running server mods own and their arguments, for the
    /// server's command tree.
    pub fn server_commands(&self) -> impl Iterator<Item = &ServerCommand> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.hooks.server())
            .flat_map(|r| r.commands.iter())
    }
    /// Runs `line` (without its slash), typed by `player`, if a server mod
    /// owns its command; its feedback is an [`Output::Reply`]. False when
    /// none does.
    pub fn server_command(&mut self, player: &ServerPlayer, line: &str) -> bool {
        let (name, args) = line.split_once(' ').unwrap_or((line, ""));
        let Some(i) = self.command_owner(true, name) else {
            return false;
        };
        let reply = |text, error| Output::Reply {
            player: player.id.clone(),
            text,
            error,
        };
        let arguments = self.entries[i]
            .run
            .as_ref()
            .and_then(|r| r.hooks.server())
            .and_then(|r| r.commands.iter().find(|c| c.name == name))
            .map(|c| c.arguments.as_slice())
            .unwrap_or_default();
        let values = match crate::commands::parse(arguments, args, player, &self.snapshot.players) {
            Ok(values) => values,
            Err(e) => {
                self.out.push(reply(e, true));
                return true;
            }
        };
        let result = self.call(i, FUEL_EVENT, |run| match &run.hooks {
            Hooks::Server(m, _) => m.horizon_mods_server_events().call_command(
                &mut run.store,
                player,
                name,
                args,
                &values,
            ),
            Hooks::Client(..) => Ok(Ok(None)),
        });
        match result {
            Some(Ok(Some(text))) => self.out.push(reply(text, false)),
            Some(Err(text)) => self.out.push(reply(text, true)),
            Some(Ok(None)) | None => {}
        }
        true
    }
    /// The running mods' HUD panels.
    pub fn panels(&self) -> impl Iterator<Item = &Panel> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().panel.as_ref())
    }
    /// The running mods' corner panels (capability `render`), in load order.
    pub fn corner_panels(&self) -> impl Iterator<Item = &CornerPanel> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().corner.as_ref())
    }
    /// Some running mod calls `game.raycast` or `game.raycast-entity`: the snapshot needs a
    /// [`Snapshot::view_hit`].
    pub fn wants_raycast(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.raycasts))
    }
    /// Some running mod reads the living entity under the crosshair.
    pub fn wants_entity_raycast(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.entity_raycasts))
    }
    /// The panel beside container screens (`browser.set-items`): the first
    /// running mod with items or recipes, its index, the panel's version and
    /// the items.
    pub fn browser_items(&self) -> Option<(usize, u64, &[BrowserStack])> {
        self.entries.iter().enumerate().find_map(|(i, e)| {
            let b = &e.run.as_ref()?.store.data().browser;
            (!b.items.is_empty() || b.recipes.is_some()).then_some((
                i,
                b.version,
                b.items.as_slice(),
            ))
        })
    }
    /// What mod `owner`'s panel shows instead of its items
    /// (`browser.set-recipes`).
    pub fn browser_recipes(&self, owner: usize) -> Option<&RecipePage> {
        let b = &self.entries.get(owner)?.run.as_ref()?.store.data().browser;
        b.recipes.as_ref()
    }
    /// The player did `event` on mod `owner`'s panel.
    pub fn browser_event(&mut self, owner: usize, event: BrowserEvent) {
        if let Some(run) = self.entries.get_mut(owner).and_then(|e| e.run.as_mut()) {
            run.store.data_mut().browser.push(event);
        }
    }
    /// The key `key` went down with the pointer on `stack` in a container
    /// screen: each mod with a key on it hears its own name for it.
    pub fn browser_key(&mut self, key: &str, stack: &BrowserStack) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let data = run.store.data_mut();
            let names: Vec<String> = data
                .input
                .keys
                .iter()
                .filter(|k| k.key == key)
                .map(|k| k.name.clone())
                .collect();
            for key in names {
                data.browser.push(BrowserEvent::Key(BrowserItemKey {
                    key,
                    stack: stack.clone(),
                }));
            }
        }
    }
    /// Some running mod reads capability `recipes`: the snapshot needs
    /// [`Snapshot::recipes`] and [`Snapshot::known_recipes`].
    pub fn wants_recipes(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_recipes))
    }
    /// Some running mod has a `frame` hook: the game refreshes the snapshot
    /// every frame for it.
    pub fn wants_frames(&self) -> bool {
        self.entries.iter().any(|e| {
            e.run
                .as_ref()
                .is_some_and(|r| r.hooks.client().is_some_and(|h| h.frame))
        })
    }
    /// Some running mod claimed items (capability `input`).
    pub fn claims_any(&self) -> bool {
        self.inputs().any(|i| !i.claimed.is_empty())
    }
    /// Some running mod claimed the item `id`.
    pub fn claims(&self, id: &str) -> bool {
        self.inputs().any(|i| i.claimed.iter().any(|c| c == id))
    }
    /// Some running mod registered a key.
    pub fn has_keys(&self) -> bool {
        self.inputs().any(|i| !i.keys.is_empty())
    }
    /// The mods' keys: (mod name, key name, label, key), for Controls.
    pub fn mod_keys(&self) -> impl Iterator<Item = (&str, &ModKey)> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| {
                let d = r.store.data();
                d.input.keys.iter().map(move |k| (d.name.as_str(), k))
            })
    }
    /// Mod `i`'s keys, as it registered them (with the player's choices).
    pub fn keys_of(&self, i: usize) -> &[ModKey] {
        self.entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .map_or(&[], |r| r.store.data().input.keys.as_slice())
    }
    /// The keys the player chose (`ui`'s key widget), by mod id: each
    /// running mod's `name → key`. They replace the defaults registered
    /// so far and later ones. Keys that aren't keys are skipped.
    pub fn set_bindings<'a>(&mut self, chosen: impl Fn(&str) -> Vec<(&'a str, &'a str)>) {
        for e in &mut self.entries {
            let (Some(m), Some(run)) = (&e.manifest, e.run.as_mut()) else {
                continue;
            };
            let input = &mut run.store.data_mut().input;
            input.bindings = chosen(&m.id)
                .into_iter()
                .filter(|(_, key)| key_ok(key))
                .map(|(n, k)| (n.to_owned(), k.to_owned()))
                .collect();
            for (name, key) in input.bindings.clone() {
                input.rebind(&name, key);
            }
        }
    }
    /// The player binds mod `i`'s key `name` to `key` (`key.keyboard.x`,
    /// `key.mouse.4`, `key.keyboard.unknown` for none). False when the mod
    /// has no such key or `key` isn't one.
    pub fn rebind(&mut self, i: usize, name: &str, key: &str) -> bool {
        let Some(run) = self.entries.get_mut(i).and_then(|e| e.run.as_mut()) else {
            return false;
        };
        let input = &mut run.store.data_mut().input;
        if !key_ok(key) || !input.keys.iter().any(|k| k.name == name) {
            return false;
        }
        input.bindings.retain(|(n, _)| n != name);
        input.bindings.push((name.to_owned(), key.to_owned()));
        input.rebind(name, key.to_owned());
        true
    }
    /// The use button went down with `item`: `use-start` to each mod that
    /// claimed it.
    pub fn use_start(&mut self, item: &HeldItem) {
        let event = InputEvent::UseStart(ItemUse {
            item: item.clone(),
            ticks: 0,
        });
        self.push_input(|i| i.claimed.contains(&item.id), &event);
    }
    /// The claimed use ended after `ticks`.
    pub fn use_stop(&mut self, item: &HeldItem, ticks: u32) {
        let event = InputEvent::UseStop(ItemUse {
            item: item.clone(),
            ticks,
        });
        self.push_input(|i| i.claimed.contains(&item.id), &event);
    }
    /// The key `key` (`key.keyboard.r`) went down or up: each mod with a
    /// key on it hears its own name for it.
    pub fn key(&mut self, key: &str, down: bool) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            let names: Vec<String> = input
                .keys
                .iter()
                .filter(|k| k.key == key)
                .map(|k| k.name.clone())
                .collect();
            for name in names {
                let held = input.down.iter().position(|n| *n == name);
                match (down, held) {
                    (true, None) => input.down.push(name.clone()),
                    (false, Some(i)) => {
                        input.down.swap_remove(i);
                    }
                    // Already let go when a screen opened.
                    (false, None) => continue,
                    (true, Some(_)) => {}
                }
                input.push(if down {
                    InputEvent::KeyDown(name)
                } else {
                    InputEvent::KeyUp(name)
                });
            }
        }
    }
    /// A screen opened, or the game stopped playing: every mod key held
    /// lets go (`key-up`), so a key needs a new press afterwards.
    pub fn release_keys(&mut self) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            for name in std::mem::take(&mut input.down) {
                input.push(InputEvent::KeyUp(name));
            }
        }
    }
    /// Some running mod holds the mouse wheel.
    pub fn wheel_claimed(&self) -> bool {
        self.inputs().any(|i| i.wheel)
    }
    /// Wheel steps while playing: `wheel` to the mods holding it; false
    /// when none does (the hotbar takes them).
    pub fn wheel(&mut self, steps: f32) -> bool {
        let event = InputEvent::Wheel(steps);
        let claimed = self.wheel_claimed();
        self.push_input(|i| i.wheel, &event);
        claimed
    }
    /// The strongest zoom a running mod asks for (capability `render`).
    pub fn zoom(&self) -> Option<ModZoom> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref()?.store.data().zoom)
            .max_by(|a, b| a.factor.total_cmp(&b.factor))
    }
    /// The minimap the first running mod shows (capability `render`).
    pub fn minimap(&self) -> Option<ModMinimap> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().minimap)
    }
    /// The far terrain the first running mod draws (capability `render`).
    pub fn far_terrain(&self) -> Option<ModFarTerrain> {
        self.entries
            .iter()
            .find_map(|e| e.run.as_ref()?.store.data().far_terrain)
    }
    fn inputs(&self) -> impl Iterator<Item = &InputState> {
        self.entries
            .iter()
            .filter_map(|e| Some(&e.run.as_ref()?.store.data().input))
    }
    fn push_input(&mut self, to: impl Fn(&InputState) -> bool, event: &InputEvent) {
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            let input = &mut run.store.data_mut().input;
            if to(input) {
                input.push(event.clone());
            }
        }
    }
    /// The running mods' HUD shapes (capability `render`), each mod's in
    /// its own order, mods in load order.
    pub fn hud_shapes(&self) -> impl Iterator<Item = &Shape> {
        self.entries
            .iter()
            .filter_map(|e| e.run.as_ref())
            .flat_map(|r| &r.store.data().shapes)
    }
    /// The screen mod `i` last pushed with `ui.open-screen`, while it is up.
    pub fn screen(&self, i: usize) -> Option<&UiScreen> {
        self.entries
            .get(i)?
            .run
            .as_ref()?
            .store
            .data()
            .ui
            .screen
            .as_ref()
    }
    /// The widgets `ui.set-settings` pushed: mod `i`'s page in
    /// Options > Mods. Empty while it has none.
    pub fn settings_widgets(&self, i: usize) -> &[UiWidget] {
        self.entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .map_or(&[], |r| &r.store.data().ui.settings)
    }
    /// A `list` widget's row was picked on a surface of mod `i`.
    pub fn ui_select(&mut self, i: usize, id: u32, row: u32) {
        self.ui_event(i, |ui, s| {
            ui.horizon_mods_ui_events().call_select(s, id, row)
        });
    }
    /// The player left mod `i`'s screen (Esc or Done).
    pub fn ui_closed(&mut self, i: usize) {
        self.ui_event(i, |ui, s| ui.horizon_mods_ui_events().call_closed(s));
    }
    /// The player opened mod `i`'s settings page.
    pub fn ui_settings_opened(&mut self, i: usize) {
        self.ui_event(i, |ui, s| {
            ui.horizon_mods_ui_events().call_settings_opened(s)
        });
    }
    /// A `ui-events` call on mod `i`; no export, no call (mods built
    /// before `ui` have none).
    fn ui_event(
        &mut self,
        i: usize,
        f: impl FnOnce(&UiHost, &mut Store<State>) -> wasmtime::Result<()>,
    ) {
        let has_ui = self
            .entries
            .get(i)
            .and_then(|e| e.run.as_ref())
            .is_some_and(|r| r.ui.is_some());
        if has_ui {
            self.call(i, FUEL_EVENT, |run| {
                f(run.ui.as_ref().expect("checked above"), &mut run.store)
            });
        }
    }
    /// What mods asked to show since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.out)
    }

    /// Restarts the mods whose file changed since they loaded (mod
    /// development: rebuild, and the game picks it up). True when any did.
    pub fn reload_changed(&mut self, reserved: impl Fn(&str) -> bool) -> bool {
        let mut any = false;
        for i in 0..self.entries.len() {
            let entry = &self.entries[i];
            let now = modified(&entry.path);
            let was_on = !matches!(entry.status, Status::Off | Status::Elsewhere);
            if now == entry.modified || !was_on {
                continue;
            }
            any = true;
            self.generation += 1;
            self.entries[i].modified = now;
            self.entries[i].run = None;
            let path = self.entries[i].path.clone();
            let started = Package::read(&path).and_then(|p| {
                self.entries[i].manifest = Some(p.manifest.clone());
                self.start(&path, &p, &reserved)
            });
            let entry = &mut self.entries[i];
            let id = entry.manifest.as_ref().map_or("?", |m| m.id.as_str());
            match started {
                Ok(run) => {
                    log::info!("Horizon: mod {id} reloaded");
                    entry.run = run;
                    entry.status = Status::Running;
                }
                Err(e) => {
                    log::warn!("Horizon: mod {id} did not reload: {e}");
                    entry.status = Status::Failed(e);
                }
            }
        }
        any
    }
}

/// A command's arguments are well formed: names given and distinct,
/// optional ones last, free text only last.
fn check_arguments(arguments: &[Argument]) -> Result<(), String> {
    for (i, a) in arguments.iter().enumerate() {
        if a.name.is_empty()
            || !a
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            return Err(format!("argument name '{}' isn't [A-Za-z0-9_]+", a.name));
        }
        if arguments[..i].iter().any(|b| b.name == a.name) {
            return Err(format!("two arguments are named {}", a.name));
        }
        if i > 0 && arguments[i - 1].optional && !a.optional {
            return Err(format!("{} is required after an optional argument", a.name));
        }
        if a.kind == ArgumentKind::Text && i + 1 < arguments.len() {
            return Err(format!(
                "{} takes the rest of the line, so it must be last",
                a.name
            ));
        }
    }
    Ok(())
}

fn modified(path: &Path) -> Option<SystemTime> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.is_dir() {
        // A folder mod changes when its component does.
        return std::fs::metadata(path.join(crate::manifest::WASM))
            .ok()?
            .modified()
            .ok();
    }
    meta.modified().ok()
}

/// A trap or error, as the player reads it.
fn reason(e: &wasmtime::Error) -> String {
    match e.downcast_ref::<Trap>() {
        Some(Trap::OutOfFuel) => "it took too long (an endless loop?)".into(),
        Some(Trap::UnreachableCodeReached) => "it crashed (a panic)".into(),
        Some(trap) => format!("it crashed: {trap}"),
        None => format!("{e:#}"),
    }
}

fn clip(text: String) -> String {
    clip_to(text, LINE_CHARS)
}
fn clip_to(text: String, chars: usize) -> String {
    match text.char_indices().nth(chars) {
        Some((at, _)) => text[..at].to_string(),
        None => text,
    }
}

/// Widgets on a `ui` screen or settings page, and rows on one `list`.
const WIDGETS: usize = 64;
const LIST_ROWS: usize = 512;

fn clip_widgets(widgets: Vec<UiWidget>) -> Vec<UiWidget> {
    widgets
        .into_iter()
        .take(WIDGETS)
        .map(|w| match w {
            UiWidget::Text(mut t) => {
                t.text = clip(t.text);
                UiWidget::Text(t)
            }
            UiWidget::Button(mut b) => {
                b.label = clip(b.label);
                UiWidget::Button(b)
            }
            UiWidget::Item(mut i) => {
                i.item = clip(i.item);
                i.name = clip(i.name);
                UiWidget::Item(i)
            }
            UiWidget::List(mut l) => {
                l.rows = l.rows.into_iter().take(LIST_ROWS).map(clip).collect();
                l.selected = l.selected.filter(|&s| (s as usize) < l.rows.len());
                UiWidget::List(l)
            }
        })
        .collect()
}

impl mod_log::Host for State {
    fn write(&mut self, level: mod_log::Level, message: String) {
        let level = match level {
            mod_log::Level::Debug => log::Level::Debug,
            mod_log::Level::Info => log::Level::Info,
            mod_log::Level::Warn => log::Level::Warn,
            mod_log::Level::Error => log::Level::Error,
        };
        log::log!(level, "[mod {}] {}", self.id, clip(message));
    }
}
impl game::Host for State {
    fn get_player(&mut self) -> Option<Player> {
        self.snapshot.player.clone()
    }
    fn get_clock(&mut self) -> Option<Clock> {
        self.snapshot.clock
    }
    fn raycast(&mut self, distance: f32) -> Option<Hit> {
        let (at, hit) = self.snapshot.view_hit.as_ref()?;
        // NaN compares false: no hit.
        (*at <= distance.min(RAYCAST_MAX) && !distance.is_nan()).then(|| hit.clone())
    }
    fn raycast_entity(&mut self, distance: f32) -> Option<EntityHit> {
        let (at, hit) = self.snapshot.view_entity.as_ref()?;
        (*at <= distance.min(RAYCAST_MAX)
            && !distance.is_nan()
            && self
                .snapshot
                .view_hit
                .as_ref()
                .is_none_or(|(block, _)| at < block))
        .then(|| hit.clone())
    }
}
impl browser::Host for State {
    fn set_items(&mut self, items: Vec<BrowserStack>) {
        self.browser.items = items
            .into_iter()
            .take(BROWSER_ITEMS)
            .map(|s| BrowserStack {
                item: clip(s.item),
                components: clip_to(s.components, 4096),
                ..s
            })
            .collect();
        self.browser.version = BROWSER_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn set_recipes(
        &mut self,
        title: String,
        tabs: Vec<BrowserStack>,
        tab: u32,
        recipes: Vec<BrowserRecipe>,
    ) {
        let stack = |s: BrowserStack| BrowserStack {
            item: clip(s.item),
            components: clip_to(s.components, 4096),
            ..s
        };
        let cycle = |list: Vec<BrowserStack>| -> Vec<BrowserStack> {
            list.into_iter().take(SLOT_STACKS).map(stack).collect()
        };
        self.browser.recipes = (!recipes.is_empty()).then(|| {
            let recipes = recipes
                .into_iter()
                .take(BROWSER_RECIPES)
                .map(|r| BrowserRecipe {
                    width: r.width.clamp(1, 3),
                    height: r.height.clamp(1, 3),
                    slots: r.slots.into_iter().take(RECIPE_SLOTS).map(cycle).collect(),
                    results: cycle(r.results),
                    note: clip(r.note),
                    ..r
                })
                .collect();
            RecipePage {
                title: clip(title),
                tabs: tabs.into_iter().take(RECIPE_TABS).map(stack).collect(),
                tab,
                recipes,
            }
        });
        self.browser.version = BROWSER_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn poll(&mut self) -> Vec<BrowserEvent> {
        self.browser.events.drain(..).collect()
    }
}
impl recipes::Host for State {
    fn items(&mut self) -> Vec<RecipeItem> {
        self.snapshot
            .recipes
            .as_ref()
            .map_or_else(Vec::new, |r| r.items.clone())
    }
    fn all(&mut self) -> Vec<Recipe> {
        self.snapshot
            .recipes
            .as_ref()
            .map_or_else(Vec::new, |r| r.recipes.clone())
    }
    fn known(&mut self) -> Vec<i32> {
        self.snapshot.known_recipes.to_vec()
    }
    fn place(&mut self, book_id: i32, all: bool) -> Result<(), String> {
        self.recipe_action()?;
        if !self.snapshot.placing {
            return Err("no screen that takes recipes is open".into());
        }
        if !self.snapshot.known_recipes.contains(&book_id) {
            return Err(format!(
                "recipe {book_id} is not in the player's recipe book"
            ));
        }
        self.out.push(Output::PlaceRecipe { book_id, all });
        Ok(())
    }
    fn give(&mut self, stack: RecipeStack) -> Result<(), String> {
        self.recipe_action()?;
        if !self.snapshot.creative {
            return Err("only in creative mode".into());
        }
        let item = self
            .snapshot
            .recipes
            .as_ref()
            .and_then(|r| r.items.get(stack.item as usize))
            .ok_or_else(|| format!("no item {}", stack.item))?
            .id
            .clone();
        self.out.push(Output::Give {
            item,
            count: stack.count.clamp(1, 99),
            components: clip_to(stack.components, 4096),
        });
        Ok(())
    }
}
impl State {
    /// `recipes.place` and `give` take at most [`RECIPE_ACTIONS`] a call.
    fn recipe_action(&self) -> Result<(), String> {
        let done = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::PlaceRecipe { .. } | Output::Give { .. }))
            .count();
        if done < RECIPE_ACTIONS {
            Ok(())
        } else {
            Err(format!("at most {RECIPE_ACTIONS} places and gives a call"))
        }
    }
}
impl State {
    /// Queues a chat line unless the call already sent [`CHAT_PER_CALL`].
    fn say(&mut self, line: impl FnOnce(String) -> Output) -> bool {
        let shown = self
            .out
            .iter()
            .filter(|o| {
                matches!(
                    o,
                    Output::Chat { .. } | Output::Tell { .. } | Output::Broadcast { .. }
                )
            })
            .count();
        if shown < CHAT_PER_CALL {
            let name = self.name.clone();
            self.out.push(line(name));
        }
        shown < CHAT_PER_CALL
    }
}
impl chat::Host for State {
    fn show(&mut self, text: String) {
        self.say(|name| Output::Chat {
            name,
            text: clip(text),
        });
    }
}
impl mod_world::Host for State {
    fn spawn_item(
        &mut self,
        dimension: String,
        at: WorldPoint,
        item: String,
        count: u32,
    ) -> Result<(), String> {
        let at = EntityPoint {
            x: at.x,
            y: at.y,
            z: at.z,
        };
        if !finite(&at) {
            return Err("spawn-item needs a finite point".into());
        }
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let view = view.as_ref().ok_or("world unavailable")?;
        if !view.has_dimension(&dimension) {
            return Err(format!("no dimension {dimension}"));
        }
        view.check_item_stack(&item, count)?;
        let tick = self.snapshot.clock.as_ref().map(|c| c.game_time);
        if self.item_spawn_budget.0 != tick {
            self.item_spawn_budget = (tick, 0);
        }
        if self.item_spawn_budget.1 >= 16 {
            return Err("at most 16 item spawns per tick per mod".into());
        }
        self.item_spawn_budget.1 += 1;
        self.out.push(Output::SpawnItem {
            dimension,
            at,
            item,
            count,
        });
        Ok(())
    }
    fn block_has_tag(&mut self, block: String, tag: String) -> bool {
        self.world
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|v| v.block_has_tag(&block, &tag)))
            .unwrap_or(false)
    }
    fn play_sound(
        &mut self,
        dimension: String,
        at: WorldPoint,
        name: String,
        volume: f32,
        pitch: f32,
    ) -> Result<(), String> {
        let at = EntityPoint {
            x: at.x,
            y: at.y,
            z: at.z,
        };
        if !finite(&at) {
            return Err("sound needs a finite point".into());
        }
        let sound = EntitySound {
            name,
            volume,
            pitch,
        };
        let order = EntityOrder::Sound(sound.clone());
        check_order(&order)?;
        check_order_names(&order, &self.snapshot)?;
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        if !view
            .as_ref()
            .ok_or("world unavailable")?
            .has_dimension(&dimension)
        {
            return Err(format!("no dimension {dimension}"));
        }
        self.out.push(Output::PlaySound {
            dimension,
            at,
            sound,
        });
        Ok(())
    }
    fn container(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
    ) -> Option<Vec<Option<ContainerItemStack>>> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.container(&dimension, x, y, z)
    }
    fn set_container(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        slots: Vec<Option<ContainerItemStack>>,
    ) -> Result<(), String> {
        if slots.len() > CONTAINER_SLOTS {
            return Err(format!("at most {CONTAINER_SLOTS} container slots"));
        }
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let view = view.as_ref().ok_or("world unavailable")?;
        if !view.has_dimension(&dimension) {
            return Err(format!("no dimension {dimension}"));
        }
        if let Some(current) = view.container(&dimension, x, y, z)
            && current.len() != slots.len()
        {
            return Err(format!(
                "container has {} slots, got {}",
                current.len(),
                slots.len()
            ));
        }
        for stack in slots.iter().flatten() {
            view.check_item_stack(&stack.item, stack.count)?;
        }
        self.out.push(Output::SetContainer {
            dimension,
            pos: (x, y, z),
            slots,
        });
        Ok(())
    }
    fn place_structure(
        &mut self,
        dimension: String,
        id: String,
        x: i32,
        y: i32,
        z: i32,
        rotation: u32,
        mirror: bool,
    ) -> Result<(), String> {
        if rotation > 3 {
            return Err("rotation is 0 to 3 quarter turns".into());
        }
        if !resource_id(&id) {
            return Err(format!("{id:?} is not a structure template id"));
        }
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let view = view.as_ref().ok_or("world unavailable")?;
        if !view.has_dimension(&dimension) {
            return Err(format!("no dimension {dimension}"));
        }
        view.check_structure(&id)?;
        self.out.push(Output::PlaceStructure {
            dimension,
            id,
            pos: (x, y, z),
            rotation,
            mirror,
        });
        Ok(())
    }
    fn set_spawner(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        entity: String,
    ) -> Result<(), String> {
        if !resource_id(&entity) {
            return Err(format!("{entity:?} is not an entity type id"));
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref())
            && !view.has_dimension(&dimension)
        {
            return Err(format!("no dimension {dimension}"));
        }
        self.out.push(Output::SetSpawner {
            dimension,
            pos: (x, y, z),
            entity,
        });
        Ok(())
    }
    fn time(&mut self, dimension: String) -> WorldTime {
        self.snapshot
            .world_times
            .get(&dimension)
            .cloned()
            .unwrap_or(WorldTime {
                day_time: 0,
                game_time: 0,
                is_day: false,
                raining: false,
                thundering: false,
            })
    }
    fn can_see_sky(&mut self, dimension: String, x: i32, y: i32, z: i32) -> bool {
        self.world
            .lock()
            .ok()
            .and_then(|v| v.as_ref().map(|v| v.can_see_sky(&dimension, x, y, z)))
            .unwrap_or(false)
    }
    fn game_rule(&mut self, name: String) -> Option<String> {
        let name = name.strip_prefix("minecraft:").unwrap_or(&name);
        let name = if name == "mobGriefing" {
            "mob_griefing"
        } else {
            name
        };
        self.snapshot.game_rules.get(name).cloned()
    }
    fn difficulty(&mut self) -> String {
        self.snapshot.difficulty.clone()
    }
    fn get_block(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<String> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.block(&dimension, x, y, z)
    }
    fn generated_chunks(&mut self) -> Vec<ChunkPos> {
        (self.snapshot.generated_chunks.iter())
            .take(GENERATED_CHUNKS)
            .cloned()
            .collect()
    }
    fn set_loot_table(
        &mut self,
        dimension: String,
        x: i32,
        y: i32,
        z: i32,
        table: String,
    ) -> Result<(), String> {
        let valid = table.split_once(':').is_some_and(|(ns, path)| {
            !ns.is_empty()
                && !path.is_empty()
                && table
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./:".contains(c))
        });
        if !valid {
            return Err(format!("{table:?} is not a loot table id"));
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref())
            && !view.has_dimension(&dimension)
        {
            return Err(format!("no dimension {dimension}"));
        }
        self.out.push(Output::SetLootTable {
            dimension,
            pos: (x, y, z),
            table,
        });
        Ok(())
    }
    fn get_biome(&mut self, dimension: String, x: i32, y: i32, z: i32) -> Option<String> {
        let view = self.world.lock().ok()?;
        view.as_ref()?.biome(&dimension, x, y, z)
    }
    fn set_blocks(&mut self, dimension: String, blocks: Vec<BlockChange>) -> Result<(), String> {
        if blocks.len() > BLOCKS_PER_CALL {
            return Err(format!(
                "{} blocks: at most {BLOCKS_PER_CALL} per call",
                blocks.len()
            ));
        }
        if let Some(view) = self.world.lock().ok().as_ref().and_then(|v| v.as_ref()) {
            if !view.has_dimension(&dimension) {
                return Err(format!("no dimension {dimension}"));
            }
            for b in &blocks {
                view.check_state(&b.state)?;
            }
        }
        if !blocks.is_empty() {
            self.out.push(Output::SetBlocks {
                name: self.name.clone(),
                dimension,
                blocks,
            });
        }
        Ok(())
    }
}
/// Decode self-describing component values without assuming a variant's
/// numeric discriminants. The derived Rust records provide the field/type
/// checks, while the normal host implementation validates names and bounds.
fn decode_orders(params: &[Val]) -> Result<Vec<(u32, EntityOrder)>, String> {
    let [Val::List(orders)] = params else {
        return Err("give-orders expects a list of (mob id, order)".into());
    };
    if orders.len() > ORDERS_PER_CALL {
        return Err(format!(
            "{} orders: at most {ORDERS_PER_CALL} per call",
            orders.len()
        ));
    }
    let value = component_json(&params[0], 0)?;
    serde_json::from_value(value).map_err(|error| format!("invalid entity orders: {error}"))
}
fn component_json(value: &Val, depth: usize) -> Result<serde_json::Value, String> {
    use serde_json::Value as Json;
    if depth > 16 {
        return Err("entity order value is too deeply nested".into());
    }
    let child = |value: &Val| component_json(value, depth + 1);
    let case_name = |name: &str| {
        name.split('-')
            .map(|part| {
                let mut chars = part.chars();
                chars.next().map_or_else(String::new, |first| {
                    first.to_uppercase().chain(chars).collect()
                })
            })
            .collect::<String>()
    };
    Ok(match value {
        Val::Bool(value) => Json::Bool(*value),
        Val::U8(value) => (*value).into(),
        Val::U32(value) => (*value).into(),
        Val::Float32(value) if value.is_finite() => Json::from(*value),
        Val::Float64(value) if value.is_finite() => Json::from(*value),
        Val::String(value) => Json::String(value.clone()),
        Val::List(values) | Val::Tuple(values) => {
            Json::Array(values.iter().map(child).collect::<Result<_, _>>()?)
        }
        Val::Record(fields) => Json::Object(
            fields
                .iter()
                .map(|(name, value)| child(value).map(|value| (name.replace('-', "_"), value)))
                .collect::<Result<_, _>>()?,
        ),
        Val::Variant(name, None) | Val::Enum(name) => Json::String(case_name(name)),
        Val::Variant(name, Some(value)) => {
            Json::Object([(case_name(name), child(value)?)].into_iter().collect())
        }
        Val::Option(None) => Json::Null,
        Val::Option(Some(value)) => child(value)?,
        _ => return Err("invalid or nonfinite entity order value".into()),
    })
}

/// A namespaced registry id with no path traversal or extra separators.
fn resource_id(id: &str) -> bool {
    id.len() <= 256
        && id.split_once(':').is_some_and(|(namespace, path)| {
            !namespace.is_empty()
                && namespace != "."
                && namespace != ".."
                && !path.is_empty()
                && namespace
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && path
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
                && path
                    .split('/')
                    .all(|part| !part.is_empty() && part != "." && part != "..")
        })
}
fn check_effect(effect: &EffectChange, snapshot: &Snapshot) -> Result<(), String> {
    if !snapshot.effect_ids.contains(&effect.id) {
        return Err(format!("unknown status effect {}", effect.id));
    }
    // The game counts durations in signed ticks, twenty per second.
    if effect.seconds > i32::MAX as u32 / 20 {
        return Err("effect duration exceeds the game's tick range".into());
    }
    Ok(())
}
/// A content mod's kind: any namespace but vanilla's.
fn own_kind(kind: &str) -> bool {
    !kind.starts_with("minecraft:")
}
impl From<EntityPoint> for WorldPoint {
    fn from(p: EntityPoint) -> Self {
        Self {
            x: p.x,
            y: p.y,
            z: p.z,
        }
    }
}
impl From<EntityPoint> for PlayerPoint {
    fn from(p: EntityPoint) -> Self {
        Self {
            x: p.x,
            y: p.y,
            z: p.z,
        }
    }
}
fn finite(p: &EntityPoint) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}
/// Why `set-data` (or `spawn`'s data) refuses its entries, if it does.
fn check_data<'a>(
    entries: impl ExactSizeIterator<Item = (&'a str, Option<&'a str>)>,
) -> Result<(), String> {
    if entries.len() > DATA_KEYS {
        return Err(format!("at most {DATA_KEYS} data keys"));
    }
    for (key, value) in entries {
        if key.is_empty() || key.chars().count() > DATA_KEY_CHARS {
            return Err(format!("a data key is 1 to {DATA_KEY_CHARS} characters"));
        }
        if value.is_some_and(|v| v.chars().count() > DATA_VALUE_CHARS) {
            return Err(format!("data {key}: at most {DATA_VALUE_CHARS} characters"));
        }
    }
    Ok(())
}
/// Why `entities.give-orders` refuses an order, if it does.
fn check_order(order: &EntityOrder) -> Result<(), String> {
    match order {
        EntityOrder::Attribute(a)
            if crate::content::TYPE_ONLY_ATTRIBUTES.contains(&a.name.as_str())
                || !crate::content::ENTITY_ATTRIBUTES.contains(&a.name.as_str())
                || !(0.0..=1024.0).contains(&a.value) =>
        {
            Err(
                "attribute names an entity JSON attribute (except scale and the fall ones), with a base of 0 to 1024"
                    .into(),
            )
        }
        EntityOrder::Mount(m)
            if m.player.is_empty()
                || m.player.len() > 64
                || !(-16.0..=64.0).contains(&m.height)
                || !(-32.0..=32.0).contains(&m.forward) =>
        {
            Err("mount names a player UUID, a height of -16 to 64 and forward of -32 to 32".into())
        }
        EntityOrder::Heal(amount) if !amount.is_finite() => {
            Err("heal needs a finite amount".into())
        }
        EntityOrder::Scale(s) if !(0.0625..=16.0).contains(s) => {
            Err("scale is 0.0625 to 16".into())
        }
        EntityOrder::SetData(entries) => {
            check_data(entries.iter().map(|(k, v)| (k.as_str(), v.as_deref())))
        }
        EntityOrder::WalkTo(w) if !finite(&w.to) || !(0.0..=4.0).contains(&w.speed) => {
            Err("walk-to needs a point and a speed of 0 to 4".into())
        }
        EntityOrder::LookAt(p) if !finite(p) => Err("look-at needs a point".into()),
        EntityOrder::Push(p) if ![p.x, p.y, p.z].iter().all(|v| (-4.0..=4.0).contains(v)) => {
            Err("push is -4 to 4 blocks per tick on each axis".into())
        }
        EntityOrder::Sound(s)
            if !s.volume.is_finite() || s.volume < 0.0 || !s.pitch.is_finite() || s.pitch < 0.0 =>
        {
            Err("sound needs a finite, nonnegative volume and pitch".into())
        }
        EntityOrder::Particles(p)
            if !finite(&p.at)
                || !finite(&p.delta)
                || ![p.delta.x, p.delta.y, p.delta.z]
                    .iter()
                    .all(|d| (*d as f32).is_finite())
                || !p.speed.is_finite()
                || p.speed < 0.0
                || p.count > 256 =>
        {
            Err("particles need finite points and a nonnegative speed, count at most 256".into())
        }
        EntityOrder::Texture(Some(name)) if !crate::content::entity_texture(name) => {
            Err("texture is <namespace>:entity/<path>".into())
        }
        EntityOrder::Animate(Some(name)) if name.is_empty() || name.len() > 256 => {
            Err("animate names an animation of 1 to 256 characters".into())
        }
        EntityOrder::HurtArea(a)
            if !finite(&a.center)
                || !(0.0..=AREA_RADIUS).contains(&a.radius)
                || !(0.0..=1024.0).contains(&a.amount)
                || a.fire > 30 =>
        {
            Err("hurt-area is a radius up to 16, up to 1024 hit points, up to 30 s of fire".into())
        }
        _ => Ok(()),
    }
}
fn check_order_names(order: &EntityOrder, snapshot: &Snapshot) -> Result<(), String> {
    let resources = &snapshot.entity_resources;
    match order {
        EntityOrder::Effect(effect) => check_effect(effect, snapshot),
        EntityOrder::Target(EntityTarget::Player(id))
            if !snapshot.players.iter().any(|p| p.id == *id) =>
        {
            Err(format!("no online player {id}"))
        }
        EntityOrder::Texture(Some(name)) if !resources.textures.contains(name) => {
            Err(format!("no loaded mod entity texture {name}"))
        }
        EntityOrder::Sound(s) if !resources.sounds.contains(&s.name) => {
            Err(format!("unknown sound event {}", s.name))
        }
        EntityOrder::Particles(p) if !resources.particles.contains_key(&p.name) => {
            Err(format!("unknown particle type without options {}", p.name))
        }
        _ => Ok(()),
    }
}
impl entities::Host for State {
    fn hurt_events(&mut self) -> Vec<HurtEvent> {
        self.snapshot
            .hurt_events
            .iter()
            .take(256)
            .cloned()
            .collect()
    }
    fn deaths(&mut self) -> Vec<Death> {
        self.snapshot.deaths.iter().take(256).cloned().collect()
    }
    fn effect(&mut self, mob: u32, change: EffectChange) -> Result<(), String> {
        if !self.snapshot.entities.iter().any(|e| e.id == mob) {
            return Err(format!("no mob {mob} is loaded"));
        }
        check_effect(&change, &self.snapshot)?;
        self.out.push(Output::MobEffect { mob, change });
        Ok(())
    }
    fn push(&mut self, mob: u32, velocity: EntityPoint) -> Result<(), String> {
        if !self.snapshot.entities.iter().any(|e| e.id == mob) {
            return Err(format!("no mob {mob} is loaded"));
        }
        check_order(&EntityOrder::Push(velocity))?;
        self.out.push(Output::MobPush { mob, velocity });
        Ok(())
    }
    fn tame(&mut self, mob: u32, owner: Option<String>) -> Result<(), String> {
        if !self.snapshot.entities.iter().any(|e| e.id == mob) {
            return Err(format!("no mob {mob} is loaded"));
        }
        if !self.snapshot.tamable_mobs.contains(&mob) {
            return Err(format!("mob {mob} cannot be tamed"));
        }
        if let Some(id) = &owner
            && (id.len() != 36
                || [8, 13, 18, 23].iter().any(|&i| id.as_bytes()[i] != b'-')
                || u128::from_str_radix(&id.replace('-', ""), 16).is_err())
        {
            return Err("owner must be a UUID".into());
        }
        self.out.push(Output::Tame { mob, owner });
        Ok(())
    }
    fn strike(&mut self, mob: u32, hit: StrikeHit) -> Result<(), String> {
        let e = self
            .snapshot
            .entities
            .iter()
            .find(|e| e.id == mob)
            .ok_or_else(|| format!("no mob {mob} is loaded"))?;
        if !own_kind(&e.kind) {
            return Err(format!("mob {mob} is not a content mod's"));
        }
        check_order(&EntityOrder::HurtArea(EntityArea {
            center: hit.center,
            radius: hit.radius,
            amount: hit.amount,
            fire: hit.fire,
        }))?;
        if !(0.0..=4.0).contains(&hit.knockback)
            || hit.skip_kinds.len() > 256
            || hit.skip_kinds.iter().any(|k| !resource_id(k))
        {
            return Err("strike needs knockback 0..4 and at most 256 entity type ids".into());
        }
        if let Some(id) = &hit.credit_player {
            self.online_player(id)?;
        }
        self.out.push(Output::Strike { mob, hit });
        Ok(())
    }
    fn mobs(&mut self) -> Vec<ModEntity> {
        let own = self.snapshot.entities.iter().filter(|e| own_kind(&e.kind));
        own.take(OWN_ENTITIES).cloned().collect()
    }
    fn interactions(&mut self) -> Vec<EntityInteraction> {
        self.snapshot
            .interactions
            .iter()
            .take(NEAR_ENTITIES)
            .cloned()
            .collect()
    }
    fn near(&mut self, dimension: String, at: EntityPoint, radius: f64) -> Vec<ModEntity> {
        if !finite(&at) || !radius.is_finite() {
            return Vec::new();
        }
        let r = radius.clamp(0.0, NEAR_RADIUS);
        let mut near: Vec<(f64, &ModEntity)> = self
            .snapshot
            .entities
            .iter()
            .filter(|e| e.dimension == dimension)
            .map(|e| {
                let d = (e.x - at.x).powi(2) + (e.y - at.y).powi(2) + (e.z - at.z).powi(2);
                (d, e)
            })
            .filter(|(d, _)| *d <= r * r)
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0));
        near.into_iter()
            .take(NEAR_ENTITIES)
            .map(|(_, e)| e.clone())
            .collect()
    }
    fn give_orders(&mut self, orders: Vec<(u32, EntityOrder)>) -> Result<(), String> {
        if orders.len() > ORDERS_PER_CALL {
            return Err(format!(
                "{} orders: at most {ORDERS_PER_CALL} per call",
                orders.len()
            ));
        }
        for (id, order) in &orders {
            let e = self.snapshot.entities.iter().find(|e| e.id == *id);
            match e {
                None => return Err(format!("no mob {id} is loaded")),
                Some(e) if !own_kind(&e.kind) => {
                    return Err(format!("mob {id} is {}, not a content mod's", e.kind));
                }
                Some(_) => {
                    check_order(order).map_err(|e| format!("mob {id}: {e}"))?;
                    check_order_names(order, &self.snapshot)
                        .map_err(|e| format!("mob {id}: {e}"))?;
                }
            }
        }
        if !orders.is_empty() {
            self.out.push(Output::Orders {
                name: self.name.clone(),
                orders,
            });
        }
        Ok(())
    }
    fn ride(&mut self, rider: u32, vehicle: Option<u32>) -> Result<(), String> {
        let find = |id: u32| self.snapshot.entities.iter().find(|e| e.id == id);
        let r = find(rider).ok_or_else(|| format!("no mob {rider} is loaded"))?;
        if !own_kind(&r.kind) {
            return Err(format!("mob {rider} is {}, not a content mod's", r.kind));
        }
        if let Some(v) = vehicle {
            if v == rider {
                return Err("a mob cannot ride itself".into());
            }
            let ve = find(v).ok_or_else(|| format!("no mob {v} is loaded"))?;
            let d = (r.x - ve.x).powi(2) + (r.y - ve.y).powi(2) + (r.z - ve.z).powi(2);
            if ve.dimension != r.dimension || d > RIDE_RANGE * RIDE_RANGE {
                return Err(format!(
                    "mob {v} is not within {RIDE_RANGE} blocks of {rider}"
                ));
            }
            // The vehicle's own chain of vehicles must not lead back to the rider.
            let mut at = v;
            for _ in 0..16 {
                match self.snapshot.vehicles.iter().find(|(m, _)| *m == at) {
                    Some(&(_, next)) if next == rider => {
                        return Err(format!("mob {v} rides {rider}"));
                    }
                    Some(&(_, next)) => at = next,
                    None => break,
                }
            }
        }
        let rides = self.out.iter().filter(|o| matches!(o, Output::Ride { .. }));
        if rides.count() >= RIDES_PER_CALL {
            return Err(format!("at most {RIDES_PER_CALL} rides per call"));
        }
        self.out.push(Output::Ride { rider, vehicle });
        Ok(())
    }
    fn vehicle(&mut self, id: u32) -> Option<u32> {
        (self.snapshot.vehicles.iter())
            .find(|(m, _)| *m == id)
            .map(|&(_, v)| v)
    }
    fn spawn(
        &mut self,
        kind: String,
        dimension: String,
        at: EntityPoint,
        yaw: f32,
        data: Vec<(String, String)>,
    ) -> Result<(), String> {
        let spawned = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Spawn { .. }));
        if spawned.count() >= SPAWNS_PER_CALL {
            return Err(format!("at most {SPAWNS_PER_CALL} spawns per call"));
        }
        if !own_kind(&kind) || !self.snapshot.mob_kinds.contains(&kind) {
            return Err(format!("{kind} is not a content mod's mob"));
        }
        if !finite(&at) || !yaw.is_finite() {
            return Err("spawn needs a point and a yaw".into());
        }
        check_data(data.iter().map(|(k, v)| (k.as_str(), Some(v.as_str()))))?;
        self.out.push(Output::Spawn {
            name: self.name.clone(),
            kind,
            dimension,
            at: [at.x, at.y, at.z],
            yaw,
            data,
        });
        Ok(())
    }
}
impl State {
    fn online_player(&self, player: &str) -> Result<(), String> {
        if self.snapshot.players.iter().any(|p| p.id == player) {
            Ok(())
        } else {
            Err(format!("no online player {player}"))
        }
    }
}
impl players::Host for State {
    fn damage_taken(&mut self) -> Vec<PlayerDamage> {
        self.snapshot
            .player_damage
            .iter()
            .take(256)
            .cloned()
            .collect()
    }
    fn heal(&mut self, player: String, amount: f32) -> Result<(), String> {
        self.online_player(&player)?;
        if !(0.0..=1024.0).contains(&amount) {
            return Err("heal amount is 0..1024".into());
        }
        self.out.push(Output::PlayerHeal { player, amount });
        Ok(())
    }
    fn equipment(&mut self, player: String) -> Option<EquipmentSlots> {
        self.inventories.lock().ok()?.as_ref()?.equipment(&player)
    }
    fn damage_item(&mut self, player: String, hand: String, amount: u32) -> Result<(), String> {
        self.online_player(&player)?;
        if !matches!(hand.as_str(), "main_hand" | "off_hand") {
            return Err("hand is main_hand or off_hand".into());
        }
        self.out.push(Output::DamageItem {
            player,
            hand,
            amount,
        });
        Ok(())
    }
    fn knockback(&mut self, player: String, strength: f32, dx: f64, dz: f64) -> Result<(), String> {
        self.online_player(&player)?;
        if !(0.0..=4.0).contains(&strength) || !dx.is_finite() || !dz.is_finite() {
            return Err("knockback needs strength 0..4 and a finite direction".into());
        }
        self.out.push(Output::PlayerKnockback {
            player,
            strength,
            dx,
            dz,
        });
        Ok(())
    }
    fn push(&mut self, player: String, velocity: PlayerPoint) -> Result<(), String> {
        self.online_player(&player)?;
        let velocity = EntityPoint {
            x: velocity.x,
            y: velocity.y,
            z: velocity.z,
        };
        check_order(&EntityOrder::Push(velocity))?;
        self.out.push(Output::PlayerPush { player, velocity });
        Ok(())
    }
    fn item_uses(&mut self) -> Vec<PlayerItemUse> {
        self.snapshot
            .item_uses
            .iter()
            .take(PLAYER_ACTIONS)
            .cloned()
            .collect()
    }
    fn attacks(&mut self) -> Vec<PlayerAttack> {
        self.snapshot
            .attacks
            .iter()
            .take(PLAYER_ACTIONS)
            .cloned()
            .collect()
    }
    fn add_effect(
        &mut self,
        player: String,
        id: String,
        seconds: u32,
        amplifier: u8,
    ) -> Result<(), String> {
        if !self.snapshot.players.iter().any(|p| p.id == player) {
            return Err(format!("no online player {player}"));
        }
        let effect = EffectChange {
            id,
            seconds,
            amplifier,
        };
        check_effect(&effect, &self.snapshot)?;
        self.out.push(Output::PlayerEffect { player, effect });
        Ok(())
    }
    fn take_item(&mut self, player: String, item: String, count: u32) -> Result<u32, String> {
        let mut slot = self
            .inventories
            .lock()
            .map_err(|_| "inventories unavailable")?;
        slot.as_mut()
            .ok_or("inventories unavailable")?
            .take_item(&player, &item, count)
    }
    fn give_item(&mut self, player: String, item: String, count: u32) -> Result<(), String> {
        let mut slot = self
            .inventories
            .lock()
            .map_err(|_| "inventories unavailable")?;
        slot.as_mut()
            .ok_or("inventories unavailable")?
            .give_item(&player, &item, count)
    }
    fn online(&mut self) -> Vec<ServerPlayer> {
        self.snapshot.players.clone()
    }
    fn facing(&mut self, player: String) -> Option<PlayerLook> {
        (self.snapshot.player_looks.iter())
            .find(|(id, _)| *id == player)
            .map(|(_, look)| *look)
    }
    fn tell(&mut self, player: String, text: String) -> bool {
        self.snapshot.players.iter().any(|p| p.id == player)
            && self.say(|name| Output::Tell {
                name,
                player,
                text: clip(text),
            })
    }
    fn broadcast(&mut self, text: String) {
        self.say(|name| Output::Broadcast {
            name,
            text: clip(text),
        });
    }
}
/// A key a mod registered (capability `input`).
#[derive(Clone, Debug, PartialEq)]
pub struct ModKey {
    /// The mod's own name for it, echoed in `key-down`.
    pub name: String,
    /// What it does, for Controls.
    pub label: String,
    /// `key.keyboard.r`.
    pub key: String,
}

/// What capability `input` holds for one mod.
#[derive(Default)]
struct InputState {
    claimed: Vec<String>,
    keys: Vec<ModKey>,
    events: std::collections::VecDeque<InputEvent>,
    /// The mod's keys held now, by name.
    down: Vec<String>,
    /// It holds the mouse wheel.
    wheel: bool,
    /// The keys the player chose, `name → key`: they win over
    /// `register-key`'s.
    bindings: Vec<(String, String)>,
}
/// Events kept for `poll`, items claimed and keys, per mod.
const INPUT_EVENTS: usize = 256;
const CLAIMS: usize = 64;
const KEYS: usize = 16;

/// `key.keyboard.<name>` or `key.mouse.<name>`, as options.txt writes keys;
/// the game knows which names exist and ignores a key it has none for.
fn key_ok(key: &str) -> bool {
    let rest = key
        .strip_prefix("key.keyboard.")
        .or_else(|| key.strip_prefix("key.mouse."));
    rest.is_some_and(|r| {
        (1..=32).contains(&r.len())
            && r.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.')
    })
}

impl InputState {
    /// Key `name` is `key` now; held, it lets go first.
    fn rebind(&mut self, name: &str, key: String) {
        let Some(k) = self.keys.iter_mut().find(|k| k.name == name) else {
            return;
        };
        k.key = key;
        if let Some(i) = self.down.iter().position(|n| n == name) {
            let name = self.down.swap_remove(i);
            self.push(InputEvent::KeyUp(name));
        }
    }
    fn push(&mut self, event: InputEvent) {
        if self.events.len() == INPUT_EVENTS {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

impl input::Host for State {
    fn claim_items(&mut self, ids: Vec<String>) {
        self.input.claimed = ids.into_iter().take(CLAIMS).map(clip).collect();
    }
    fn register_key(&mut self, name: String, label: String, key: String) -> Result<(), String> {
        if !key_ok(&key) {
            return Err(format!(
                "{key:?} is not a key as options.txt writes them (key.keyboard.r)"
            ));
        }
        let (name, label) = (clip(name), clip(label));
        let key = match self.input.bindings.iter().find(|(n, _)| *n == name) {
            Some((_, chosen)) => chosen.clone(),
            None => key,
        };
        if let Some(k) = self.input.keys.iter_mut().find(|k| k.name == name) {
            (k.label, k.key) = (label, key);
            return Ok(());
        }
        if self.input.keys.len() == KEYS {
            return Err(format!("a mod may register {KEYS} keys"));
        }
        self.input.keys.push(ModKey { name, label, key });
        Ok(())
    }
    fn held(&mut self) -> Option<HeldItem> {
        self.snapshot.held.clone()
    }
    fn poll(&mut self) -> Vec<InputEvent> {
        self.input.events.drain(..).collect()
    }
    fn mapping(&mut self, name: String) -> Option<input::MappingState> {
        let (_, label, down) = self.snapshot.mappings.iter().find(|m| m.0 == name)?;
        Some(input::MappingState {
            label: label.clone(),
            down: *down,
        })
    }
    fn clicks_per_second(&mut self) -> (u32, u32) {
        self.snapshot.cps
    }
    fn claim_wheel(&mut self, on: bool) {
        self.input.wheel = on;
    }
}

/// HUD shapes one mod may keep (capability `render`).
const SHAPES: usize = 1024;
/// The longest jolt `render.shake` makes, in seconds.
const SHAKE_SECONDS: f32 = 10.0;
/// `render.set-zoom`'s limits: `Camera.tickFov` keeps the modifier at 0.1
/// or more, and turning never stops.
const ZOOM_MAX: f32 = 10.0;
const TURN_MIN: f32 = 0.05;
/// `set-minimap`'s side in GUI pixels and its blocks per map pixel.
const MINIMAP_SIZE: (f32, f32) = (16.0, 256.0);
const MINIMAP_SCALE: (f32, f32) = (0.5, 4.0);
/// `set-far-terrain`'s distance in chunks, as the LOD Distance setting's.
const FAR_TERRAIN_DISTANCE: (u32, u32) = (64, 1024);

/// A mod's corner panel: shapes in a `width` × `height` box the game
/// places in `corner`, stacked with the other overlays.
#[derive(Clone, Debug)]
pub struct CornerPanel {
    pub corner: RenderCorner,
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<Shape>,
}

fn clip_shapes(shapes: Vec<Shape>) -> Vec<Shape> {
    shapes
        .into_iter()
        .take(SHAPES)
        .map(|shape| match shape {
            Shape::Sprite(mut s) => {
                s.sprite = clip(s.sprite);
                Shape::Sprite(s)
            }
            Shape::Item(mut i) => {
                i.item = clip(i.item);
                i.scale = if i.scale.is_finite() {
                    i.scale.clamp(0.0, 16.0)
                } else {
                    1.0
                };
                Shape::Item(i)
            }
            Shape::Text(mut t) => {
                t.text = clip(t.text);
                t.color &= 0xFF_FFFF;
                Shape::Text(t)
            }
            other => other,
        })
        .collect()
}

impl render::Host for State {
    fn set_hud(&mut self, shapes: Vec<Shape>) {
        self.shapes = clip_shapes(shapes);
    }
    fn set_corner(&mut self, corner: RenderCorner, width: f32, height: f32, shapes: Vec<Shape>) {
        let size = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1024.0)
            } else {
                0.0
            }
        };
        self.corner = Some(CornerPanel {
            corner,
            width: size(width),
            height: size(height),
            shapes: clip_shapes(shapes),
        });
    }
    fn clear_corner(&mut self) {
        self.corner = None;
    }
    fn text_width(&mut self, text: String) -> f32 {
        let widths = &self.snapshot.font_widths;
        text.chars()
            .map(|c| widths.get(c as usize).copied().unwrap_or(6.0))
            .sum::<f32>()
            .ceil()
    }
    fn set_zoom(&mut self, zoom: ModZoom) {
        let finite = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        let factor = finite(zoom.factor, 1.0, ZOOM_MAX);
        self.zoom = (factor > 1.0).then(|| ModZoom {
            factor,
            smooth: zoom.smooth,
            turn: if zoom.turn.is_finite() {
                zoom.turn.clamp(TURN_MIN, 1.0)
            } else {
                1.0
            },
        });
    }
    fn set_minimap(&mut self, minimap: Option<ModMinimap>) {
        let finite = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        self.minimap = minimap.map(|m| ModMinimap {
            size: finite(m.size, MINIMAP_SIZE.0, MINIMAP_SIZE.1),
            blocks_per_pixel: finite(m.blocks_per_pixel, MINIMAP_SCALE.0, MINIMAP_SCALE.1),
            ..m
        });
    }
    fn set_far_terrain(&mut self, far_terrain: Option<ModFarTerrain>) {
        self.far_terrain = far_terrain.map(|f| ModFarTerrain {
            distance: f
                .distance
                .clamp(FAR_TERRAIN_DISTANCE.0, FAR_TERRAIN_DISTANCE.1),
            ..f
        });
    }
    fn frame_stats(&mut self) -> FrameRate {
        let (fps, min, low) = self.snapshot.frame;
        FrameRate { fps, min, low }
    }
    fn gui_size(&mut self) -> (f32, f32) {
        self.snapshot.gui_size
    }
    fn shake(&mut self, strength: f32, seconds: f32) {
        let ok = |v: f32, max: f32| {
            if v.is_finite() {
                v.clamp(0.0, max)
            } else {
                0.0
            }
        };
        let (strength, seconds) = (ok(strength, 1.0), ok(seconds, SHAKE_SECONDS));
        if strength > 0.0 && seconds > 0.0 {
            self.out.push(Output::Shake { strength, seconds });
        }
    }
}

impl command::Host for State {
    fn run(&mut self, line: String) {
        let queued = self
            .out
            .iter()
            .filter(|o| matches!(o, Output::Command { .. }))
            .count();
        if queued < CHAT_PER_CALL {
            self.out.push(Output::Command { line: clip(line) });
        }
    }
    fn run_quiet(&mut self, lines: Vec<String>) {
        let queued: usize = self
            .out
            .iter()
            .map(|o| match o {
                Output::QuietCommands { lines, .. } => lines.len(),
                _ => 0,
            })
            .sum();
        let room = QUIET_PER_CALL.saturating_sub(queued);
        if room > 0 && !lines.is_empty() {
            self.out.push(Output::QuietCommands {
                mod_id: self.id.clone(),
                lines: lines.into_iter().take(room).map(clip).collect(),
            });
        }
    }
}
impl hud::Host for State {
    fn set_panel(&mut self, corner: Corner, lines: Vec<Line>) {
        let lines = lines
            .into_iter()
            .take(HUD_LINES)
            .map(|l| Line {
                text: clip(l.text),
                color: l.color & 0xFF_FFFF,
            })
            .collect();
        self.panel = Some(Panel { corner, lines });
    }
    fn clear(&mut self) {
        self.panel = None;
    }
}
impl ui::Host for State {
    fn open_screen(&mut self, mut screen: ui::Screen) {
        screen.title = clip(screen.title);
        screen.widgets = clip_widgets(screen.widgets);
        self.ui.screen = Some(screen);
        self.ui.open = true;
    }
    fn close_screen(&mut self) {
        self.ui.screen = None;
        self.ui.close = true;
    }
    fn set_settings(&mut self, widgets: Vec<ui::Widget>) {
        self.ui.settings = clip_widgets(widgets);
    }
}
impl storage::Host for State {
    fn get(&mut self, key: String) -> Option<Vec<u8>> {
        storage_key_ok(&key)
            .then(|| std::fs::read(self.data_dir.join(&key)).ok())
            .flatten()
    }
    fn set(&mut self, key: String, value: Vec<u8>) -> Result<(), String> {
        if !storage_key_ok(&key) {
            return Err(format!("{key:?} is not [a-z0-9_.-]{{1,64}}"));
        }
        let len = value.len() as u64;
        if len > self.storage_budget {
            return Err("too much storage traffic in one call".into());
        }
        // Everything already on disk but the file this call replaces.
        let existing: Vec<std::fs::DirEntry> = std::fs::read_dir(&self.data_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy() != key)
            .collect();
        if existing.len() >= STORAGE_MAX_KEYS {
            return Err(format!("would hold over {STORAGE_MAX_KEYS} keys"));
        }
        let used: u64 = existing
            .iter()
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();
        if used + len > STORAGE_QUOTA {
            return Err(format!(
                "would use over {} MiB of storage",
                STORAGE_QUOTA >> 20
            ));
        }
        std::fs::create_dir_all(&self.data_dir).map_err(|e| e.to_string())?;
        std::fs::write(self.data_dir.join(&key), value).map_err(|e| e.to_string())?;
        self.storage_budget -= len;
        Ok(())
    }
    fn delete(&mut self, key: String) {
        if storage_key_ok(&key) {
            let _ = std::fs::remove_file(self.data_dir.join(&key));
        }
    }
}

/// A storage key: one path segment of `[a-z0-9_.-]{1,64}`, not `.` or `..`.
/// No `/`, so a key can never leave the mod's own directory.
fn storage_key_ok(key: &str) -> bool {
    let chars_ok = (1..=64).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
    chars_ok && key != "." && key != ".."
}

#[cfg(test)]
mod entity_tests {
    use super::*;

    fn state() -> State {
        State {
            name: "Mob Orders".into(),
            id: "mob_orders".into(),
            snapshot: Arc::new(Snapshot::default()),
            out: Vec::new(),
            panel: None,
            ui: UiState::default(),
            shapes: Vec::new(),
            corner: None,
            zoom: None,
            minimap: None,
            far_terrain: None,
            input: InputState::default(),
            browser: BrowserState::default(),
            data_dir: PathBuf::new(),
            storage_budget: STORAGE_WRITE_BUDGET,
            item_spawn_budget: (None, 0),
            limits: StoreLimitsBuilder::new().build(),
            world: WorldSlot::default(),
            inventories: InventoriesSlot::default(),
        }
    }

    #[test]
    fn entity_raycast_respects_distance_blocks_and_live_snapshot() {
        let mut s = state();
        assert!(game::Host::raycast_entity(&mut s, 64.0).is_none());
        let snapshot = Arc::make_mut(&mut s.snapshot);
        snapshot.view_entity = Some((
            4.0,
            EntityHit {
                kind: "ruby:golem".into(),
                name: "Ruby Golem".into(),
                health: 37.5,
                max_health: 80.0,
                x: 0.0,
                y: 64.0,
                z: 4.0,
            },
        ));
        for distance in [-1.0, 3.9, f32::NAN] {
            assert!(game::Host::raycast_entity(&mut s, distance).is_none());
        }
        let hit = game::Host::raycast_entity(&mut s, 4.0).unwrap();
        assert_eq!((hit.health, hit.max_health), (37.5, 80.0));
        Arc::make_mut(&mut s.snapshot).view_hit = Some((
            3.0,
            Hit {
                block_x: 0,
                block_y: 64,
                block_z: 3,
                x: 0.0,
                y: 64.0,
                z: 3.0,
                block: "minecraft:stone".into(),
            },
        ));
        assert!(game::Host::raycast_entity(&mut s, 64.0).is_none());
        assert!(game::Host::raycast(&mut s, 64.0).is_some());
        let snapshot = Arc::make_mut(&mut s.snapshot);
        snapshot.view_hit = None;
        snapshot.view_entity.as_mut().unwrap().1.health = 12.0;
        assert_eq!(
            game::Host::raycast_entity(&mut s, 64.0).unwrap().health,
            12.0
        );
        Arc::make_mut(&mut s.snapshot)
            .view_entity
            .as_mut()
            .unwrap()
            .0 = RAYCAST_MAX + 1.0;
        assert!(game::Host::raycast_entity(&mut s, f32::INFINITY).is_none());
    }

    #[test]
    fn pre_level_16_game_components_still_link() {
        use wit_parser::{ManglingAndAbi, Resolve};
        let mut resolve = Resolve::default();
        let package = resolve
            .push_str(
                "horizon.wit",
                include_str!("../../horizon-mod-sdk/wit/horizon.wit"),
            )
            .unwrap();
        for (_, interface) in resolve.interfaces.iter_mut() {
            if interface.name.as_deref() == Some("game") {
                interface
                    .functions
                    .retain(|name, _| name != "raycast-entity");
            }
        }
        let world = resolve
            .select_world(&[package], Some("horizon-mod"))
            .unwrap();
        resolve.worlds[world].exports.clear();
        let interfaces = &resolve.interfaces;
        resolve.worlds[world].imports.retain(|_, item| matches!(item, wit_parser::WorldItem::Interface { id, .. } if interfaces[*id].name.as_deref() == Some("game")));
        let mut module = wit_component::dummy_module(&resolve, world, ManglingAndAbi::Standard32);
        wit_component::embed_component_metadata(
            &mut module,
            &resolve,
            world,
            wit_component::StringEncoding::UTF8,
        )
        .unwrap();
        let wasm = wit_component::ComponentEncoder::default()
            .module(&module)
            .unwrap()
            .encode()
            .unwrap();
        let functions = crate::api_level::imports(&wasm).unwrap();
        assert_eq!(
            crate::api_level::needed(functions.iter().map(String::as_str)).0,
            2
        );
        let mut mods = Mods::default();
        let (engine, linker) = mods.engine().unwrap();
        let component = Component::new(engine, wasm).unwrap();
        let mut store = Store::new(engine, state());
        store.set_fuel(FUEL_INIT).unwrap();
        linker.instantiate(&mut store, &component).unwrap();
    }

    #[test]
    fn inventory_imports_share_mutations_between_mods_and_return_errors() {
        let mut first = state();
        let mut second = state();
        let fixture = crate::testing::InventoryFixture {
            players: [("alice".into(), false), ("creative".into(), true)].into(),
            ..Default::default()
        };
        *first.inventories.lock().unwrap() = Some(Box::new(fixture));
        second.inventories = first.inventories.clone();
        players::Host::give_item(&mut first, "alice".into(), "minecraft:beef".into(), 2).unwrap();
        assert_eq!(
            players::Host::take_item(&mut first, "alice".into(), "minecraft:beef".into(), 1)
                .unwrap(),
            1
        );
        assert_eq!(
            players::Host::take_item(&mut second, "alice".into(), "minecraft:beef".into(), 2)
                .unwrap(),
            1
        );
        assert_eq!(
            players::Host::take_item(&mut first, "alice".into(), "minecraft:beef".into(), 1)
                .unwrap(),
            0
        );
        assert_eq!(
            players::Host::take_item(&mut first, "creative".into(), "minecraft:beef".into(), 50)
                .unwrap(),
            50
        );
        assert!(
            players::Host::take_item(&mut first, "offline".into(), "minecraft:beef".into(), 1)
                .is_err()
        );
        assert!(players::Host::give_item(&mut first, "alice".into(), "bad id".into(), 1).is_err());
        first.inventories.lock().unwrap().take();
        assert!(
            players::Host::take_item(&mut second, "alice".into(), "minecraft:beef".into(), 1)
                .is_err()
        );
    }

    #[test]
    fn explicit_targets_are_checked_against_online_players() {
        let id = "00000000-0000-4000-8000-000000000001";
        let order = EntityOrder::Target(EntityTarget::Player(id.into()));
        assert!(check_order_names(&order, &Snapshot::default()).is_err());
        let snapshot = Snapshot {
            players: vec![ServerPlayer {
                id: id.into(),
                name: "Alex".into(),
                x: 0.0,
                y: 64.0,
                z: 0.0,
                dimension: "minecraft:overworld".into(),
                game_mode: GameMode::Survival,
                health: 20.0,
            }],
            ..Default::default()
        };
        assert!(check_order_names(&order, &snapshot).is_ok());
        assert!(
            check_order_names(
                &EntityOrder::Target(EntityTarget::Player("offline".into())),
                &snapshot
            )
            .is_err()
        );
    }

    #[test]
    fn pre_level_12_server_components_still_link() {
        use wit_parser::{ManglingAndAbi, Resolve, TypeDefKind};
        let wit = include_str!("../../horizon-mod-sdk/wit/horizon.wit");
        let mut resolve = Resolve::default();
        let package = resolve.push_str("horizon.wit", wit).unwrap();
        // Reconstruct the previous imported interfaces: no API-12 functions
        // and the old order variant, including its original mount discriminant.
        for (_, interface) in resolve.interfaces.iter_mut() {
            let additions: &[&str] = match interface.name.as_deref() {
                Some("players") => &["item-uses", "attacks", "add-effect"],
                Some("world") => &["container", "set-container", "place-structure"],
                _ => &[],
            };
            let prefix = interface.name.clone().unwrap_or_default();
            interface.functions.retain(|name, _| {
                !additions.contains(&name.as_str())
                    && crate::api_level::level_of(&format!("{prefix}.{name}"))
                        .is_none_or(|l| l < 12)
            });
        }
        for (_, ty) in resolve.types.iter_mut() {
            if ty.name.as_deref() == Some("order")
                && let TypeDefKind::Variant(variant) = &mut ty.kind
            {
                variant.cases.retain(|case| case.name != "effect");
            }
        }
        let world = resolve
            .select_world(&[package], Some("horizon-server-mod"))
            .unwrap();
        let mut module = wit_component::dummy_module(&resolve, world, ManglingAndAbi::Standard32);
        wit_component::embed_component_metadata(
            &mut module,
            &resolve,
            world,
            wit_component::StringEncoding::UTF8,
        )
        .unwrap();
        let wasm = wit_component::ComponentEncoder::default()
            .module(&module)
            .unwrap()
            .encode()
            .unwrap();
        let mut mods = Mods::default();
        let (engine, linker) = mods.engine().unwrap();
        let component = Component::new(engine, wasm).unwrap();
        let mut store = Store::new(engine, state());
        store.set_fuel(FUEL_INIT).unwrap();
        let instance = linker.instantiate(&mut store, &component).unwrap();
        // Dummy exports trap if called; binding them still checks every old
        // server-events signature against the current generated bindings.
        HorizonServerMod::new(&mut store, &instance).unwrap();
    }

    #[test]
    fn dynamic_orders_decode_case_names_and_preserve_payloads() {
        let record = |fields: Vec<(&str, Val)>| {
            Val::Record(
                fields
                    .into_iter()
                    .map(|(name, value)| (name.into(), value))
                    .collect(),
            )
        };
        let variant =
            |name: &str, payload: Option<Val>| Val::Variant(name.into(), payload.map(Box::new));
        let order = |value| Val::Tuple(vec![Val::U32(7), value]);
        let values = Val::List(vec![
            order(variant(
                "mount",
                Some(record(vec![
                    ("player", Val::String("alice".into())),
                    ("height", Val::Float32(2.0)),
                    ("forward", Val::Float32(-0.5)),
                ])),
            )),
            order(variant("dismount", None)),
            order(variant(
                "set-data",
                Some(Val::List(vec![
                    Val::Tuple(vec![
                        Val::String("owner".into()),
                        Val::Option(Some(Box::new(Val::String("alice".into())))),
                    ]),
                    Val::Tuple(vec![Val::String("old".into()), Val::Option(None)]),
                ])),
            )),
            order(variant(
                "effect",
                Some(record(vec![
                    ("id", Val::String("minecraft:wither".into())),
                    ("seconds", Val::U32(10)),
                    ("amplifier", Val::U8(2)),
                ])),
            )),
            order(variant("target", Some(variant("nearest-player", None)))),
            order(variant("animate", Some(Val::Option(None)))),
        ]);
        let got = decode_orders(&[values]).unwrap();
        assert_eq!(
            got,
            vec![
                (
                    7,
                    EntityOrder::Mount(EntityMount {
                        player: "alice".into(),
                        height: 2.0,
                        forward: -0.5
                    })
                ),
                (7, EntityOrder::Dismount),
                (
                    7,
                    EntityOrder::SetData(vec![
                        ("owner".into(), Some("alice".into())),
                        ("old".into(), None)
                    ])
                ),
                (
                    7,
                    EntityOrder::Effect(EffectChange {
                        id: "minecraft:wither".into(),
                        seconds: 10,
                        amplifier: 2
                    })
                ),
                (7, EntityOrder::Target(EntityTarget::NearestPlayer)),
                (7, EntityOrder::Animate(None)),
            ]
        );
        assert!(decode_orders(&[Val::List(vec![order(variant("unknown", None))])]).is_err());
        assert!(
            decode_orders(&[Val::List(vec![order(variant(
                "heal",
                Some(Val::Float32(f32::NAN))
            ))])])
            .is_err()
        );
        assert!(
            decode_orders(&[Val::List(vec![
                order(variant("halt", None));
                ORDERS_PER_CALL + 1
            ])])
            .is_err()
        );
    }

    #[test]
    fn registry_ids_reject_namespace_and_path_traversal() {
        for bad in [
            ".:template",
            "..:template",
            "minecraft:../template",
            "minecraft:./template",
            "minecraft:a//b",
            "minecraft:a:b",
        ] {
            assert!(!resource_id(bad), "{bad}");
        }
        for good in ["minecraft:igloo/top", "ruby_mod:temple.v2/test-1"] {
            assert!(resource_id(good), "{good}");
        }
    }

    #[test]
    fn player_action_lists_are_ordered_bounded_and_repeatable() {
        let mut state = state();
        for count in [0, 2, 256, 300] {
            let item_uses = (0..count)
                .map(|i| PlayerItemUse {
                    player: i.to_string(),
                    item: "minecraft:stick".into(),
                    hand: "off_hand".into(),
                    sneaking: i % 2 == 0,
                    target: Some(PlayerBlockHit {
                        x: i,
                        y: 64,
                        z: 0,
                        face: "up".into(),
                    }),
                })
                .collect::<Vec<_>>();
            let attacks = (0..count)
                .map(|i| PlayerAttack {
                    player: i.to_string(),
                    item: None,
                    mob: Some(i as u32),
                    target_player: None,
                    damage: i as f32,
                })
                .collect::<Vec<_>>();
            state.snapshot = Arc::new(Snapshot {
                item_uses: item_uses.into(),
                attacks: attacks.into(),
                ..Default::default()
            });
            for _ in 0..2 {
                let uses = players::Host::item_uses(&mut state);
                let attacks = players::Host::attacks(&mut state);
                assert_eq!(
                    uses,
                    state.snapshot.item_uses[..(count as usize).min(PLAYER_ACTIONS)]
                );
                assert_eq!(
                    attacks,
                    state.snapshot.attacks[..(count as usize).min(PLAYER_ACTIONS)]
                );
            }
        }
    }

    #[test]
    fn status_effects_require_registry_ids_online_players_and_safe_durations() {
        let mut state = state();
        state.snapshot = Arc::new(Snapshot {
            players: vec![ServerPlayer {
                id: "alice".into(),
                name: "Alice".into(),
                x: 0.0,
                y: 64.0,
                z: 0.0,
                dimension: "minecraft:overworld".into(),
                game_mode: GameMode::Survival,
                health: 20.0,
            }],
            effect_ids: Arc::new(["minecraft:slowness".into()].into()),
            ..Default::default()
        });
        let effect = |id: &str, seconds| EffectChange {
            id: id.into(),
            seconds,
            amplifier: 255,
        };
        assert!(
            check_order_names(
                &EntityOrder::Effect(effect("minecraft:slowness", 0)),
                &state.snapshot
            )
            .is_ok()
        );
        assert!(
            check_order_names(
                &EntityOrder::Effect(effect("minecraft:unknown", 2)),
                &state.snapshot
            )
            .is_err()
        );
        assert!(
            check_order_names(
                &EntityOrder::Effect(effect("minecraft:slowness", u32::MAX)),
                &state.snapshot
            )
            .is_err()
        );
        assert!(
            players::Host::add_effect(
                &mut state,
                "offline".into(),
                "minecraft:slowness".into(),
                2,
                0
            )
            .is_err()
        );
        assert!(
            players::Host::add_effect(&mut state, "alice".into(), "minecraft:unknown".into(), 2, 0)
                .is_err()
        );
        assert!(state.out.is_empty());
        for seconds in [0, 20] {
            players::Host::add_effect(
                &mut state,
                "alice".into(),
                "minecraft:slowness".into(),
                seconds,
                255,
            )
            .unwrap();
            assert_eq!(
                state.out.last(),
                Some(&Output::PlayerEffect {
                    player: "alice".into(),
                    effect: effect("minecraft:slowness", seconds)
                })
            );
        }
    }

    #[test]
    fn container_and_template_validation_queues_only_valid_changes() {
        struct Templates;
        impl WorldView for Templates {
            fn block(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
                None
            }
            fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
                None
            }
            fn has_dimension(&self, dimension: &str) -> bool {
                dimension == "minecraft:overworld"
            }
            fn check_state(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn container(
                &self,
                _: &str,
                x: i32,
                _: i32,
                _: i32,
            ) -> Option<Vec<Option<ContainerItemStack>>> {
                (x == 0).then(|| vec![None; 3])
            }
            fn check_item_stack(&self, item: &str, count: u32) -> Result<(), String> {
                (item == "minecraft:stick" && (1..=64).contains(&count))
                    .then_some(())
                    .ok_or("invalid stack".into())
            }
            fn check_structure(&self, id: &str) -> Result<(), String> {
                (id == "minecraft:igloo/top")
                    .then_some(())
                    .ok_or("unknown template".into())
            }
        }
        let mut state = state();
        *state.world.lock().unwrap() = Some(Box::new(Templates));
        let dimension = "minecraft:overworld";
        assert_eq!(
            mod_world::Host::container(&mut state, dimension.into(), 0, 64, 0),
            Some(vec![None; 3])
        );
        assert_eq!(
            mod_world::Host::container(&mut state, dimension.into(), 1, 64, 0),
            None
        );
        let stack = |item: &str, count| {
            Some(ContainerItemStack {
                item: item.into(),
                count,
            })
        };
        for slots in [
            vec![None; 2],
            vec![None; CONTAINER_SLOTS + 1],
            vec![stack("unknown:item", 1), None, None],
            vec![stack("minecraft:stick", 0), None, None],
            vec![stack("minecraft:stick", 65), None, None],
        ] {
            assert!(
                mod_world::Host::set_container(&mut state, dimension.into(), 0, 64, 0, slots)
                    .is_err()
            );
        }
        assert!(
            mod_world::Host::set_container(
                &mut state,
                "unknown:dimension".into(),
                0,
                64,
                0,
                vec![None; 3]
            )
            .is_err()
        );
        for (id, rotation) in [
            ("minecraft:igloo/top", 4),
            ("minecraft:unknown", 0),
            ("minecraft:../igloo/top", 0),
            ("minecraft:igloo:top", 0),
        ] {
            assert!(
                mod_world::Host::place_structure(
                    &mut state,
                    dimension.into(),
                    id.into(),
                    0,
                    64,
                    0,
                    rotation,
                    false
                )
                .is_err()
            );
        }
        assert!(state.out.is_empty());
        mod_world::Host::set_container(
            &mut state,
            dimension.into(),
            0,
            64,
            0,
            vec![stack("minecraft:stick", 64), None, None],
        )
        .unwrap();
        // A container placed by this tick's block writes is validated again on apply.
        mod_world::Host::set_container(&mut state, dimension.into(), 1, 64, 0, vec![None; 3])
            .unwrap();
        mod_world::Host::place_structure(
            &mut state,
            dimension.into(),
            "minecraft:igloo/top".into(),
            0,
            64,
            0,
            3,
            true,
        )
        .unwrap();
        assert_eq!(state.out.len(), 3);
    }

    #[test]
    fn interactions_read_the_snapshot_in_order_up_to_256() {
        let mut state = state();
        for count in [0, 2, 256, 300] {
            let clicks: Vec<_> = (0..count)
                .map(|mob| EntityInteraction {
                    player: "01234567-89ab-cdef-0123-456789abcdef".into(),
                    mob,
                    kind: "ruby:golem".into(),
                    item: (mob % 2 == 0).then(|| "minecraft:bone".into()),
                    sneaking: mob % 3 == 0,
                    hand: if mob % 2 == 0 {
                        "main_hand"
                    } else {
                        "off_hand"
                    }
                    .into(),
                })
                .collect();
            state.snapshot = Arc::new(Snapshot {
                interactions: clicks.into(),
                ..Snapshot::default()
            });
            // Every reader sees the same tick; reading does not consume the clicks.
            for _ in 0..2 {
                let got = entities::Host::interactions(&mut state);
                assert_eq!(got.len(), (count as usize).min(256));
                for (click, expected) in got.iter().zip(state.snapshot.interactions.iter()) {
                    assert_eq!(click.player, expected.player);
                    assert_eq!(click.mob, expected.mob);
                    assert_eq!(click.kind, expected.kind);
                    assert_eq!(click.item, expected.item);
                    assert_eq!(click.sneaking, expected.sneaking);
                    assert_eq!(click.hand, expected.hand);
                }
            }
        }
    }

    struct Api15World;
    impl WorldView for Api15World {
        fn check_item_stack(&self, item: &str, count: u32) -> Result<(), String> {
            (item == "minecraft:stick" && (1..=64).contains(&count))
                .then_some(())
                .ok_or("invalid stack".into())
        }
        fn block_has_tag(&self, block: &str, tag: &str) -> bool {
            block == "minecraft:stone" && tag == "minecraft:stone_ore_replaceables"
        }

        fn block(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn has_dimension(&self, d: &str) -> bool {
            d == "minecraft:overworld"
        }
        fn check_state(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    #[test]
    fn api15_sound_checks_registry_dimension_and_finite_values() {
        let mut s = api15_state();
        *s.world.lock().unwrap() = Some(Box::new(Api15World));
        for (d, name, v, pitch, x) in [
            ("missing", "ruby:roar", 1.0, 1.0, 0.0),
            ("minecraft:overworld", "ruby:missing", 1.0, 1.0, 0.0),
            ("minecraft:overworld", "ruby:roar", -1.0, 1.0, 0.0),
            ("minecraft:overworld", "ruby:roar", 1.0, f32::NAN, 0.0),
            ("minecraft:overworld", "ruby:roar", 1.0, 1.0, f64::INFINITY),
        ] {
            assert!(
                mod_world::Host::play_sound(
                    &mut s,
                    d.into(),
                    point(x, 64.0, 0.0).into(),
                    name.into(),
                    v,
                    pitch
                )
                .is_err()
            );
        }
        assert!(s.out.is_empty());
        for name in ["ruby:roar", "minecraft:entity.zombie.ambient"] {
            mod_world::Host::play_sound(
                &mut s,
                "minecraft:overworld".into(),
                point(0.0, 64.0, 0.0).into(),
                name.into(),
                1.0,
                2.0,
            )
            .unwrap();
        }
        assert_eq!(s.out.len(), 2);
    }

    fn api15_state() -> State {
        let mut s = state();
        let mob = |id, kind: &str| ModEntity {
            id,
            kind: kind.into(),
            dimension: "minecraft:overworld".into(),
            x: 0.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            health: 20.0,
            max_health: 20.0,
            on_ground: true,
            target: None,
            target_player: false,
            scale: 1.0,
            data: vec![],
            age: 0,
            animation: None,
            texture: None,
            rider: None,
        };
        s.snapshot = Arc::new(Snapshot {
            players: vec![ServerPlayer {
                id: "alice".into(),
                name: "Alice".into(),
                x: 0.0,
                y: 64.0,
                z: 0.0,
                dimension: "minecraft:overworld".into(),
                game_mode: GameMode::Survival,
                health: 20.0,
            }],
            tamable_mobs: vec![2].into(),
            entities: vec![
                mob(1, "ruby:knight"),
                mob(2, "minecraft:horse"),
                mob(3, "minecraft:zombie"),
            ]
            .into(),
            entity_resources: Arc::new(EntityResources {
                sounds: ["ruby:roar".into(), "minecraft:entity.zombie.ambient".into()].into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        s
    }

    #[test]
    fn api15_tame_and_damage_item_validate_before_queueing() {
        let mut s = api15_state();
        for mob in [1, 3, 999] {
            assert!(entities::Host::tame(&mut s, mob, None).is_err());
        }
        assert!(entities::Host::tame(&mut s, 2, Some("bad owner".into())).is_err());
        assert!(
            players::Host::damage_item(&mut s, "offline".into(), "main_hand".into(), 1).is_err()
        );
        assert!(players::Host::damage_item(&mut s, "alice".into(), "head".into(), 1).is_err());
        assert!(s.out.is_empty());
        entities::Host::tame(&mut s, 2, None).unwrap();
        entities::Host::tame(
            &mut s,
            2,
            Some("01234567-89ab-cdef-0123-456789abcdef".into()),
        )
        .unwrap();
        for hand in ["main_hand", "off_hand"] {
            players::Host::damage_item(&mut s, "alice".into(), hand.into(), u32::MAX).unwrap();
        }
        assert_eq!(s.out.len(), 4);
        assert!(matches!(
            &s.out[0],
            Output::Tame {
                mob: 2,
                owner: None
            }
        ));
    }

    #[test]
    fn api15_block_tags_require_a_world_and_use_its_tables() {
        let mut s = api15_state();
        assert!(!mod_world::Host::block_has_tag(
            &mut s,
            "minecraft:stone".into(),
            "minecraft:stone_ore_replaceables".into()
        ));
        *s.world.lock().unwrap() = Some(Box::new(Api15World));
        assert!(mod_world::Host::block_has_tag(
            &mut s,
            "minecraft:stone".into(),
            "minecraft:stone_ore_replaceables".into()
        ));
        assert!(!mod_world::Host::block_has_tag(
            &mut s,
            "missing".into(),
            "minecraft:stone_ore_replaceables".into()
        ));
        assert!(!mod_world::Host::block_has_tag(
            &mut s,
            "minecraft:stone".into(),
            "#minecraft:stone_ore_replaceables".into()
        ));
    }

    #[test]
    fn api15_item_spawns_validate_stacks_and_budget_across_hooks() {
        let mut s = api15_state();
        *s.world.lock().unwrap() = Some(Box::new(Api15World));
        let spawn = |s: &mut State, d: &str, item: &str, count| {
            mod_world::Host::spawn_item(
                s,
                d.into(),
                point(0.0, 64.0, 0.0).into(),
                item.into(),
                count,
            )
        };
        assert!(spawn(&mut s, "missing", "minecraft:stick", 1).is_err());
        for (item, count) in [
            ("missing", 1),
            ("minecraft:stick", 0),
            ("minecraft:stick", 65),
        ] {
            assert!(spawn(&mut s, "minecraft:overworld", item, count).is_err());
        }
        assert!(s.out.is_empty());
        for _ in 0..16 {
            spawn(&mut s, "minecraft:overworld", "minecraft:stick", 64).unwrap();
        }
        // Draining output between hooks must not reset a per-tick budget.
        s.out.clear();
        assert!(spawn(&mut s, "minecraft:overworld", "minecraft:stick", 1).is_err());
        Arc::make_mut(&mut s.snapshot).clock = Some(Clock {
            game_time: 1,
            day_time: 1,
        });
        spawn(&mut s, "minecraft:overworld", "minecraft:stick", 1).unwrap();
        assert_eq!(s.out.len(), 1);
    }
    #[test]
    fn api15_equipment_reads_the_lent_server_inventory() {
        struct Gear;
        impl PlayerInventories for Gear {
            fn take_item(&mut self, _: &str, _: &str, _: u32) -> Result<u32, String> {
                Ok(0)
            }
            fn give_item(&mut self, _: &str, _: &str, _: u32) -> Result<(), String> {
                Ok(())
            }
            fn equipment(&self, p: &str) -> Option<EquipmentSlots> {
                (p == "alice").then(|| EquipmentSlots {
                    head: Some("ruby:helmet".into()),
                    chest: None,
                    legs: None,
                    feet: None,
                    main_hand: None,
                    off_hand: None,
                    hotbar: vec![None; 9],
                })
            }
        }
        let mut s = api15_state();
        assert!(players::Host::equipment(&mut s, "alice".into()).is_none());
        *s.inventories.lock().unwrap() = Some(Box::new(Gear));
        assert_eq!(
            players::Host::equipment(&mut s, "alice".into())
                .unwrap()
                .head
                .as_deref(),
            Some("ruby:helmet")
        );
        assert!(players::Host::equipment(&mut s, "offline".into()).is_none());
        assert!(s.out.is_empty());
    }

    #[test]
    fn api15_effect_push_and_heal_validate_and_include_vanilla_mobs() {
        let mut s = api15_state();
        Arc::make_mut(&mut s.snapshot).effect_ids = Arc::new(["minecraft:poison".into()].into());
        let effect = EffectChange {
            id: "minecraft:poison".into(),
            seconds: 10,
            amplifier: 1,
        };
        assert!(entities::Host::effect(&mut s, 999, effect.clone()).is_err());
        assert!(
            entities::Host::effect(
                &mut s,
                2,
                EffectChange {
                    id: "missing".into(),
                    ..effect.clone()
                }
            )
            .is_err()
        );
        assert!(
            entities::Host::effect(
                &mut s,
                2,
                EffectChange {
                    seconds: u32::MAX,
                    ..effect.clone()
                }
            )
            .is_err()
        );
        assert!(entities::Host::push(&mut s, 999, point(0.0, 0.0, 0.0)).is_err());
        assert!(entities::Host::push(&mut s, 2, point(0.0, f64::NAN, 0.0)).is_err());
        for amount in [-1.0, f32::NAN, 1025.0] {
            assert!(players::Host::heal(&mut s, "alice".into(), amount).is_err());
        }
        assert!(players::Host::heal(&mut s, "offline".into(), 1.0).is_err());
        assert!(s.out.is_empty());
        for mob in [1, 2, 3] {
            entities::Host::effect(&mut s, mob, effect.clone()).unwrap();
            entities::Host::push(&mut s, mob, point(0.0, 1.0, 0.0)).unwrap();
        }
        players::Host::heal(&mut s, "alice".into(), 1024.0).unwrap();
        assert_eq!(s.out.len(), 7);
    }
    #[test]
    fn api15_damage_lists_are_bounded_shared_and_replaced_each_tick() {
        let mut s = api15_state();
        let snapshot = Arc::make_mut(&mut s.snapshot);
        snapshot.hurt_events = (0..300)
            .map(|i| HurtEvent {
                victim: i,
                kind: "ruby:knight".into(),
                attacker_mob: Some(7),
                attacker_player: None,
                amount: 2.0,
            })
            .collect::<Vec<_>>()
            .into();
        snapshot.deaths = (0..300)
            .map(|id| Death {
                id,
                kind: "ruby:knight".into(),
                x: 1.0,
                y: 64.0,
                z: 2.0,
            })
            .collect::<Vec<_>>()
            .into();
        snapshot.player_damage = (0..300)
            .map(|_| PlayerDamage {
                player: "alice".into(),
                amount: 2.0,
                source_kind: "minecraft:arrow".into(),
                attacker_mob: Some(7),
            })
            .collect::<Vec<_>>()
            .into();
        for _ in 0..2 {
            let hurts = entities::Host::hurt_events(&mut s);
            assert_eq!(hurts.len(), 256);
            assert_eq!(hurts[255].victim, 255);
            assert_eq!(hurts[0].attacker_mob, Some(7));
            assert_eq!(entities::Host::deaths(&mut s).len(), 256);
            let damage = players::Host::damage_taken(&mut s);
            assert_eq!(damage.len(), 256);
            assert_eq!(damage[0].source_kind, "minecraft:arrow");
        }
        s.snapshot = Arc::new(Snapshot::default());
        assert!(entities::Host::hurt_events(&mut s).is_empty());
        assert!(entities::Host::deaths(&mut s).is_empty());
        assert!(players::Host::damage_taken(&mut s).is_empty());
    }

    #[test]
    fn api15_players_and_world_imports_do_not_require_entities_capability() {
        use wit_parser::{ManglingAndAbi, Resolve};
        for interface in ["players", "world"] {
            let mut resolve = Resolve::default();
            let package = resolve
                .push_str(
                    "horizon.wit",
                    include_str!("../../horizon-mod-sdk/wit/horizon.wit"),
                )
                .unwrap();
            let world = resolve
                .select_world(&[package], Some("horizon-server-mod"))
                .unwrap();
            resolve.worlds[world].exports.clear();
            let interfaces = &resolve.interfaces;
            resolve.worlds[world].imports.retain(|_, item| matches!(item, wit_parser::WorldItem::Interface { id, .. } if interfaces[*id].name.as_deref() == Some(interface)));
            let mut module =
                wit_component::dummy_module(&resolve, world, ManglingAndAbi::Standard32);
            wit_component::embed_component_metadata(
                &mut module,
                &resolve,
                world,
                wit_component::StringEncoding::UTF8,
            )
            .unwrap();
            let wasm = wit_component::ComponentEncoder::default()
                .module(&module)
                .unwrap()
                .encode()
                .unwrap();
            let imports = crate::manifest::component_imports(&wasm).unwrap();
            assert!(
                imports
                    .iter()
                    .all(|i| i.starts_with(&format!("horizon:mods/{interface}@"))),
                "{imports:?}"
            );
            let manifest = Manifest::parse(&format!("id = \"api15\"\nname = \"Api15\"\nversion = \"1\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"{interface}\"]\n")).unwrap();
            manifest
                .check_imports(imports.iter().map(String::as_str))
                .unwrap();
        }
    }

    #[test]
    fn api15_strike_validates_before_queueing() {
        let mut s = api15_state();
        let hit = StrikeHit {
            center: point(0.0, 64.0, 0.0),
            radius: 16.0,
            amount: 1024.0,
            fire: 30,
            skip_kinds: vec!["ruby:knight".into()],
            knockback: 0.25,
            credit_player: Some("alice".into()),
        };
        for mob in [2, 999] {
            assert!(entities::Host::strike(&mut s, mob, hit.clone()).is_err());
        }
        for bad in [
            StrikeHit {
                radius: f64::NAN,
                ..hit.clone()
            },
            StrikeHit {
                amount: 1025.0,
                ..hit.clone()
            },
            StrikeHit {
                fire: 31,
                ..hit.clone()
            },
            StrikeHit {
                knockback: f32::INFINITY,
                ..hit.clone()
            },
            StrikeHit {
                skip_kinds: vec!["bad id".into()],
                ..hit.clone()
            },
            StrikeHit {
                credit_player: Some("offline".into()),
                ..hit.clone()
            },
        ] {
            assert!(entities::Host::strike(&mut s, 1, bad).is_err());
        }
        assert!(s.out.is_empty());
        entities::Host::strike(&mut s, 1, hit.clone()).unwrap();
        assert_eq!(s.out, vec![Output::Strike { mob: 1, hit }]);
    }

    #[test]
    fn api15_player_motion_requires_online_finite_bounded_inputs() {
        let mut s = api15_state();
        assert!(
            players::Host::push(&mut s, "offline".into(), point(0.0, 0.0, 0.0).into()).is_err()
        );
        assert!(
            players::Host::push(&mut s, "alice".into(), point(f64::NAN, 0.0, 0.0).into()).is_err()
        );
        assert!(players::Host::push(&mut s, "alice".into(), point(4.1, 0.0, 0.0).into()).is_err());
        assert!(players::Host::knockback(&mut s, "alice".into(), -1.0, 1.0, 0.0).is_err());
        assert!(
            players::Host::knockback(&mut s, "alice".into(), 0.25, f64::INFINITY, 0.0).is_err()
        );
        assert!(s.out.is_empty());
        players::Host::push(&mut s, "alice".into(), point(1.0, 0.5, 0.0).into()).unwrap();
        players::Host::knockback(&mut s, "alice".into(), 0.25, 1.0, 0.0).unwrap();
        assert!(matches!(
            &s.out[..],
            [Output::PlayerPush { .. }, Output::PlayerKnockback { .. }]
        ));
    }

    #[test]
    fn ride_checks_kinds_range_and_loops_and_vehicle_reads_the_snapshot() {
        let mob = |id: u32, kind: &str, x: f64| ModEntity {
            id,
            kind: kind.into(),
            dimension: "minecraft:overworld".into(),
            x,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            health: 20.0,
            max_health: 20.0,
            on_ground: true,
            target: None,
            target_player: false,
            scale: 1.0,
            data: vec![],
            age: 0,
            animation: None,
            texture: None,
            rider: None,
        };
        let mut state = state();
        state.snapshot = Arc::new(Snapshot {
            entities: vec![
                mob(1, "ruby:knight", 0.0),
                mob(2, "ruby:horse", 2.0),
                mob(3, "minecraft:horse", 4.0),
                mob(4, "ruby:far", 40.0),
            ]
            .into(),
            // 2 already rides 1: 1 onto 2 would loop.
            vehicles: vec![(2, 1)].into(),
            ..Snapshot::default()
        });
        let ride = |state: &mut State, r, v| entities::Host::ride(state, r, v);
        assert!(ride(&mut state, 1, Some(3)).is_ok());
        assert!(ride(&mut state, 1, None).is_ok());
        assert!(ride(&mut state, 3, Some(2)).is_err(), "a vanilla rider");
        assert!(ride(&mut state, 1, Some(1)).is_err());
        assert!(ride(&mut state, 1, Some(4)).is_err(), "out of range");
        assert!(ride(&mut state, 1, Some(9)).is_err(), "not loaded");
        assert!(ride(&mut state, 1, Some(2)).is_err(), "a loop");
        let queued: Vec<_> = (state.out.iter())
            .filter_map(|o| match o {
                Output::Ride { rider, vehicle } => Some((*rider, *vehicle)),
                _ => None,
            })
            .collect();
        assert_eq!(queued, [(1, Some(3)), (1, None)]);
        assert_eq!(entities::Host::vehicle(&mut state, 2), Some(1));
        assert_eq!(entities::Host::vehicle(&mut state, 1), None);
    }

    #[test]
    fn texture_orders_need_safe_loaded_mod_textures() {
        let name = "ruby:entity/dragon/blue_stage3";
        let order = EntityOrder::Texture(Some(name.into()));
        let snapshot = Snapshot {
            entity_resources: Arc::new(EntityResources {
                textures: [name.to_string()].into(),
                ..EntityResources::default()
            }),
            ..Snapshot::default()
        };
        assert!(check_order(&order).is_ok());
        assert!(check_order_names(&order, &snapshot).is_ok());
        assert!(check_order_names(&order, &Snapshot::default()).is_err());
        assert!(check_order_names(&EntityOrder::Texture(None), &snapshot).is_ok());
        for bad in [
            "",
            "entity/blue",
            "minecraft:entity/pig",
            "ruby:block/blue",
            "ruby:entity/../blue",
            "ruby:entity/",
            "Ruby:entity/blue",
        ] {
            assert!(
                check_order(&EntityOrder::Texture(Some(bad.into()))).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn effects_validate_names_bounds_and_queue_batches_atomically() {
        let sound = |name: &str, volume, pitch| {
            EntityOrder::Sound(EntitySound {
                name: name.into(),
                volume,
                pitch,
            })
        };
        let particles = |name: &str, speed, count| {
            EntityOrder::Particles(EntityParticles {
                name: name.into(),
                at: point(1.0, 64.0, 2.0),
                delta: point(0.5, 0.5, 0.5),
                speed,
                count,
            })
        };
        let roar = sound("ruby:roar", 2.0, 0.5);
        let flame = particles("minecraft:flame", 0.1, 256);
        let mut state = state();
        state.snapshot = Arc::new(Snapshot {
            entities: vec![ModEntity {
                id: 1,
                kind: "ruby:dragon".into(),
                dimension: "minecraft:overworld".into(),
                x: 0.0,
                y: 64.0,
                z: 0.0,
                yaw: 0.0,
                health: 20.0,
                max_health: 20.0,
                on_ground: true,
                target: None,
                target_player: false,
                scale: 1.0,
                data: vec![],
                age: 0,
                animation: None,
                texture: None,
                rider: None,
            }]
            .into(),
            entity_resources: Arc::new(EntityResources {
                sounds: [
                    "ruby:roar".into(),
                    "minecraft:entity.ender_dragon.growl".into(),
                ]
                .into(),
                particles: [("minecraft:flame".into(), 32)].into(),
                ..EntityResources::default()
            }),
            ..Snapshot::default()
        });
        for valid in [
            roar.clone(),
            flame.clone(),
            particles("minecraft:flame", 0.0, 0),
            sound("minecraft:entity.ender_dragon.growl", 0.0, 0.0),
        ] {
            assert!(check_order(&valid).is_ok());
            assert!(check_order_names(&valid, &state.snapshot).is_ok());
        }
        for bad in [
            EntityOrder::Attribute(EntityAttributeChange {
                name: "max_health".into(),
                value: f64::NAN,
            }),
            EntityOrder::Target(EntityTarget::Player("offline".into())),
            EntityOrder::Attribute(EntityAttributeChange {
                name: "not_an_attribute".into(),
                value: 500.0,
            }),
            EntityOrder::Heal(f32::INFINITY),
            sound("ruby:missing", 1.0, 1.0),
            particles("minecraft:missing", 0.0, 1),
            particles("minecraft:dust", 0.0, 1),
            particles("minecraft:dragon_breath", 0.0, 1),
            sound("ruby:roar", f32::NAN, 1.0),
            sound("ruby:roar", 1.0, f32::INFINITY),
            sound("ruby:roar", -1.0, 1.0),
            sound("ruby:roar", 1.0, -1.0),
            particles("minecraft:flame", -1.0, 1),
            particles("minecraft:flame", f32::NAN, 1),
            particles("minecraft:flame", 0.0, 257),
            EntityOrder::Particles(EntityParticles {
                name: "minecraft:flame".into(),
                at: point(f64::NAN, 0.0, 0.0),
                delta: point(0.0, 0.0, 0.0),
                speed: 0.0,
                count: 1,
            }),
            EntityOrder::Particles(EntityParticles {
                name: "minecraft:flame".into(),
                at: point(0.0, 0.0, 0.0),
                delta: point(f64::MAX, 0.0, 0.0),
                speed: 0.0,
                count: 1,
            }),
        ] {
            assert!(
                entities::Host::give_orders(&mut state, vec![(1, roar.clone()), (1, bad)]).is_err()
            );
            assert!(state.out.is_empty());
        }
        entities::Host::give_orders(&mut state, vec![(1, roar), (1, flame)]).unwrap();
        assert!(matches!(&state.out[..], [Output::Orders { orders, .. }] if orders.len() == 2));
    }

    #[test]
    fn world_queries_read_current_rules_and_difficulty() {
        let mut state = state();
        assert_eq!(
            mod_world::Host::game_rule(&mut state, "unknown".into()),
            None
        );
        for (griefing, difficulty) in [("true", "normal"), ("false", "hard")] {
            state.snapshot = Arc::new(Snapshot {
                game_rules: [
                    ("mob_griefing".into(), griefing.into()),
                    ("random_tick_speed".into(), "9".into()),
                ]
                .into(),
                difficulty: difficulty.into(),
                ..Snapshot::default()
            });
            for name in ["mobGriefing", "mob_griefing", "minecraft:mob_griefing"] {
                assert_eq!(
                    mod_world::Host::game_rule(&mut state, name.into()).as_deref(),
                    Some(griefing)
                );
            }
            assert_eq!(
                mod_world::Host::game_rule(&mut state, "random_tick_speed".into()).as_deref(),
                Some("9")
            );
            assert_eq!(mod_world::Host::difficulty(&mut state), difficulty);
        }
    }

    #[test]
    fn time_is_dimension_specific_and_sky_reads_the_lent_world() {
        let mut state = state();
        assert!(!mod_world::Host::time(&mut state, "missing".into()).is_day);
        assert!(!mod_world::Host::can_see_sky(
            &mut state,
            "minecraft:overworld".into(),
            1,
            65,
            -1
        ));
        for day in [true, false] {
            state.snapshot = Arc::new(Snapshot {
                world_times: [
                    (
                        "minecraft:overworld".into(),
                        WorldTime {
                            day_time: 25000,
                            game_time: 100000,
                            is_day: day,
                            raining: !day,
                            thundering: !day,
                        },
                    ),
                    (
                        "minecraft:the_nether".into(),
                        WorldTime {
                            day_time: 25000,
                            game_time: 100000,
                            is_day: false,
                            raining: !day,
                            thundering: false,
                        },
                    ),
                ]
                .into(),
                ..Snapshot::default()
            });
            let time = mod_world::Host::time(&mut state, "minecraft:overworld".into());
            assert_eq!(
                (
                    time.day_time,
                    time.game_time,
                    time.is_day,
                    time.raining,
                    time.thundering
                ),
                (25000, 100000, day, !day, !day)
            );
            assert!(!mod_world::Host::time(&mut state, "minecraft:the_nether".into()).is_day);
            assert_eq!(
                mod_world::Host::time(&mut state, "missing".into()).game_time,
                0
            );
        }
        struct Sky;
        impl WorldView for Sky {
            fn block(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
                None
            }
            fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
                None
            }
            fn has_dimension(&self, _: &str) -> bool {
                true
            }
            fn check_state(&self, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn can_see_sky(&self, dim: &str, x: i32, y: i32, z: i32) -> bool {
                dim == "minecraft:overworld" && [x, y, z] == [1, 65, -1]
            }
        }
        *state.world.lock().unwrap() = Some(Box::new(Sky));
        assert!(mod_world::Host::can_see_sky(
            &mut state,
            "minecraft:overworld".into(),
            1,
            65,
            -1
        ));
        assert!(!mod_world::Host::can_see_sky(
            &mut state,
            "minecraft:the_end".into(),
            1,
            65,
            -1
        ));
        assert!(!mod_world::Host::can_see_sky(
            &mut state,
            "minecraft:overworld".into(),
            1,
            64,
            -1
        ));
    }

    fn point(x: f64, y: f64, z: f64) -> EntityPoint {
        EntityPoint { x, y, z }
    }

    #[test]
    fn attributes_and_heal_validate_names_and_finite_values() {
        let attribute = |name: &str, value| {
            EntityOrder::Attribute(EntityAttributeChange {
                name: name.into(),
                value,
            })
        };
        for name in crate::content::ENTITY_ATTRIBUTES
            .into_iter()
            .filter(|n| !crate::content::TYPE_ONLY_ATTRIBUTES.contains(n))
        {
            for value in [0.0, 500.0, 1024.0] {
                assert!(
                    check_order(&attribute(name, value)).is_ok(),
                    "{name}={value}"
                );
            }
        }
        for name in ["", "scale", "minecraft:max_health", "luck", "unknown"] {
            assert!(check_order(&attribute(name, 10.0)).is_err(), "{name}");
        }
        for value in [-0.1, 1024.1, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(check_order(&attribute("max_health", value)).is_err());
        }
        for value in [0.0, 1.0, -1.0, f32::MAX] {
            assert!(check_order(&EntityOrder::Heal(value)).is_ok());
        }
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(check_order(&EntityOrder::Heal(value)).is_err());
        }
    }

    #[test]
    fn orders_are_checked_against_their_bounds() {
        let walk = |speed| {
            EntityOrder::WalkTo(EntityWalk {
                to: point(0.0, 64.0, 0.0),
                speed,
            })
        };
        assert!(check_order(&walk(1.0)).is_ok());
        assert!(check_order(&walk(5.0)).is_err());
        let far = EntityOrder::WalkTo(EntityWalk {
            to: point(f64::NAN, 0.0, 0.0),
            speed: 1.0,
        });
        assert!(check_order(&far).is_err());
        assert!(check_order(&EntityOrder::Push(point(0.0, 4.0, -4.0))).is_ok());
        assert!(check_order(&EntityOrder::Push(point(0.0, 4.5, 0.0))).is_err());
        assert!(check_order(&EntityOrder::Animate(Some(String::new()))).is_err());
        assert!(check_order(&EntityOrder::Animate(None)).is_ok());
        let area = |radius, amount, fire| {
            EntityOrder::HurtArea(EntityArea {
                center: point(0.0, 0.0, 0.0),
                radius,
                amount,
                fire,
            })
        };
        assert!(check_order(&area(16.0, 1024.0, 30)).is_ok());
        assert!(check_order(&area(17.0, 1.0, 0)).is_err());
        assert!(check_order(&area(2.0, 2000.0, 0)).is_err());
        assert!(check_order(&area(2.0, 1.0, 31)).is_err());
        assert!(own_kind("ruby:golem") && !own_kind("minecraft:zombie"));
        assert!(check_order(&EntityOrder::Scale(2.0)).is_ok());
        assert!(check_order(&EntityOrder::Scale(20.0)).is_err());
        assert!(check_order(&EntityOrder::Target(EntityTarget::NearestPlayer)).is_ok());
        let data =
            |k: &str, v: Option<&str>| EntityOrder::SetData(vec![(k.into(), v.map(Into::into))]);
        assert!(check_order(&data("stage", Some("3"))).is_ok());
        assert!(check_order(&data("stage", None)).is_ok());
        assert!(check_order(&data("", Some("x"))).is_err());
        assert!(check_order(&data("k", Some(&"x".repeat(1025)))).is_err());
        let many: Vec<(String, Option<String>)> = (0..33).map(|i| (i.to_string(), None)).collect();
        assert!(check_order(&EntityOrder::SetData(many)).is_err());
    }
}
