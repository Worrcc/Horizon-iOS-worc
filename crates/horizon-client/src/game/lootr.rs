//! Lootr's core rule (Lootr by Noobanidus, MIT: <https://github.com/noobanidus/Lootr>), ported
//! to Horizon: a chest, trapped chest or barrel that still holds the loot its table rolled gives
//! every player their own loot. The container keeps the shared roll; each player opening it
//! gets a copy rolled from the same table with a seed of their own, kept in the world folder
//! (`horizon-lootr.nbt`), swapped in while they have it open and out when they close it. A
//! Lootr container breaks only while sneaking (Lootr's break protection).
//!
//! Not ported: Lootr's own blocks and models, refreshing and decaying loot, chest minecarts,
//! double chests (they open as vanilla's), several players in one container at once (the
//! second is told it is in use), its config. On by default; `"enabled": 0b` in the file turns
//! it off for that world.
use super::Game;
use horizon_client::containers::BlockEntity;
use horizon_client::items::ItemStack;
use horizon_world::nbt::{Compound, Tag};
use std::collections::HashMap;

/// A container: its dimension and block position.
type Key = (String, (i32, i32, i32));
type Slots = Vec<Option<ItemStack>>;

const FILE: &str = "horizon-lootr.nbt";

#[derive(Default)]
pub(super) struct Lootr {
    loaded: bool,
    enabled: bool,
    dirty: bool,
    /// Each player's own loot, by container and player UUID (`xxxxxxxx...` hex).
    copies: HashMap<Key, HashMap<String, Slots>>,
    /// A container a player has open: who, and the shared roll put aside meanwhile.
    open: HashMap<Key, (String, Slots)>,
    /// Containers settled for a save, to open again after it.
    settled: Vec<(Key, String)>,
}

fn uuid_hex(u: [i32; 4]) -> String {
    u.iter().map(|p| format!("{:08x}", *p as u32)).collect()
}

fn lootr_block(name: &str) -> bool {
    matches!(
        name,
        "minecraft:chest" | "minecraft:trapped_chest" | "minecraft:barrel"
    )
}

/// A seed of the player's own for a container (Lootr rolls each player's loot separately).
fn player_seed(seed: Option<i64>, pos: (i32, i32, i32), player: &str) -> i64 {
    let mut h = seed.unwrap_or(0) as u64 ^ 0x6c6f_6f74_7200_0000;
    for b in player.bytes().chain(format!("{pos:?}").bytes()) {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h as i64 | 1
}

impl Game<'_> {
    fn lootr_load(&mut self) {
        if self.lootr.loaded {
            return;
        }
        self.lootr.loaded = true;
        self.lootr.enabled = true;
        let path = self.save.root.join(FILE);
        if !path.exists() {
            return;
        }
        let Some(root) = horizon_client::save::read_dat(&path) else {
            log::warn!(
                "Horizon: {} is unreadable; Lootr starts over",
                path.display()
            );
            return;
        };
        if let Some(Tag::Byte(on)) = root.get("enabled") {
            self.lootr.enabled = *on != 0;
        }
        let Some(Tag::List(containers)) = root.get("containers") else {
            return;
        };
        for c in containers.iter() {
            let (Some(Tag::String(dim)), Some(Tag::IntArray(pos))) =
                (c.get("dimension"), c.get("pos"))
            else {
                continue;
            };
            let [x, y, z] = pos[..] else { continue };
            let Some(Tag::Compound(players)) = c.get("players") else {
                continue;
            };
            let mut map = HashMap::new();
            for (uuid, items) in players.iter() {
                let Some(Tag::Int(size)) = c.get("size") else {
                    continue;
                };
                let slots = horizon_client::save::decode_items(Some(items), *size as usize);
                map.insert(uuid.to_string_lossy(), slots);
            }
            self.lootr
                .copies
                .insert((dim.to_string_lossy(), (x, y, z)), map);
        }
    }

    fn lootr_write(&mut self) {
        if !self.lootr.loaded || !self.lootr.dirty {
            return;
        }
        let containers: Vec<Tag> = (self.lootr.copies.iter())
            .map(|((dim, (x, y, z)), players)| {
                let size = players.values().next().map_or(27, Vec::len);
                let players: Vec<(horizon_world::nbt::JavaString, Tag)> = players
                    .iter()
                    .map(|(uuid, slots)| {
                        (
                            uuid.as_str().into(),
                            horizon_client::save::encode_items(slots),
                        )
                    })
                    .collect();
                Tag::compound([
                    ("dimension", Tag::String(dim.as_str().into())),
                    ("pos", Tag::IntArray(vec![*x, *y, *z])),
                    ("size", Tag::Int(size as i32)),
                    ("players", Tag::Compound(Compound::from_iter(players))),
                ])
            })
            .collect();
        let root = Tag::compound([
            ("enabled", Tag::Byte(i8::from(self.lootr.enabled))),
            ("containers", Tag::List(containers)),
        ]);
        let path = self.save.root.join(FILE);
        match horizon_client::save::write_dat(&path, root) {
            Ok(()) => self.lootr.dirty = false,
            Err(e) => log::warn!("Horizon: writing {}: {e}", path.display()),
        }
    }

    /// The container at `key` holds exactly what its loot table rolled (no one emptied it
    /// the vanilla way): its table, seed and the roll.
    fn lootr_roll(&self, key: (i32, i32, i32)) -> Option<horizon_client::save::RolledLoot> {
        let chunk = self.world.chunks.get(&(key.0 >> 4, key.2 >> 4))?;
        let rolled = chunk
            .saved
            .as_ref()?
            .rolled_loot
            .iter()
            .find(|r| r.pos == key)?;
        let BlockEntity::Chest(slots) = self.world.block_entities.get(&key)? else {
            return None;
        };
        (horizon_client::save::encode_items(slots) == rolled.items).then(|| rolled.clone())
    }

    /// A player opens the container at `key`: false when it must not open (another player
    /// has their own loot open in it). Called before the menu opens.
    pub(super) fn lootr_open(&mut self, key: (i32, i32, i32), name: &str) -> bool {
        if !lootr_block(name) || self.remote.is_some() {
            return true;
        }
        self.lootr_load();
        if !self.lootr.enabled || self.chest_partner(key).is_some() {
            return true;
        }
        let ck: Key = (self.dim.id().to_owned(), key);
        let player = uuid_hex(self.acting_player_uuid());
        if let Some((who, _)) = self.lootr.open.get(&ck) {
            if *who == player {
                return true;
            }
            self.message = Some((
                "Someone else is looking at their loot in here".into(),
                std::time::Instant::now(),
            ));
            return false;
        }
        let Some(rolled) = self.lootr_roll(key) else {
            return true;
        };
        let Some(BlockEntity::Chest(shared)) = self.world.block_entities.get(&key) else {
            return true;
        };
        let size = shared.len();
        let own = match self.lootr.copies.get(&ck).and_then(|m| m.get(&player)) {
            Some(own) => own.clone(),
            None => {
                let mut own: Slots = vec![None; size];
                let loot = horizon_client::structure_blocks::loot_data(
                    &self.options.resources,
                    &self.pack_files(),
                );
                let at = horizon_core::geometry::Vec3::new(
                    f64::from(key.0),
                    f64::from(key.1),
                    f64::from(key.2),
                );
                let biome = self.biome_name_at(at).unwrap_or_default();
                horizon_client::structure_blocks::roll_container(
                    &mut own,
                    loot.as_deref(),
                    rolled.table.clone(),
                    Some(player_seed(rolled.seed, key, &player)),
                    key,
                    &biome,
                );
                self.lootr.dirty = true;
                own
            }
        };
        if let Some(BlockEntity::Chest(slots)) = self.world.block_entities.get_mut(&key) {
            let shared = std::mem::replace(slots, own);
            self.lootr.open.insert(ck, (player, shared));
        }
        true
    }

    /// The menu of the container at `key` closes: the player's loot is kept and the shared
    /// roll goes back.
    pub(super) fn lootr_close(&mut self, key: (i32, i32, i32)) {
        let ck: Key = (self.dim.id().to_owned(), key);
        let Some((player, shared)) = self.lootr.open.remove(&ck) else {
            return;
        };
        if let Some(BlockEntity::Chest(slots)) = self.world.block_entities.get_mut(&key) {
            let own = std::mem::replace(slots, shared);
            self.lootr.copies.entry(ck).or_default().insert(player, own);
            self.lootr.dirty = true;
        }
    }

    /// Before a save: every open Lootr container shows its shared roll, so the save keeps the
    /// loot table; [`Self::lootr_reopen`] puts the players' loot back after.
    pub(super) fn lootr_settle(&mut self) {
        let here = self.dim.id().to_owned();
        let keys: Vec<Key> = (self.lootr.open.keys())
            .filter(|(dim, _)| *dim == here)
            .cloned()
            .collect();
        for ck in keys {
            let player = self.lootr.open[&ck].0.clone();
            self.lootr_close(ck.1);
            self.lootr.settled.push((ck, player));
        }
        self.lootr_write();
    }

    pub(super) fn lootr_reopen(&mut self) {
        for ((dim, key), player) in std::mem::take(&mut self.lootr.settled) {
            let Some(BlockEntity::Chest(slots)) = self.world.block_entities.get_mut(&key) else {
                continue;
            };
            let ck = (dim, key);
            let Some(own) = self
                .lootr
                .copies
                .get(&ck)
                .and_then(|m| m.get(&player))
                .cloned()
            else {
                continue;
            };
            let shared = std::mem::replace(slots, own);
            self.lootr.open.insert(ck, (player, shared));
        }
    }

    /// Lootr's break protection: a Lootr container breaks only while its breaker sneaks.
    pub(super) fn lootr_protects(
        &mut self,
        key: (i32, i32, i32),
        name: &str,
        sneaking: bool,
    ) -> bool {
        if sneaking || !lootr_block(name) || self.remote.is_some() {
            return false;
        }
        self.lootr_load();
        self.lootr.enabled && self.lootr_roll(key).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_get_their_own_seeds() {
        let a = player_seed(Some(5), (1, 2, 3), "aaaa");
        let b = player_seed(Some(5), (1, 2, 3), "bbbb");
        assert_ne!(a, b);
        assert_eq!(a, player_seed(Some(5), (1, 2, 3), "aaaa"));
        assert_ne!(a, player_seed(Some(5), (1, 2, 4), "aaaa"));
        assert_eq!(uuid_hex([1, -1, 0, 2]), "00000001ffffffff0000000000000002");
        assert!(lootr_block("minecraft:barrel") && !lootr_block("minecraft:hopper"));
    }
}
