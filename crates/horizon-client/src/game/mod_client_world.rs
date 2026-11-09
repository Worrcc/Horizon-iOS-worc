//! Capability `world-read` (docs/mods.md, `client-world`): the chunks this
//! window has loaded, as a minimap mod reads them, lent to the client mods
//! for their hook calls as server mods are lent the levels
//! (`server_mods.rs`). Everything Horizon-specific a mod reads is adapted
//! here, from `World`, the block facts, the block sprite lookup and the
//! block textures the game keeps on the CPU, so a later merge of the game
//! changes this file and not the runtime's contract.
//!
//! Change counters: one per loaded chunk, from one counter for the window.
//! A chunk's moves on when a block in it is written (`World::set` and bulk
//! edits note the chunk in `World::mod_writes` while a mod reads the world),
//! when it loads, and when it is replaced whole (the server sent it again);
//! an unloaded chunk's moves on once more, so `changes` lists it. Light and
//! biome updates alone don't move it.
#[cfg(feature = "mods")]
use {
    super::{Game, Resources},
    horizon_assets::model::Direction,
    horizon_client::{
        biome::Biomes,
        block_sprites::Lookup,
        chunk_map::{ChunkMap, ChunkMembershipDelta},
        fast_hash::FxHashMap,
        mesher::TintSource,
        streaming::WorkerContext,
    },
    horizon_mods::{
        BlockFace, BlockTable, ChunkColumns, ClientDimension, ClientTexture, ClientWorldView,
        ColumnLayer, FaceTint, NO_SPRITE, NO_TOP, state_flags as flags,
    },
    horizon_world::registry::BlockStateId,
    std::{
        sync::Arc,
        time::{Duration, Instant},
    },
};

/// What the window keeps between lends: the change counters, the block
/// table and the block textures uncompressed, while mods read them.
#[cfg(feature = "mods")]
#[derive(Default)]
pub(super) struct ClientWorldHost {
    revisions: Revisions,
    /// The table for the resources of this generation.
    table: Option<(u64, Arc<BlockTable>)>,
    /// The block sprites' pixels, for `texture`; dropped once unread a while.
    pixels: Option<(u64, Arc<Vec<u8>>, Instant)>,
}

/// Block textures unread this long are let go of (the game holds them
/// compressed).
#[cfg(feature = "mods")]
const PIXELS_KEPT: Duration = Duration::from_secs(30);
/// Unloaded chunks remembered for `changes`.
#[cfg(feature = "mods")]
const GONE_KEPT: usize = 16_384;

/// The change counters ([`ClientWorldHost`]).
#[cfg(feature = "mods")]
#[derive(Default)]
struct Revisions {
    counter: u64,
    /// Each loaded chunk's counter, and the chunk it was taken from (a
    /// chunk sent again is another allocation).
    chunks: FxHashMap<(i32, i32), (u64, usize)>,
    /// Chunks unloaded since, with the counter they went at.
    gone: FxHashMap<(i32, i32), u64>,
    /// The chunk map's membership revision at the last sync.
    membership: u64,
}
#[cfg(feature = "mods")]
impl Revisions {
    fn bump(&mut self) -> u64 {
        self.counter += 1;
        self.counter
    }
    /// Catch up with the world: written, loaded, replaced and unloaded chunks.
    fn sync(&mut self, world: &mut horizon_client::world::World) {
        let writes = world.mod_writes.get_or_insert_with(Default::default);
        for key in std::mem::take(writes) {
            if let Some(entry) = self.chunks.get_mut(&key) {
                self.counter += 1;
                entry.0 = self.counter;
            }
        }
        // Chunks that came in since, even where one went and another took
        // its place (and maybe its address); all of them when the map can't
        // say (another map, or cleared).
        let membership = world.chunks.membership_revision();
        let mut added = Vec::new();
        let mut all = false;
        if membership != self.membership {
            match world.chunks.deltas_since(self.membership) {
                Some(deltas) => added.extend(deltas.filter_map(|(_, d)| match d {
                    ChunkMembershipDelta::Added(k) => Some(k),
                    _ => None,
                })),
                None => all = true,
            }
            self.membership = membership;
        }
        let mut seen = 0;
        for (&key, chunk) in world.chunks.iter() {
            seen += 1;
            let at = Arc::as_ptr(chunk) as usize;
            let came = all || added.contains(&key);
            if came || self.chunks.get(&key).is_none_or(|&(_, was)| was != at) {
                let r = self.bump();
                self.chunks.insert(key, (r, at));
                self.gone.remove(&key);
            }
        }
        if seen != self.chunks.len() {
            let gone: Vec<_> = (self.chunks.keys())
                .filter(|k| !world.chunks.contains_key(k))
                .copied()
                .collect();
            for key in gone {
                self.chunks.remove(&key);
                let r = self.bump();
                self.gone.insert(key, r);
            }
            if self.gone.len() > GONE_KEPT {
                let keep = self.counter.saturating_sub(GONE_KEPT as u64);
                self.gone.retain(|_, r| *r > keep);
            }
        }
    }
    fn changes(&self, since: u64) -> Vec<(i32, i32)> {
        let loaded = self.chunks.iter().map(|(k, (r, _))| (*k, *r));
        let gone = self.gone.iter().map(|(k, r)| (*k, *r));
        loaded
            .chain(gone)
            .filter(|(_, r)| *r > since)
            .map(|(k, _)| k)
            .collect()
    }
}

/// The window's world as lent to the mods: its chunks (taken out of
/// `World` for the calls, as the server mods' levels are), its shape and
/// the resources that draw it.
#[cfg(feature = "mods")]
struct LentClientWorld {
    chunks: ChunkMap,
    revisions: Revisions,
    dimension: Option<ClientDimension>,
    context: Arc<WorkerContext>,
    res: Arc<Resources>,
    biomes: Arc<Biomes>,
    table: Option<(u64, Arc<BlockTable>)>,
    pixels: Option<(u64, Arc<Vec<u8>>, Instant)>,
    /// `state-flags`, for the column scan.
    flags: Option<Arc<Vec<u16>>>,
}

/// `face.tint` and `constant-tint` from a face's tint source.
#[cfg(feature = "mods")]
fn tint(source: Option<TintSource>) -> (FaceTint, u32) {
    match source {
        None => (FaceTint::None, 0),
        Some(TintSource::Grass) => (FaceTint::Grass, 0),
        Some(TintSource::Foliage) => (FaceTint::Foliage, 0),
        Some(TintSource::DryFoliage) => (FaceTint::DryFoliage, 0),
        Some(TintSource::Water) => (FaceTint::Water, 0),
        Some(TintSource::Fixed(c)) => (FaceTint::Constant, c & 0xFF_FFFF),
    }
}

/// The block table of these resources: every state's block, facts and the
/// first face its model draws up and south, as the mesher bakes it.
#[cfg(feature = "mods")]
fn build_table(context: &WorkerContext, res: &Resources) -> BlockTable {
    use horizon_client::blocks::{Fluid, MAP_COLORS, RenderShape};
    let facts = res.facts();
    let render = &context.render;
    let lookup = Lookup::new(context, u32::MAX);
    let mut sprites = vec![String::new(); render.texture_layers as usize];
    for (name, &layer) in &render.sprite_layers {
        if let Some(slot) = sprites.get_mut(layer as usize) {
            *slot = name.trim_start_matches("minecraft:").to_owned();
        }
    }
    let mut blocks: Vec<String> = Vec::new();
    let mut index: FxHashMap<&str, u16> = FxHashMap::default();
    let n = facts.states.len();
    let mut t = BlockTable {
        version: render.generation,
        blocks: Vec::new(),
        sprites: Vec::new(),
        state_block: Vec::with_capacity(n),
        state_flags: Vec::with_capacity(n),
        state_top: Vec::with_capacity(n),
        state_side: Vec::with_capacity(n),
        constant_tint: Vec::with_capacity(n),
        map_color: Vec::with_capacity(n),
    };
    for (i, f) in facts.states.iter().enumerate() {
        let id = BlockStateId(i as u32);
        let block = *index.entry(&f.name).or_insert_with(|| {
            blocks.push(f.name.to_string());
            (blocks.len() - 1) as u16
        });
        let air = facts.is_air(id);
        let mut bits = f.emission.min(15) as u16 * (1 << flags::EMISSION_SHIFT);
        let mut set = |on: bool, bit: u16| {
            if on {
                bits |= bit;
            }
        };
        set(air, flags::AIR);
        set(f.render_shape == RenderShape::Invisible, flags::INVISIBLE);
        set(f.fluid == Fluid::Water, flags::WATER);
        set(f.fluid == Fluid::Lava, flags::LAVA);
        set(f.waterlogged, flags::WATERLOGGED);
        set(!air && !f.can_occlude, flags::SEE_THROUGH);
        set(
            !air && !f.legacy_solid && f.fluid == Fluid::None,
            flags::NO_COLLISION,
        );
        // isFaceSturdy(UP, FULL): Direction.UP's ordinal is 1.
        set(f.sturdy_full >> 1 & 1 != 0, flags::FULL_TOP);
        let sides = if air { None } else { Some(lookup.state(id)) };
        let mut constant = 0;
        let mut face = |side: Direction| {
            let first = sides
                .as_ref()
                .and_then(|s| s.sides[side as usize].first().copied());
            match first {
                None => BlockFace {
                    sprite: NO_SPRITE,
                    tint: FaceTint::None,
                },
                Some(f) => {
                    let (tint, c) = tint(f.tint);
                    if c != 0 {
                        constant = c;
                    }
                    BlockFace {
                        sprite: f.sprite.layer,
                        tint,
                    }
                }
            }
        };
        let (mut up, mut south) = (face(Direction::Up), face(Direction::South));
        if !air && up.sprite == NO_SPRITE && south.sprite == NO_SPRITE {
            // Nothing flat on either side (a flower's crossed planes, a
            // rotated element): the model's first face stands for both, with
            // its tint, as a map averages a plant's loose quads.
            let models = (render.model_catalog)
                .collect(id, horizon_core::position::BlockPos::new(0, 0, 0))
                .unwrap_or_default();
            if let Some(f) = models.iter().flat_map(|(_, m)| &m.faces).next() {
                let (tint, c) = tint(context.tables.tint_source(id, f.quad.tint_index));
                if c != 0 {
                    constant = c;
                }
                up = BlockFace {
                    sprite: f.layer,
                    tint,
                };
                south = up;
            }
        }
        t.state_top.push(up);
        t.state_side.push(south);
        t.state_block.push(block);
        t.state_flags.push(bits);
        t.constant_tint.push(constant);
        t.map_color.push(
            MAP_COLORS
                .get(usize::from(f.map_color))
                .map_or(0, |c| c & 0xFF_FFFF),
        );
    }
    t.blocks = blocks;
    t.sprites = sprites;
    t
}

#[cfg(feature = "mods")]
impl LentClientWorld {
    fn table(&mut self) -> Arc<BlockTable> {
        let generation = self.context.render.generation;
        if let Some((g, t)) = &self.table
            && *g == generation
        {
            return t.clone();
        }
        let table = Arc::new(build_table(&self.context, &self.res));
        self.table = Some((generation, table.clone()));
        table
    }
    fn flags(&mut self) -> Arc<Vec<u16>> {
        if let Some(f) = &self.flags {
            return f.clone();
        }
        let f = Arc::new(self.table().state_flags.clone());
        self.flags = Some(f.clone());
        f
    }
}

#[cfg(feature = "mods")]
impl ClientWorldView for LentClientWorld {
    fn dimension(&self) -> Option<ClientDimension> {
        self.dimension.clone()
    }
    fn block_table(&mut self) -> BlockTable {
        (*self.table()).clone()
    }
    fn table_version(&mut self) -> u64 {
        self.context.render.generation
    }
    fn texture(&mut self, name: &str) -> Option<ClientTexture> {
        let render = &self.context.render;
        let layers = &render.sprite_layers;
        let layer = *layers
            .get(name)
            .or_else(|| layers.get(&format!("minecraft:{name}")))?;
        let generation = render.generation;
        let pixels = match &mut self.pixels {
            Some((g, p, used)) if *g == generation => {
                *used = Instant::now();
                p.clone()
            }
            slot => {
                let p = render.pixels.get();
                *slot = Some((generation, p.clone(), Instant::now()));
                p
            }
        };
        let size = render.sprite_size;
        let bytes = (size * size * 4) as usize;
        let at = layer as usize * bytes;
        let rgba = pixels.get(at..at + bytes)?.to_vec();
        Some(ClientTexture {
            width: size,
            height: size,
            rgba,
        })
    }
    fn columns(&mut self, key: (i32, i32), layer: ColumnLayer, depth: u8) -> Option<ChunkColumns> {
        let flags = self.flags();
        let chunk = self.chunks.get(&key)?.clone();
        let revision = self.revisions.chunks.get(&key).map_or(0, |r| r.0);
        let depth = usize::from(depth);
        let (min_y, max_y) = (chunk.min_y(), chunk.max_y());
        // Fluids are drawn as nothing by their block (the fluid renderer
        // draws them), but they are what a column shows: only air and
        // invisible blocks without a fluid are passed.
        let skip = |id: u32| {
            flags.get(id as usize).is_none_or(|f| {
                f & flags::AIR != 0
                    || (f & flags::INVISIBLE != 0 && f & (flags::WATER | flags::LAVA) == 0)
            })
        };
        // As the game's minimap: from the highest section holding a block.
        let air = self.res.facts().air.0;
        let surface = (0..chunk.sections.len())
            .rev()
            .find(|&s| !chunk.sections[s].is_uniform(air))
            .map_or(min_y - 1, |s| min_y + s as i32 * 16 + 15);
        let start = match layer {
            ColumnLayer::Surface => surface,
            ColumnLayer::Below(y) => surface.min(y),
        };
        let mut out = ChunkColumns {
            revision,
            tops: vec![NO_TOP; 256],
            states: vec![air; 256 * depth],
            water: vec![0; 256],
            light: vec![0xF0; 256],
            tints: vec![0; 256 * 3],
        };
        for i in 0..256 {
            let (x, z) = ((i % 16) as i32, (i / 16) as i32);
            let mut y = start;
            while y >= min_y && skip(chunk.get(x, y, z)) {
                y -= 1;
            }
            let (wx, wz) = (key.0 * 16 + x, key.1 * 16 + z);
            let biome = chunk.biome(x, y.max(min_y), z);
            let b = self.biomes.get(biome);
            out.tints[i * 3] = self.biomes.grass(biome, f64::from(wx), f64::from(wz)) & 0xFF_FFFF;
            out.tints[i * 3 + 1] = b.foliage & 0xFF_FFFF;
            out.tints[i * 3 + 2] = b.water & 0xFF_FFFF;
            if y < min_y {
                continue;
            }
            out.tops[i] = y.clamp(i16::MIN as i32 + 1, i16::MAX as i32) as i16;
            for k in 0..depth {
                let below = y - k as i32;
                if below >= min_y {
                    out.states[i * depth + k] = chunk.get(x, below, z);
                }
            }
            let mut water = 0u32;
            while water < 255
                && y - (water as i32) >= min_y
                && flags
                    .get(chunk.get(x, y - water as i32, z) as usize)
                    .is_some_and(|f| f & flags::WATER != 0)
            {
                water += 1;
            }
            out.water[i] = water as u8;
            if y + 1 < max_y {
                out.light[i] = chunk.light(x, y + 1, z);
            }
        }
        Some(out)
    }
    fn changes(&mut self, since: u64) -> (u64, Vec<(i32, i32)>) {
        (self.revisions.counter, self.revisions.changes(since))
    }
    fn light(&self, x: i32, y: i32, z: i32) -> Option<u8> {
        let chunk = self.chunks.get(&(x >> 4, z >> 4))?;
        if y >= chunk.max_y() {
            return Some(0xF0);
        }
        (y >= chunk.min_y()).then(|| chunk.light(x & 15, y, z & 15))
    }
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<String> {
        let chunk = self.chunks.get(&(x >> 4, z >> 4))?;
        let id = chunk.biome(x & 15, y, z & 15);
        Some(self.biomes.get(id).name.clone())
    }
}

#[cfg(feature = "mods")]
impl Game<'_> {
    /// `game.world-id`: `sp:<save folder>` in single player, `mp:<address>`
    /// on a server; none on the title screen.
    pub(super) fn mod_world_id(&self) -> Option<String> {
        self.player.as_ref()?;
        // Single player runs the world on the integrated server's thread,
        // so the window's `world_root` is empty there: its folder is the
        // handle's.
        if let Some(i) = &self.integrated
            && !i.folder.is_empty()
        {
            return Some(format!("sp:{}", i.folder));
        }
        let save = || {
            let name = self.world_root.file_name()?.to_string_lossy();
            Some(format!("sp:{name}"))
        };
        if self.integrated.is_some() || self.remote.is_none() {
            return save();
        }
        Some(match self.server_address() {
            Some(address) => format!("mp:{address}"),
            None => "mp:lan".into(),
        })
    }
    /// Lends the window's world to capability `world-read` for the calls up
    /// to [`Game::return_client_world`], when a running mod reads it (`any`:
    /// before the mods start, for their `init`).
    pub(super) fn lend_client_world(&mut self, any: bool) {
        if !any && !self.mods.mods.wants_client_world() {
            return;
        }
        let host = &mut self.mods.client_world;
        host.revisions.sync(&mut self.world);
        if host
            .pixels
            .as_ref()
            .is_some_and(|(_, _, used)| used.elapsed() > PIXELS_KEPT)
        {
            host.pixels = None;
        }
        let in_world = self.player.is_some();
        let shape = self.world.shape;
        let dimension = in_world.then(|| ClientDimension {
            id: self.dim.id().to_owned(),
            min_y: shape.min_y,
            height: shape.height.max(0) as u32,
            ceiling: shape.has_ceiling,
        });
        let context = self.pack.context.clone();
        let biomes = self
            .world_biomes
            .clone()
            .unwrap_or_else(|| context.biomes.clone());
        let lent = LentClientWorld {
            chunks: std::mem::take(&mut self.world.chunks),
            revisions: std::mem::take(&mut host.revisions),
            dimension,
            context,
            res: self.res.clone(),
            biomes,
            table: host.table.take(),
            pixels: host.pixels.take(),
            flags: None,
        };
        self.mods.mods.lend_client_world(Box::new(lent));
    }
    /// Puts back what [`Game::lend_client_world`] lent.
    pub(super) fn return_client_world(&mut self) {
        let Some(view) = self.mods.mods.take_client_world() else {
            return;
        };
        let view: Box<dyn std::any::Any> = view;
        let Ok(lent) = view.downcast::<LentClientWorld>() else {
            return;
        };
        let lent = *lent;
        self.world.chunks = lent.chunks;
        let host = &mut self.mods.client_world;
        host.revisions = lent.revisions;
        host.table = lent.table;
        host.pixels = lent.pixels;
    }
    /// No mod reads the world any more: the world stops noting writes.
    pub(super) fn forget_client_world(&mut self) {
        if !self.mods.mods.wants_client_world() {
            self.world.mod_writes = None;
        }
    }
}

#[cfg(all(test, feature = "mods"))]
mod tests {
    use super::*;
    use horizon_client::{chunk::Chunk, world::World};

    #[test]
    fn counters_move_for_writes_loads_resends_and_unloads() {
        let mut world = World::new(BlockStateId(0), 0);
        world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        world.chunks.insert((1, 0), Arc::new(Chunk::empty(1, 0)));
        let mut r = Revisions::default();
        r.sync(&mut world);
        let mut all = r.changes(0);
        all.sort();
        assert_eq!(all, [(0, 0), (1, 0)]);
        let now = r.counter;
        r.sync(&mut world);
        assert!(r.changes(now).is_empty(), "nothing happened");
        // A block written in (1, 0): only it moves.
        world.mod_writes.as_mut().unwrap().insert((1, 0));
        r.sync(&mut world);
        assert_eq!(r.changes(now), [(1, 0)]);
        // (0, 0) sent again, (1, 0) unloaded, (2, 0) loaded.
        let now = r.counter;
        world.chunks.insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        world.chunks.remove(&(1, 0));
        world.chunks.insert((2, 0), Arc::new(Chunk::empty(2, 0)));
        r.sync(&mut world);
        let mut moved = r.changes(now);
        moved.sort();
        assert_eq!(moved, [(0, 0), (1, 0), (2, 0)]);
        assert!(!r.chunks.contains_key(&(1, 0)));
        // A write in a chunk not loaded moves nothing.
        let now = r.counter;
        world.mod_writes.as_mut().unwrap().insert((9, 9));
        r.sync(&mut world);
        assert!(r.changes(now).is_empty());
    }
}
