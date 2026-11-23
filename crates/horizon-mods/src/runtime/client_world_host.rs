//! Capability `world-read`'s `client-world` interface (API level 17): the
//! chunks, biomes and light the player's game has loaded, and the loaded
//! packs' block table and textures, read through a [`ClientWorldView`] the
//! game lends for its client hook calls (as it lends a [`super::WorldView`]
//! to server mods).
use super::{Mods, State, world_read_bindings::horizon::mods::client_world};
pub use client_world::{
    BlockStates as BlockTable, ChunkColumns, DimensionInfo as ClientDimension, Face as BlockFace,
    HarvestInfo as Harvest, Layer as ColumnLayer, TexturePixels as ClientTexture,
    TintSource as FaceTint,
};
use std::sync::{Arc, Mutex};

/// Chunks one `columns` call may read, and the blocks it reads of a column.
pub const COLUMN_CHUNKS: usize = 16;
pub const COLUMN_DEPTH: u8 = 16;
/// `changes`' largest radius in chunks, and the most chunks it lists.
pub const CHANGES_RADIUS: u32 = 64;
pub const CHANGES_MAX: usize = 4096;
/// `columns`' `tops` for a column with no block.
pub const NO_TOP: i16 = i16::MIN;

/// `block-table.state-flags` bits.
pub mod state_flags {
    pub const AIR: u16 = 1;
    pub const INVISIBLE: u16 = 2;
    pub const WATER: u16 = 4;
    pub const LAVA: u16 = 8;
    pub const WATERLOGGED: u16 = 16;
    pub const SEE_THROUGH: u16 = 32;
    pub const NO_COLLISION: u16 = 64;
    pub const FULL_TOP: u16 = 128;
    /// The light a state gives, 0 to 15, from this bit.
    pub const EMISSION_SHIFT: u16 = 8;
}
/// `face.sprite` for a side that draws nothing.
pub const NO_SPRITE: u32 = u32::MAX;

/// The player's world as capability `world-read` reads it during a call:
/// the game lends one with [`Mods::lend_client_world`] and takes it back
/// after. The checks on counts and ranges are the runtime's; a view answers
/// for one chunk or block at a time.
pub trait ClientWorldView: std::any::Any + Send {
    /// The player's dimension; none on the title screen.
    fn dimension(&self) -> Option<ClientDimension>;
    /// API 20: a loaded state in the current dimension.
    fn block_at(&self, _x: i32, _y: i32, _z: i32) -> Option<String> {
        None
    }
    /// The loaded packs' block table (`client-world.block-table`).
    fn block_table(&mut self) -> BlockTable;
    /// The block table's `version` (`client-world.table-version`), without
    /// building it.
    fn table_version(&mut self) -> u64 {
        self.block_table().version
    }
    /// A texture of the block table's `sprites`, by name.
    fn texture(&mut self, name: &str) -> Option<ClientTexture>;
    /// One chunk's columns, `depth` (1 to [`COLUMN_DEPTH`]) blocks of each in
    /// `layer`; none when it isn't loaded.
    fn columns(&mut self, chunk: (i32, i32), layer: ColumnLayer, depth: u8)
    -> Option<ChunkColumns>;
    /// The change counter now, and every chunk whose counter moved past
    /// `since` (loaded, unloaded or changed), anywhere.
    fn changes(&mut self, since: u64) -> (u64, Vec<(i32, i32)>);
    /// Sky light << 4 | block light at a block; none where nothing is loaded.
    fn light(&self, x: i32, y: i32, z: i32) -> Option<u8>;
    /// The biome at a block; none where nothing is loaded.
    fn biome(&self, x: i32, y: i32, z: i32) -> Option<String>;
    /// API 24: what mines a block id or state, for the held item; none for an
    /// unknown block.
    fn harvest(&self, _block: &str) -> Option<Harvest> {
        None
    }
}

/// The view lent for the current calls, shared by every mod's store.
pub(super) type ClientWorldSlot = Arc<Mutex<Option<Box<dyn ClientWorldView>>>>;

impl State {
    fn with_view<R>(&self, f: impl FnOnce(&mut dyn ClientWorldView) -> R) -> Option<R> {
        let mut slot = self
            .client_world
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.as_mut().map(|v| f(v.as_mut()))
    }
}

/// An empty block table: what a mod reads with no view lent.
fn no_table() -> BlockTable {
    BlockTable {
        version: 0,
        blocks: Vec::new(),
        sprites: Vec::new(),
        state_block: Vec::new(),
        state_flags: Vec::new(),
        state_top: Vec::new(),
        state_side: Vec::new(),
        constant_tint: Vec::new(),
        map_color: Vec::new(),
    }
}

impl client_world::Host for State {
    fn set_see_through(&mut self, states: Vec<u32>) -> Result<(), String> {
        if states.len() > 65536 {
            return Err("at most 65536 see-through state ids".into());
        }
        let started = std::time::Instant::now();
        self.see_through = states.into_iter().collect();
        self.canvas.charge(started);
        Ok(())
    }
    fn prepared_columns(
        &mut self,
        chunks: Vec<(i32, i32)>,
        layer: ColumnLayer,
        depth: u8,
    ) -> Result<Vec<Option<client_world::PreparedChunk>>, String> {
        let read = self.columns(chunks, layer, depth)?;
        let started = std::time::Instant::now();
        let prepared = read
            .into_iter()
            .map(|chunk| {
                chunk.map(|columns| prepare(columns, usize::from(depth), &self.see_through))
            })
            .collect();
        self.canvas.charge(started);
        Ok(prepared)
    }
    fn block_at(&mut self, x: i32, y: i32, z: i32) -> Option<String> {
        self.with_view(|v| v.dimension().and_then(|_| v.block_at(x, y, z)))
            .flatten()
    }
    fn dimension(&mut self) -> Option<ClientDimension> {
        self.with_view(|v| v.dimension()).flatten()
    }
    fn block_table(&mut self) -> BlockTable {
        self.with_view(|v| v.block_table()).unwrap_or_else(no_table)
    }
    fn table_version(&mut self) -> u64 {
        self.with_view(|v| v.table_version()).unwrap_or(0)
    }
    fn texture(&mut self, name: String) -> Option<ClientTexture> {
        if name.len() > 256 {
            return None;
        }
        self.with_view(|v| v.texture(&name)).flatten()
    }
    fn columns(
        &mut self,
        chunks: Vec<(i32, i32)>,
        layer: ColumnLayer,
        depth: u8,
    ) -> Result<Vec<Option<ChunkColumns>>, String> {
        if chunks.len() > COLUMN_CHUNKS {
            return Err(format!(
                "at most {COLUMN_CHUNKS} chunks a call, not {}",
                chunks.len()
            ));
        }
        if !(1..=COLUMN_DEPTH).contains(&depth) {
            return Err(format!("depth is 1 to {COLUMN_DEPTH}, not {depth}"));
        }
        let started = std::time::Instant::now();
        let read = self
            .with_view(|v| chunks.iter().map(|&c| v.columns(c, layer, depth)).collect())
            .unwrap_or_else(|| vec![None; chunks.len()]);
        // Reading the world burns no fuel, but takes the frame's time.
        self.canvas.charge(started);
        Ok(read)
    }
    fn changes(&mut self, since: u64, radius: u32) -> (u64, Vec<(i32, i32)>) {
        let Some(p) = self.snapshot.player.as_ref() else {
            return (since, Vec::new());
        };
        let center = ((p.x.floor() as i32) >> 4, (p.z.floor() as i32) >> 4);
        let radius = radius.min(CHANGES_RADIUS) as i32;
        let Some((now, mut chunks)) = self.with_view(|v| v.changes(since)) else {
            return (since, Vec::new());
        };
        let distance = |&(x, z): &(i32, i32)| (x - center.0).abs().max((z - center.1).abs());
        chunks.retain(|c| distance(c) <= radius);
        chunks.sort_by_key(|c| {
            let (dx, dz) = (c.0 - center.0, c.1 - center.1);
            (dx * dx + dz * dz, c.1, c.0)
        });
        chunks.truncate(CHANGES_MAX);
        (now, chunks)
    }
    fn light(&mut self, x: i32, y: i32, z: i32) -> Option<u8> {
        self.with_view(|v| v.light(x, y, z)).flatten()
    }
    fn biome(&mut self, x: i32, y: i32, z: i32) -> Option<String> {
        self.with_view(|v| v.biome(x, y, z)).flatten()
    }
    fn harvest(&mut self, block: String) -> Option<Harvest> {
        if block.len() > 256 {
            return None;
        }
        let started = std::time::Instant::now();
        let out = self
            .with_view(|v| v.dimension().and_then(|_| v.harvest(&block)))
            .flatten();
        self.canvas.charge(started);
        out
    }
}

fn prepare(
    mut columns: ChunkColumns,
    depth: usize,
    see_through: &std::collections::HashSet<u32>,
) -> client_world::PreparedChunk {
    let mut skips = vec![0; 256];
    let mut skipped_states = Vec::new();
    for (col, skip) in skips.iter_mut().enumerate() {
        let Some(top) = columns.tops.get_mut(col).filter(|t| **t != NO_TOP) else {
            continue;
        };
        let Some(row) = columns.states.get_mut(col * depth..(col + 1) * depth) else {
            continue;
        };
        let Some(k) = row
            .iter()
            .position(|s| !see_through.contains(s))
            .filter(|k| *k > 0)
        else {
            continue;
        };
        *skip = k as u8;
        skipped_states.extend_from_slice(&row[..k]);
        row.copy_within(k.., 0);
        let last = row[depth - k - 1];
        row[depth - k..].fill(last);
        *top -= k as i16;
    }
    client_world::PreparedChunk {
        columns,
        skips,
        skipped_states,
    }
}

impl Mods {
    /// Some running mod reads capability `world-read`: only then is a view
    /// worth lending.
    pub fn wants_client_world(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.reads_client_world))
    }
    /// Lends the player's world to capability `world-read` for the calls
    /// until [`Mods::take_client_world`]; with none lent, a mod reads no
    /// dimension, an empty block table and no chunks.
    pub fn lend_client_world(&mut self, view: Box<dyn ClientWorldView>) {
        *self
            .client_world
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(view);
    }
    /// The view [`Mods::lend_client_world`] lent, back to the game.
    pub fn take_client_world(&mut self) -> Option<Box<dyn ClientWorldView>> {
        self.client_world
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_columns_keep_missing_chunks_and_validate_without_a_view() {
        let mut state = super::super::entity_tests::state();
        use client_world::Host;
        let read = state
            .prepared_columns(vec![(0, 0), (2, -3)], ColumnLayer::Surface, 16)
            .unwrap();
        assert_eq!(read.len(), 2);
        assert!(read.iter().all(Option::is_none));
        assert!(
            state
                .prepared_columns(vec![(0, 0); 17], ColumnLayer::Surface, 1)
                .is_err()
        );
        assert!(
            state
                .prepared_columns(vec![], ColumnLayer::Surface, 0)
                .is_err()
        );
        assert!(
            state
                .prepared_columns(vec![], ColumnLayer::Surface, 17)
                .is_err()
        );
        assert!(
            state
                .prepared_columns(vec![], ColumnLayer::Surface, 1)
                .unwrap()
                .is_empty()
        );
    }
}
