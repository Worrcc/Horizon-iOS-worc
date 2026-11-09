//! A mod run with no game: `cargo horizon-mod test` (docs/mods.md, "Testing a mod").
//!
//! [`TestGame`] loads one `.hmod` from a scratch copy (its `storage` lives and
//! dies with the run) into the same sandbox the game uses, and plays the game's
//! part: the snapshot, ticks, frames, chat lines and commands, and for a
//! server mod the players joining, leaving, chatting and breaking or placing
//! blocks. [`Script`] is a
//! test written in TOML, `tests/<name>.toml` in the mod's folder: steps, each
//! saying what happens and what the mod must have shown by then.
use crate::{
    BlockFace, BlockPos, BlockTable, ChunkColumns, ClientDimension, ClientTexture, Clock,
    ColumnLayer, Corner, FaceTint, GameMode, Mods, NO_SPRITE, NO_TOP, Output, Panel, Player,
    RenderCorner, ServerPlayer, Snapshot, Status, state_flags,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

/// Inventory fixture for script/check runs without game resources. Validates identifier
/// syntax (the game checks the registry), with persistent counts and no capacity limit.
#[derive(Clone, Default)]
pub struct InventoryFixture {
    pub players: std::collections::BTreeMap<String, bool>,
    pub counts: std::sync::Arc<std::sync::Mutex<std::collections::BTreeMap<(String, String), u32>>>,
}
impl InventoryFixture {
    fn check(&self, player: &str, item: &str) -> Result<bool, String> {
        let plain = |s: &str, slash| {
            !s.is_empty()
                && s.bytes().all(|b| {
                    b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || b"_.-".contains(&b)
                        || slash && b == b'/'
                })
        };
        if !item
            .split_once(':')
            .is_some_and(|(ns, path)| plain(ns, false) && plain(path, true))
        {
            return Err(format!("invalid item id {item}"));
        }
        self.players
            .get(player)
            .copied()
            .ok_or_else(|| format!("no online player {player}"))
    }
}
impl crate::PlayerInventories for InventoryFixture {
    fn take_item(&mut self, player: &str, item: &str, count: u32) -> Result<u32, String> {
        if self.check(player, item)? {
            return Ok(count);
        }
        let mut counts = self.counts.lock().map_err(|_| "inventories unavailable")?;
        let available = counts.entry((player.into(), item.into())).or_default();
        let taken = (*available).min(count);
        *available -= taken;
        Ok(taken)
    }
    fn give_item(&mut self, player: &str, item: &str, count: u32) -> Result<(), String> {
        self.check(player, item)?;
        let mut counts = self.counts.lock().map_err(|_| "inventories unavailable")?;
        let available = counts.entry((player.into(), item.into())).or_default();
        *available = available
            .checked_add(count)
            .ok_or("fixture inventory count overflow")?;
        Ok(())
    }
}

/// One mod in the sandbox, driven by a test.
pub struct TestGame {
    mods: Mods,
    inventories: InventoryFixture,
    dir: PathBuf,
    hmod: PathBuf,
    /// What the mod reads with `game-read`; ticks move the clock as the game does.
    pub player: Option<Player>,
    pub clock: Option<Clock>,
    /// The players on the server, for a server mod, in join order.
    pub players: Vec<ServerPlayer>,
    /// Every name that joined, for their id.
    names: Vec<String>,
    tick: u64,
    /// Capability `world`'s Overworld: the blocks a script set or the mod
    /// changed; every other block, and the other dimensions, are air. Every
    /// biome is the dimension's plainest: plains, nether wastes, the end.
    pub world: std::collections::BTreeMap<(i32, i32, i32), String>,
    /// Capability `world-read`'s block states, by id: air first, then each
    /// state a script set, in the order they came.
    pub states: Vec<String>,
    /// Each chunk's change counter (`client-world.changes`), and the last.
    pub revisions: std::collections::BTreeMap<(i32, i32), u64>,
    pub revision: u64,
    /// `client-world.texture`'s textures: one colour each (0xAARRGGBB), square.
    pub textures: std::collections::BTreeMap<String, TextureSet>,
}

/// The world a client mod reads with capability `world-read`: the script's
/// Overworld blocks (every chunk counts as loaded, all air but for them),
/// lit by the sky, plains everywhere; its block table has a block per state
/// id, each drawing its own `block/<name>` texture.
struct ScriptClientWorld {
    blocks: std::collections::BTreeMap<(i32, i32, i32), String>,
    states: Vec<String>,
    revisions: std::collections::BTreeMap<(i32, i32), u64>,
    revision: u64,
    textures: std::collections::BTreeMap<String, TextureSet>,
    dimension: Option<String>,
}
impl ScriptClientWorld {
    fn id(&self, state: &str) -> u32 {
        self.states.iter().position(|s| s == state).unwrap_or(0) as u32
    }
    /// The block names of the states, in order, and each state's index.
    fn blocks(&self) -> (Vec<String>, Vec<u16>) {
        let mut blocks: Vec<String> = Vec::new();
        let mut of = Vec::new();
        for s in &self.states {
            let name = s.split('[').next().unwrap_or(s).to_owned();
            let i = match blocks.iter().position(|b| *b == name) {
                Some(i) => i,
                None => {
                    blocks.push(name);
                    blocks.len() - 1
                }
            };
            of.push(i as u16);
        }
        (blocks, of)
    }
}
impl crate::ClientWorldView for ScriptClientWorld {
    fn dimension(&self) -> Option<ClientDimension> {
        let id = self.dimension.clone()?;
        let (min_y, height, ceiling) = match id.as_str() {
            "minecraft:the_nether" => (0, 256, true),
            "minecraft:the_end" => (0, 256, false),
            _ => (-64, 384, false),
        };
        Some(ClientDimension {
            id,
            min_y,
            height,
            ceiling,
        })
    }
    fn block_table(&mut self) -> BlockTable {
        let (blocks, state_block) = self.blocks();
        let sprites = blocks
            .iter()
            .map(|b| format!("block/{}", b.split(':').nth(1).unwrap_or(b)))
            .collect();
        let face = |state: &str, block: u16, top: bool| {
            let tint = match state.split('[').next().unwrap_or(state) {
                "minecraft:grass_block" if top => FaceTint::Grass,
                "minecraft:water" => FaceTint::Water,
                _ => FaceTint::None,
            };
            let sprite = if state == "minecraft:air" {
                NO_SPRITE
            } else {
                u32::from(block)
            };
            BlockFace { sprite, tint }
        };
        let flags = |state: &str| match state.split('[').next().unwrap_or(state) {
            "minecraft:air" => state_flags::AIR,
            "minecraft:water" => state_flags::WATER,
            "minecraft:lava" => state_flags::LAVA,
            _ => state_flags::FULL_TOP,
        };
        let n = self.states.len();
        BlockTable {
            version: n as u64,
            state_flags: self.states.iter().map(|s| flags(s)).collect(),
            state_top: (self.states.iter().zip(&state_block))
                .map(|(s, &b)| face(s, b, true))
                .collect(),
            state_side: (self.states.iter().zip(&state_block))
                .map(|(s, &b)| face(s, b, false))
                .collect(),
            constant_tint: vec![0; n],
            map_color: vec![0; n],
            blocks,
            sprites,
            state_block,
        }
    }
    fn table_version(&mut self) -> u64 {
        self.states.len() as u64
    }
    fn texture(&mut self, name: &str) -> Option<ClientTexture> {
        let t = self.textures.get(name)?;
        let [a, r, g, b] = t.color.to_be_bytes();
        Some(ClientTexture {
            width: t.size,
            height: t.size,
            rgba: [r, g, b, a].repeat((t.size * t.size) as usize),
        })
    }
    fn columns(
        &mut self,
        (cx, cz): (i32, i32),
        layer: ColumnLayer,
        depth: u8,
    ) -> Option<ChunkColumns> {
        let overworld = self.dimension.as_deref() == Some("minecraft:overworld");
        let depth = usize::from(depth);
        let mut out = ChunkColumns {
            revision: self.revisions.get(&(cx, cz)).copied().unwrap_or(1),
            tops: vec![NO_TOP; 256],
            states: vec![0; 256 * depth],
            water: vec![0; 256],
            light: vec![0xF0; 256],
            tints: [0x91_BD59, 0x77_AB2F, 0x3F_76E4].repeat(256),
        };
        if !overworld {
            return Some(out);
        }
        let below = match layer {
            ColumnLayer::Surface => i32::MAX,
            ColumnLayer::Below(y) => y,
        };
        let at = |x: i32, y: i32, z: i32| {
            self.blocks
                .get(&(x, y, z))
                .map_or("minecraft:air", String::as_str)
        };
        for i in 0..256 {
            let (x, z) = (cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32);
            let top = (self.blocks.iter())
                .filter(|((bx, by, bz), s)| {
                    *bx == x && *bz == z && *by <= below && s.as_str() != "minecraft:air"
                })
                .map(|((_, y, _), _)| *y)
                .max();
            let Some(top) = top else {
                continue;
            };
            out.tops[i] = top.clamp(i16::MIN.into(), i16::MAX.into()) as i16;
            for k in 0..depth {
                out.states[i * depth + k] = self.id(at(x, top - k as i32, z));
            }
            let water = (0..255)
                .take_while(|k| at(x, top - k, z) == "minecraft:water")
                .count();
            out.water[i] = water as u8;
        }
        Some(out)
    }
    fn changes(&mut self, since: u64) -> (u64, Vec<(i32, i32)>) {
        let changed = (self.revisions.iter())
            .filter(|(_, r)| **r > since)
            .map(|(c, _)| *c)
            .collect();
        (self.revision, changed)
    }
    fn light(&self, _x: i32, _y: i32, _z: i32) -> Option<u8> {
        self.dimension.as_ref().map(|_| 0xF0)
    }
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<String> {
        let d = self.dimension.as_deref()?;
        crate::WorldView::biome(&ScriptWorld(Default::default()), d, x, y, z)
    }
}

/// The world a script's mod reads: [`TestGame::world`], copied for a step.
struct ScriptWorld(std::collections::BTreeMap<(i32, i32, i32), String>);

impl crate::WorldView for ScriptWorld {
    fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
        let here = (dimension == "minecraft:overworld")
            .then(|| self.0.get(&(x, y, z)).cloned())
            .flatten();
        Some(here.unwrap_or_else(|| "minecraft:air".into()))
    }
    fn biome(&self, dimension: &str, _x: i32, _y: i32, _z: i32) -> Option<String> {
        let biome = match dimension {
            "minecraft:overworld" => "minecraft:plains",
            "minecraft:the_nether" => "minecraft:nether_wastes",
            "minecraft:the_end" => "minecraft:the_end",
            _ => return None,
        };
        Some(biome.into())
    }
    fn has_dimension(&self, dimension: &str) -> bool {
        matches!(
            dimension,
            "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
        )
    }
    fn check_state(&self, state: &str) -> Result<(), String> {
        // No registry here: the shape only (the game checks the names).
        let name = state.split('[').next().unwrap_or_default();
        let plain = |t: &str| {
            !t.is_empty()
                && t.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b))
        };
        let ok = match name.split_once(':') {
            Some((ns, path)) => plain(ns) && plain(path),
            None => plain(name),
        };
        let closed = !state.contains('[') || state.ends_with(']');
        (ok && closed)
            .then_some(())
            .ok_or_else(|| format!("{state} is not a block state"))
    }
}

/// Where a script starts: standing at 0.5 64 0.5 in the Overworld, facing
/// south, full health and food, at the first tick of a new world's first morning.
pub fn default_player() -> Player {
    Player {
        x: 0.5,
        y: 64.0,
        z: 0.5,
        yaw: 0.0,
        pitch: 0.0,
        health: 20.0,
        food: 20,
        dimension: "minecraft:overworld".into(),
    }
}

impl TestGame {
    /// Starts the mod at `hmod`, as the game would with it turned on. Errs
    /// when it doesn't start, with the game's reason.
    pub fn load(hmod: &Path) -> Result<TestGame, String> {
        static RUN: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "horizon-mod-test-{}-{}",
            std::process::id(),
            RUN.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let copy = dir.join(hmod.file_name().unwrap_or("mod.hmod".as_ref()));
        let mut game = TestGame {
            mods: Mods::default(),
            inventories: InventoryFixture::default(),
            hmod: copy.clone(),
            dir,
            player: Some(default_player()),
            clock: Some(Clock {
                game_time: 0,
                day_time: 0,
            }),
            players: Vec::new(),
            names: Vec::new(),
            tick: 0,
            world: Default::default(),
            states: vec!["minecraft:air".into()],
            revisions: Default::default(),
            revision: 1,
            textures: Default::default(),
        };
        copy_mod(hmod, &copy)?;
        game.start()?;
        Ok(game)
    }

    fn start(&mut self) -> Result<(), String> {
        self.mods = Mods::default();
        // `init` reads the client world as it is at the start.
        self.snapshot();
        let hmod = std::slice::from_ref(&self.hmod);
        self.mods
            .load_into(crate::Runs::Both, hmod, |_| true, |_| false);
        self.mods.take_client_world();
        match &self.mods.entries()[0].status {
            Status::Failed(reason) => Err(format!("the mod didn't start: {reason}")),
            _ if !self.mods.is_running() => Err("the mod has no code to run".into()),
            _ => Ok(()),
        }
    }

    /// Stops the mod and starts it again, as a rebuild does in the game: its
    /// memory starts over, its `storage` stays.
    pub fn reload(&mut self) -> Result<(), String> {
        self.mods = Mods::default();
        self.start()
    }

    fn snapshot(&mut self) {
        self.inventories.players = self
            .players
            .iter()
            .map(|p| (p.id.clone(), matches!(p.game_mode, GameMode::Creative)))
            .collect();
        self.mods
            .lend_inventories(Box::new(self.inventories.clone()));
        self.mods.set_snapshot(Snapshot {
            player: self.player.clone(),
            clock: self.clock,
            players: self.players.clone(),
            // The GUI of a 1280 x 720 window at scale 3, as the trial run's.
            gui_size: (427.0, 240.0),
            world_id: self.player.as_ref().map(|_| "sp:Test World".into()),
            ..Snapshot::default()
        });
        self.mods.lend_client_world(Box::new(ScriptClientWorld {
            blocks: self.world.clone(),
            states: self.states.clone(),
            revisions: self.revisions.clone(),
            revision: self.revision,
            textures: self.textures.clone(),
            dimension: self.player.as_ref().map(|p| p.dimension.clone()),
        }));
    }
    /// Sets a block of the Overworld, as a script's `blocks` do: its chunk's
    /// change counter moves on, and a new state gets the next id.
    pub fn set_block(&mut self, x: i32, y: i32, z: i32, state: &str) {
        if !self.states.iter().any(|s| s == state) {
            self.states.push(state.to_owned());
        }
        self.world.insert((x, y, z), state.to_owned());
        self.revision += 1;
        self.revisions.insert((x >> 4, z >> 4), self.revision);
    }
    /// The mod's canvas box: its corner, size and shapes.
    pub fn canvas(&self) -> Option<&crate::CanvasPanel> {
        self.mods.canvas_panels().next().map(|(_, p)| p)
    }
    /// The mod's full-window canvas, until closed.
    pub fn canvas_screen(&self) -> Option<&crate::CanvasScreen> {
        self.mods.canvas_screen(0)
    }
    /// A pointer action in GUI pixels on the open canvas screen.
    pub fn pointer(&mut self, kind: crate::PointerKind, x: f32, y: f32, button: u32) {
        self.snapshot();
        self.mods.ui_pointer(0, kind, x, y, button);
    }
    /// Wheel steps at GUI coordinates on the open canvas screen.
    pub fn scroll(&mut self, x: f32, y: f32, steps: f32) {
        self.snapshot();
        self.mods.ui_scroll(0, x, y, steps);
    }
    /// A canvas key; Escape leaves the screen and sends closed.
    pub fn screen_key(&mut self, key: &str, down: bool) {
        self.snapshot();
        if key == "key.keyboard.escape" && down && self.canvas_screen().is_some() {
            self.mods.ui_closed(0);
        } else {
            self.mods.ui_screen_key(0, key, down);
        }
    }
    /// The texels the mod's images sent since the last call, as the game
    /// writes them into its atlas once a frame.
    pub fn canvas_uploads(&mut self) -> Vec<crate::CanvasUpload> {
        self.mods.take_canvas_uploads(false)
    }
    /// Whether the mod is a server mod.
    pub fn server(&self) -> bool {
        self.mods.runs_server()
    }

    /// `n` game ticks; each moves the clock one tick on, as the game does.
    pub fn tick(&mut self, n: u64) {
        for _ in 0..n {
            self.tick += 1;
            if let Some(clock) = &mut self.clock {
                clock.game_time += 1;
                clock.day_time += 1;
            }
            self.snapshot();
            self.mods.tick(self.tick);
            self.mods.server_tick(self.tick);
            // A frame is drawn between ticks: it takes the images' texels.
            self.mods.take_canvas_uploads(false);
        }
    }
    /// `n` frames, 1/60 s apart.
    pub fn frames(&mut self, n: u64) {
        self.snapshot();
        for _ in 0..n {
            self.mods.frame(1.0 / 60.0);
            self.mods.take_canvas_uploads(false);
        }
    }
    /// A chat line the player receives.
    pub fn chat(&mut self, line: &str) {
        self.snapshot();
        self.mods.chat(line);
    }
    /// `/line` typed by the player (without the slash); false when the mod
    /// owns no such command.
    pub fn command(&mut self, line: &str) -> bool {
        self.snapshot();
        self.mods.command(line)
    }

    /// A player named `name` joins the server, standing where a script starts
    /// in survival. Their id is `00000000-0000-4000-8000-000000000001` for
    /// the first name that joined, `...0002` for the second, and so on.
    pub fn join(&mut self, name: &str) -> ServerPlayer {
        let n = match self.names.iter().position(|n| n == name) {
            Some(i) => i + 1,
            None => {
                self.names.push(name.to_owned());
                self.names.len()
            }
        };
        let p = default_player();
        let player = ServerPlayer {
            id: format!("00000000-0000-4000-8000-{n:012x}"),
            name: name.to_owned(),
            x: p.x,
            y: p.y,
            z: p.z,
            dimension: p.dimension,
            game_mode: GameMode::Survival,
            health: p.health,
        };
        self.players.push(player.clone());
        self.snapshot();
        self.mods.player_joined(&player);
        player
    }
    /// The player named `name` leaves; false when no such player is on.
    pub fn leave(&mut self, name: &str) -> bool {
        let Some(i) = self.players.iter().position(|p| p.name == name) else {
            return false;
        };
        let player = self.players.remove(i);
        self.snapshot();
        self.mods.player_left(&player);
        true
    }
    /// A chat message `player` sends, for a server mod.
    pub fn player_chat(&mut self, player: &ServerPlayer, line: &str) {
        self.snapshot();
        self.mods.player_chat(player, line);
    }
    /// `/line` typed by `player`, for a server mod; false when the mod owns
    /// no such command.
    pub fn player_command(&mut self, player: &ServerPlayer, line: &str) -> bool {
        self.snapshot();
        self.mods.server_command(player, line)
    }
    /// `player` breaks (or places) `state` at `pos`.
    pub fn block(&mut self, player: &ServerPlayer, pos: BlockPos, state: &str, placed: bool) {
        self.snapshot();
        if placed {
            self.mods.block_placed(player, pos, state);
        } else {
            self.mods.block_broken(player, pos, state);
        }
    }

    /// The scratch copy the mod runs from; its `storage` is beside it.
    pub fn hmod(&self) -> &Path {
        &self.hmod
    }
    /// The commands the mod registered.
    pub fn commands(&self) -> Vec<String> {
        let server = self.mods.server_commands().map(|c| c.name.as_str());
        self.mods
            .commands()
            .chain(server)
            .map(str::to_owned)
            .collect()
    }
    pub fn panel(&self) -> Option<&Panel> {
        self.mods.panels().next()
    }
    pub fn status(&self) -> &Status {
        &self.mods.entries()[0].status
    }
    /// What the mod showed since the last call.
    pub fn drain(&mut self) -> Vec<Output> {
        self.mods.drain()
    }
}

impl Drop for TestGame {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A `.hmod` file, or a mod folder (`horizon-mod.toml` + `mod.wasm`).
fn copy_mod(from: &Path, to: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{}: {e}", from.display());
    if !from.is_dir() {
        return std::fs::copy(from, to).map(drop).map_err(err);
    }
    std::fs::create_dir_all(to).map_err(err)?;
    for entry in std::fs::read_dir(from).map_err(err)? {
        let entry = entry.map_err(err)?;
        if entry.file_type().map_err(err)?.is_file() {
            std::fs::copy(entry.path(), to.join(entry.file_name())).map_err(err)?;
        }
    }
    Ok(())
}

/// A test in TOML: where a client mod's player starts, then steps run in order.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Script {
    #[serde(default)]
    pub player: PlayerSet,
    #[serde(default)]
    pub clock: ClockSet,
    #[serde(default, rename = "step")]
    pub steps: Vec<Step>,
    /// Textures capability `world-read` reads, by name (`block/stone`).
    #[serde(default)]
    pub textures: std::collections::BTreeMap<String, TextureSet>,
}

/// A texture of one colour: `{ size = 16, color = 0xFF7F7F7F }` (0xAARRGGBB).
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextureSet {
    #[serde(default = "texture_size")]
    pub size: u32,
    pub color: u32,
}
fn texture_size() -> u32 {
    16
}

/// Fields of the player to change; the others keep their values. A server
/// mod's players have no `yaw`, `pitch` or `food`, and a client mod's player
/// no `game-mode`.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PlayerSet {
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    yaw: Option<f32>,
    pitch: Option<f32>,
    health: Option<f32>,
    food: Option<u32>,
    dimension: Option<String>,
    game_mode: Option<Mode>,
}

/// `players.game-mode` as a script spells it.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Survival,
    Creative,
    Adventure,
    Spectator,
}
impl From<Mode> for GameMode {
    fn from(m: Mode) -> GameMode {
        match m {
            Mode::Survival => GameMode::Survival,
            Mode::Creative => GameMode::Creative,
            Mode::Adventure => GameMode::Adventure,
            Mode::Spectator => GameMode::Spectator,
        }
    }
}

/// A block a player breaks or places.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockSet {
    x: i32,
    y: i32,
    z: i32,
    state: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ClockSet {
    game_time: Option<u64>,
    day_time: Option<u64>,
}

/// One step. Its parts happen in this order: `reload`, `title-screen`,
/// `blocks`, `join`, `player`, `clock`, `ticks`, `frames`, `chat`, `command`,
/// `pointer`, `scroll`, `screen-key`, `break`, `place`, `leave`; then the blocks the mod set are made, and
/// `expect` is checked against what the mod showed and changed during the
/// step. For a server mod, `player`, `chat`, `command`, `break`
/// and `place` are the doing of the player named by `as`, or of the first
/// player on.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Step {
    #[serde(default)]
    reload: bool,
    /// Leave the world: the mod reads no player and no clock until a step sets them.
    #[serde(default)]
    title_screen: bool,
    player: Option<PlayerSet>,
    clock: Option<ClockSet>,
    #[serde(default)]
    ticks: u64,
    #[serde(default)]
    frames: u64,
    chat: Option<String>,
    command: Option<String>,
    /// Canvas screen events, after commands and before assertions.
    pointer: Option<PointerStep>,
    scroll: Option<ScrollStep>,
    screen_key: Option<ScreenKeyStep>,
    /// Server mods: a player joins, leaves, breaks or places a block.
    join: Option<String>,
    leave: Option<String>,
    #[serde(rename = "as")]
    actor: Option<String>,
    #[serde(rename = "break")]
    broken: Option<BlockSet>,
    place: Option<BlockSet>,
    /// Blocks of the Overworld before the rest of the step (capability
    /// `world`); every other block is air.
    #[serde(default)]
    blocks: Vec<BlockSet>,
    #[serde(default)]
    expect: Expect,
}

/// A canvas pointer action in GUI pixels.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointerStep {
    kind: PointerAction,
    x: f32,
    y: f32,
    #[serde(default)]
    button: u32,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum PointerAction {
    Down,
    Up,
    Drag,
    Move,
}
impl From<&PointerAction> for crate::PointerKind {
    fn from(kind: &PointerAction) -> Self {
        match kind {
            PointerAction::Down => Self::Down,
            PointerAction::Up => Self::Up,
            PointerAction::Drag => Self::Drag,
            PointerAction::Move => Self::Move,
        }
    }
}
/// Wheel steps at GUI coordinates.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScrollStep {
    x: f32,
    y: f32,
    steps: f32,
}
/// A key.keyboard.* press or release.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenKeyStep {
    key: String,
    down: bool,
}

/// What a step checks; a part left out is not checked.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Expect {
    /// The commands the mod owns.
    commands: Option<Vec<String>>,
    /// Every chat line shown during the step, in order.
    chat: Option<Vec<String>>,
    /// The command's feedback, and its red error.
    feedback: Option<String>,
    error: Option<String>,
    /// Server mods: every `players.tell` as `"Name: text"`, and every
    /// `players.broadcast`, in order.
    told: Option<Vec<String>>,
    broadcast: Option<Vec<String>>,
    /// The panel's lines (`[]`: no panel) and its corner.
    panel: Option<Vec<String>>,
    corner: Option<String>,
    /// Blocks of the Overworld after the step, the mod's changes made
    /// (capability `world`).
    blocks: Option<Vec<BlockSet>>,
    /// The mod is turned off during this step, for a reason holding this text.
    /// Without it, a mod that stops fails the test.
    stopped: Option<String>,
    /// The mod's canvas box (capability `render`); with no box, any part
    /// asked for fails.
    canvas: Option<CanvasExpect>,
    canvas_screen: Option<CanvasScreenExpect>,
}

/// What a step checks of the canvas box; a part left out is not checked.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CanvasExpect {
    placed: Option<bool>,
    dx: Option<f32>,
    dy: Option<f32>,
    /// The clamped top left for the runner's 427 × 240 GUI.
    x: Option<f32>,
    y: Option<f32>,
    corner: Option<String>,
    width: Option<f32>,
    height: Option<f32>,
    /// Shapes in the box, and of them the images that exist.
    shapes: Option<usize>,
    images: Option<usize>,
    /// Image shapes clipped round.
    round: Option<usize>,
}

/// The open full-window canvas; zero shapes can also assert its absence.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct CanvasScreenExpect {
    open: Option<bool>,
    title: Option<String>,
    shapes: Option<usize>,
    texts: Option<Vec<String>>,
}
impl CanvasScreenExpect {
    fn check(&self, game: &TestGame) -> Result<(), String> {
        let screen = game.canvas_screen();
        if let Some(open) = self.open {
            same("canvas screen open", &open, &screen.is_some())?;
        }
        if let Some(title) = &self.title {
            same(
                "canvas screen title",
                title,
                &screen.map_or(String::new(), |s| s.title.clone()),
            )?;
        }
        if let Some(shapes) = self.shapes {
            same(
                "canvas screen shapes",
                &shapes,
                &screen.map_or(0, |s| s.shapes.len()),
            )?;
        }
        if let Some(texts) = &self.texts {
            let actual: Vec<String> = screen
                .into_iter()
                .flat_map(|s| &s.shapes)
                .filter_map(|s| match s {
                    crate::CanvasShape::Text(t) => Some(t.text.clone()),
                    _ => None,
                })
                .collect();
            same("canvas screen texts", texts, &actual)?;
        }
        Ok(())
    }
}

impl Script {
    pub fn parse(text: &str) -> Result<Script, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    /// Runs every step on `game`; the error names the first step that failed.
    pub fn run(&self, game: &mut TestGame) -> Result<(), String> {
        if let Some(p) = &mut game.player {
            self.player.apply(p);
        }
        if let Some(c) = &mut game.clock {
            self.clock.apply(c);
        }
        game.textures.clone_from(&self.textures);
        for (i, step) in self.steps.iter().enumerate() {
            step.run(game)
                .map_err(|e| format!("step {} ({}): {e}", i + 1, step.describe()))?;
        }
        Ok(())
    }
}

impl PlayerSet {
    fn apply(&self, p: &mut Player) {
        let set = |to: &mut f64, v: Option<f64>| *to = v.unwrap_or(*to);
        set(&mut p.x, self.x);
        set(&mut p.y, self.y);
        set(&mut p.z, self.z);
        p.yaw = self.yaw.unwrap_or(p.yaw);
        p.pitch = self.pitch.unwrap_or(p.pitch);
        p.health = self.health.unwrap_or(p.health);
        p.food = self.food.unwrap_or(p.food);
        if let Some(d) = &self.dimension {
            p.dimension.clone_from(d);
        }
    }
    fn apply_server(&self, p: &mut ServerPlayer) {
        let set = |to: &mut f64, v: Option<f64>| *to = v.unwrap_or(*to);
        set(&mut p.x, self.x);
        set(&mut p.y, self.y);
        set(&mut p.z, self.z);
        p.health = self.health.unwrap_or(p.health);
        p.game_mode = self.game_mode.map_or(p.game_mode, GameMode::from);
        if let Some(d) = &self.dimension {
            p.dimension.clone_from(d);
        }
    }
}

impl ClockSet {
    fn apply(&self, c: &mut Clock) {
        c.game_time = self.game_time.unwrap_or(c.game_time);
        c.day_time = self.day_time.unwrap_or(c.day_time);
    }
}

impl Step {
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        if self.reload {
            parts.push("reload".to_string());
        }
        let count = |n: u64, what: &str| format!("{n} {what}{}", if n == 1 { "" } else { "s" });
        if self.ticks > 0 {
            parts.push(count(self.ticks, "tick"));
        }
        if self.frames > 0 {
            parts.push(count(self.frames, "frame"));
        }
        if let Some(line) = &self.chat {
            parts.push(format!("chat {line:?}"));
        }
        if let Some(line) = &self.command {
            parts.push(format!("/{line}"));
        }
        if let Some(name) = &self.join {
            parts.insert(0, format!("{name} joins"));
        }
        for (what, b) in [("breaks", &self.broken), ("places", &self.place)] {
            if let Some(b) = b {
                parts.push(format!("{what} {} at {} {} {}", b.state, b.x, b.y, b.z));
            }
        }
        if let Some(name) = &self.leave {
            parts.push(format!("{name} leaves"));
        }
        if parts.is_empty() {
            parts.push("checks".into());
        }
        parts.join(", ")
    }

    fn run(&self, game: &mut TestGame) -> Result<(), String> {
        if self.reload {
            game.reload()?;
        }
        if self.title_screen {
            game.player = None;
            game.clock = None;
        }
        for b in &self.blocks {
            game.set_block(b.x, b.y, b.z, &b.state);
        }
        game.mods
            .lend_world(Box::new(ScriptWorld(game.world.clone())));
        if let Some(name) = &self.join {
            game.join(name);
        }
        let server = game.server();
        let actor = |game: &TestGame| match &self.actor {
            Some(name) => game
                .players
                .iter()
                .position(|p| p.name == *name)
                .ok_or_else(|| format!("{name} is not on the server")),
            None => (!game.players.is_empty())
                .then_some(0)
                .ok_or_else(|| "no player is on the server: add join = \"Name\"".to_string()),
        };
        let acts = self.player.is_some()
            || self.chat.is_some()
            || self.command.is_some()
            || self.broken.is_some()
            || self.place.is_some();
        let actor = if server && acts {
            Some(actor(game)?)
        } else {
            None
        };
        if let Some(set) = &self.player {
            match actor {
                Some(i) => set.apply_server(&mut game.players[i]),
                None => set.apply(game.player.get_or_insert_with(default_player)),
            }
        }
        if let Some(set) = &self.clock {
            set.apply(game.clock.get_or_insert(Clock {
                game_time: 0,
                day_time: 0,
            }));
        }
        game.tick(self.ticks);
        game.frames(self.frames);
        let who = actor.map(|i| game.players[i].clone());
        if let Some(line) = &self.chat {
            match &who {
                Some(p) => game.player_chat(p, line),
                None => game.chat(line),
            }
        }
        let owned = self.command.as_ref().map(|line| match &who {
            Some(p) => game.player_command(p, line),
            None => game.command(line),
        });
        if let Some(p) = &self.pointer {
            game.pointer((&p.kind).into(), p.x, p.y, p.button);
        }
        if let Some(s) = &self.scroll {
            game.scroll(s.x, s.y, s.steps);
        }
        if let Some(k) = &self.screen_key {
            game.screen_key(&k.key, k.down);
        }
        for (b, placed) in [(&self.broken, false), (&self.place, true)] {
            if let (Some(b), Some(p)) = (b, &who) {
                let pos = BlockPos {
                    x: b.x,
                    y: b.y,
                    z: b.z,
                };
                game.block(p, pos, &b.state, placed);
            }
        }
        if let Some(name) = &self.leave
            && !game.leave(name)
        {
            return Err(format!("{name} is not on the server"));
        }
        game.mods.take_world();
        game.mods.take_client_world();
        let out = game.drain();
        for o in &out {
            if let Output::SetBlocks {
                dimension, blocks, ..
            } = o
                && dimension == "minecraft:overworld"
            {
                for b in blocks {
                    game.world.insert((b.x, b.y, b.z), b.state.clone());
                }
            }
        }
        let stopped = out.iter().find_map(|o| match o {
            Output::Stopped { reason, .. } => Some(reason.as_str()),
            _ => None,
        });
        let e = &self.expect;
        match (stopped, &e.stopped) {
            (Some(reason), Some(want)) if !reason.contains(want.as_str()) => {
                return Err(format!("the mod stopped ({reason}), expected {want:?}"));
            }
            (Some(_), Some(_)) => return Ok(()),
            (Some(reason), None) => return Err(format!("the mod stopped: {reason}")),
            (None, Some(want)) => {
                return Err(format!(
                    "the mod kept running, expected it to stop ({want:?})"
                ));
            }
            (None, None) => {}
        }
        if owned == Some(false) {
            let line = self.command.as_deref().unwrap_or_default();
            let name = line.split(' ').next().unwrap_or_default();
            return Err(format!("the mod owns no command /{name}"));
        }
        if let Some(want) = &e.commands {
            same("commands", want, &game.commands())?;
        }
        if let Some(want) = &e.chat {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Chat { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            same("chat", want, &got)?;
        }
        let feedback = |error: bool| {
            out.iter().rev().find_map(|o| match o {
                Output::Feedback { text, error: e } | Output::Reply { text, error: e, .. }
                    if *e == error =>
                {
                    Some(text.clone())
                }
                _ => None,
            })
        };
        if let Some(want) = &e.told {
            let name = |id: &str| {
                let p = game.players.iter().find(|p| p.id == id);
                p.map_or(id.to_owned(), |p| p.name.clone())
            };
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Tell { player, text, .. } => Some(format!("{}: {text}", name(player))),
                    _ => None,
                })
                .collect();
            same("told", want, &got)?;
        }
        if let Some(want) = &e.broadcast {
            let got: Vec<String> = out
                .iter()
                .filter_map(|o| match o {
                    Output::Broadcast { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            same("broadcast", want, &got)?;
        }
        if let Some(want) = &e.feedback {
            same("feedback", &Some(want.clone()), &feedback(false))?;
        }
        if let Some(want) = &e.error {
            same("error", &Some(want.clone()), &feedback(true))?;
        }
        if let Some(want) = &e.panel {
            let got: Vec<String> = game
                .panel()
                .map(|p| p.lines.iter().map(|l| l.text.clone()).collect())
                .unwrap_or_default();
            same("panel", want, &got)?;
        }
        if let Some(want) = &e.blocks {
            for b in want {
                let got = game.world.get(&(b.x, b.y, b.z));
                let got = got.map_or("minecraft:air", String::as_str);
                if got != b.state {
                    return Err(format!(
                        "block at {} {} {}: expected {}, got {got}",
                        b.x, b.y, b.z, b.state
                    ));
                }
            }
        }
        if let Some(want) = &e.corner {
            let got = game.panel().map(|p| corner_name(p.corner).to_string());
            same("corner", &Some(want.clone()), &got)?;
        }
        if let Some(want) = &e.canvas_screen {
            want.check(game)?;
        }
        if let Some(want) = &e.canvas {
            want.check(game)?;
        }
        Ok(())
    }
}

impl CanvasExpect {
    fn check(&self, game: &TestGame) -> Result<(), String> {
        let panel = game.mods.canvas_panels().next();
        let Some((owner, p)) = panel else {
            let any = self.placed.is_some()
                || self.dx.is_some()
                || self.dy.is_some()
                || self.x.is_some()
                || self.y.is_some()
                || self.corner.is_some()
                || self.width.is_some()
                || self.height.is_some()
                || self.shapes.is_some_and(|n| n > 0)
                || self.images.is_some_and(|n| n > 0);
            return if any {
                Err("canvas: the mod shows no canvas box".into())
            } else {
                Ok(())
            };
        };
        if let Some(want) = self.placed {
            same("canvas placed", &want, &p.placed.is_some())?;
        }
        let origin = p.placed_origin((427.0, 240.0));
        for (label, want, actual) in [
            ("canvas dx", self.dx, p.placed.map(|v| v.0)),
            ("canvas dy", self.dy, p.placed.map(|v| v.1)),
            ("canvas x", self.x, origin.map(|v| v.0)),
            ("canvas y", self.y, origin.map(|v| v.1)),
        ] {
            if let Some(want) = want {
                same(label, &Some(want), &actual)?;
            }
        }
        let images: Vec<_> = (p.shapes.iter())
            .filter_map(|s| match s {
                crate::CanvasShape::Image(i) => Some(i),
                _ => None,
            })
            .collect();
        let existing = (images.iter())
            .filter(|i| game.mods.canvas_image(owner, i.image).is_some())
            .count();
        let round = images.iter().filter(|i| i.round).count();
        if let Some(want) = &self.corner {
            same(
                "canvas corner",
                want,
                &render_corner_name(p.corner).to_string(),
            )?;
        }
        if let Some(want) = self.width {
            same("canvas width", &want, &p.width)?;
        }
        if let Some(want) = self.height {
            same("canvas height", &want, &p.height)?;
        }
        if let Some(want) = self.shapes {
            same("canvas shapes", &want, &p.shapes.len())?;
        }
        if let Some(want) = self.images {
            same("canvas images", &want, &existing)?;
        }
        if let Some(want) = self.round {
            same("canvas round images", &want, &round)?;
        }
        Ok(())
    }
}

fn render_corner_name(c: RenderCorner) -> &'static str {
    match c {
        RenderCorner::TopLeft => "top-left",
        RenderCorner::TopRight => "top-right",
        RenderCorner::BottomLeft => "bottom-left",
        RenderCorner::BottomRight => "bottom-right",
    }
}

fn corner_name(c: Corner) -> &'static str {
    match c {
        Corner::TopLeft => "top-left",
        Corner::TopRight => "top-right",
        Corner::BottomLeft => "bottom-left",
        Corner::BottomRight => "bottom-right",
    }
}

fn same<T: PartialEq + std::fmt::Debug>(what: &str, want: &T, got: &T) -> Result<(), String> {
    if want == got {
        return Ok(());
    }
    Err(format!(
        "{what}\n    expected: {want:?}\n    got:      {got:?}"
    ))
}
