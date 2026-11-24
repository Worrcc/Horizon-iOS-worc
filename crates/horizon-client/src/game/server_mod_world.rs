//! Container access for server mods. Reads use a call's snapshot; writes use
//! the live block entity after that tick's queued block changes.
use super::Game;
use horizon_client::furnace::FurnaceCooking;
use horizon_client::{containers::BlockEntity, items::ItemStack, world::World};
use horizon_mods::{ContainerItemStack, MachineStack};
use std::{collections::HashMap, path::Path};

type Pos = (i32, i32, i32);
pub(super) type ContainerSnapshot = HashMap<(String, Pos), Vec<Option<ContainerItemStack>>>;
pub(super) type MachineStackSnapshot = HashMap<(String, Pos), Vec<Option<MachineStack>>>;
pub(super) type EnergySnapshot = HashMap<(String, Pos), (u64, u64)>;

/// API 39: energy state of every loaded content block that stores it.
pub(super) fn energy_of(entity: &BlockEntity) -> Option<(u64, u64)> {
    match entity {
        BlockEntity::Mod(m) => m.energy(),
        _ => None,
    }
}

/// API 37: a real slot with its full component patch as lossless network NBT.
pub(super) fn encode_machine_stack(stack: &ItemStack) -> Option<MachineStack> {
    let components = horizon_client::item_data::encode_components(stack)
        .and_then(|tag| {
            horizon_world::nbt::write_network(
                &tag,
                horizon_world::nbt::Limits {
                    max_bytes: 65536,
                    ..Default::default()
                },
            )
            .ok()
        })
        .filter(|bytes| !bytes.is_empty());
    Some(MachineStack {
        item: stack.item.to_string(),
        count: stack.count,
        components,
    })
}

pub(super) fn slots(entity: &BlockEntity) -> &[Option<ItemStack>] {
    match entity {
        BlockEntity::Lectern(l) => std::slice::from_ref(&l.book),
        _ => entity.slots(),
    }
}

pub(super) fn slots_mut(entity: &mut BlockEntity) -> &mut [Option<ItemStack>] {
    match entity {
        BlockEntity::Lectern(l) => std::slice::from_mut(&mut l.book),
        _ => entity.slots_mut(),
    }
}

/// The same two locations `commands::place` searches: world saves override
/// resources. The content overlay includes loaded mods' data templates.
pub(super) fn structure_exists(resources: &Path, world_root: &Path, id: &str) -> bool {
    let Some((namespace, path)) = id.split_once(':') else {
        return false;
    };
    if namespace.is_empty()
        || namespace == "."
        || namespace == ".."
        || path.is_empty()
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        || namespace.contains(['/', '\\'])
        || path.contains('\\')
    {
        return false;
    }
    world_root
        .join("generated")
        .join(namespace)
        .join("structures")
        .join(format!("{path}.nbt"))
        .is_file()
        || resources
            .join("worldgen/structure")
            .join(namespace)
            .join(format!("{path}.nbt"))
            .is_file()
}

impl Game<'_> {
    pub(super) fn mod_container_snapshot(&self) -> ContainerSnapshot {
        let mut out = HashMap::new();
        let mut add = |dimension: &str, world: &World| {
            for (&pos, entity) in &world.block_entities {
                let slots = slots(entity);
                if slots.is_empty() || !world.chunks.contains_key(&(pos.0 >> 4, pos.2 >> 4)) {
                    continue;
                }
                out.insert(
                    (dimension.to_owned(), pos),
                    slots
                        .iter()
                        .map(|slot| {
                            slot.as_ref().map(|stack| ContainerItemStack {
                                item: stack.item.to_string(),
                                count: stack.count,
                            })
                        })
                        .collect(),
                );
            }
        };
        add(self.dim.id(), &self.world);
        for (dimension, world) in self.levels.all_parked_worlds() {
            add(dimension.id(), world);
        }
        out
    }

    /// API 37: the same loaded containers, with full component patches. Only
    /// built while a running mod imports `world.machine-stacks` or
    /// `world.set-machine-stacks`.
    pub(super) fn mod_machine_stack_snapshot(&self) -> MachineStackSnapshot {
        let mut out = HashMap::new();
        let mut add = |dimension: &str, world: &World| {
            for (&pos, entity) in &world.block_entities {
                let slots = slots(entity);
                if slots.is_empty() || !world.chunks.contains_key(&(pos.0 >> 4, pos.2 >> 4)) {
                    continue;
                }
                out.insert(
                    (dimension.to_owned(), pos),
                    slots
                        .iter()
                        .map(|slot| slot.as_ref().and_then(encode_machine_stack))
                        .collect(),
                );
            }
        };
        add(self.dim.id(), &self.world);
        for (dimension, world) in self.levels.all_parked_worlds() {
            add(dimension.id(), world);
        }
        out
    }

    /// API 39: energy of loaded content blocks, built only while a mod reads it.
    pub(super) fn mod_energy_snapshot(&self) -> EnergySnapshot {
        let mut out = HashMap::new();
        let mut add = |dimension: &str, world: &World| {
            for (&pos, entity) in &world.block_entities {
                if !world.chunks.contains_key(&(pos.0 >> 4, pos.2 >> 4)) {
                    continue;
                }
                if let Some(energy) = energy_of(entity) {
                    out.insert((dimension.to_owned(), pos), energy);
                }
            }
        };
        add(self.dim.id(), &self.world);
        for (dimension, world) in self.levels.all_parked_worlds() {
            add(dimension.id(), world);
        }
        out
    }

    pub(super) fn apply_mod_container(
        &mut self,
        pos: Pos,
        new: Vec<Option<ContainerItemStack>>,
    ) -> Result<(), String> {
        let entity = self.world.block_entities.get(&pos).ok_or("no container")?;
        let count = slots(entity).len();
        if count == 0 {
            return Err("no container".into());
        }
        if new.len() != count {
            return Err(format!("container has {count} slots, got {}", new.len()));
        }
        // Recheck after the blocks were applied: both the container and the
        // registry may have changed since the mod's call.
        let new: Vec<Option<ItemStack>> = new
            .into_iter()
            .map(|stack| {
                stack
                    .map(|stack| {
                        let info = self
                            .res
                            .items()
                            .get(&stack.item)
                            .ok_or_else(|| format!("unknown item {}", stack.item))?;
                        if stack.count == 0 || stack.count > info.max_stack {
                            return Err(format!("invalid count for {}", stack.item));
                        }
                        Ok(ItemStack {
                            item: stack.item.into(),
                            count: stack.count,
                            damage: 0,
                            extra: None,
                        })
                    })
                    .transpose()
            })
            .collect::<Result<_, String>>()?;
        let Some(mut entity) = self.world.block_entities.remove(&pos) else {
            return Err("no container".into());
        };
        if let BlockEntity::Furnace(furnace) = &mut entity {
            furnace.set_input(self.res.recipes(), new[0].clone());
        }
        slots_mut(&mut entity).clone_from_slice(&new);
        // Explicit slot replacement consumes the saved generated loot roll,
        // including when the replacement happens to equal its old contents.
        self.take_loot_table(
            pos,
            &horizon_world::nbt::Tag::Compound(Default::default()),
            &mut entity,
        );
        self.world.block_entities.insert(pos, entity);
        self.container_changed(pos);
        if let Some(host) = self.lan.as_mut() {
            host.hub.sync_containers(&self.world);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_slots_include_persistent_inventories_and_lectern_books() {
        for (block, count) in [
            ("minecraft:chest", 27),
            ("minecraft:barrel", 27),
            ("minecraft:hopper", 5),
            ("minecraft:furnace", 3),
            ("minecraft:brewing_stand", 5),
            ("minecraft:lectern", 1),
            ("minecraft:chiseled_bookshelf", 6),
            ("minecraft:crafter", 9),
        ] {
            let mut entity = BlockEntity::for_block(block).expect(block);
            assert_eq!(slots(&entity).len(), count, "{block}");
            assert_eq!(slots_mut(&mut entity).len(), count, "{block}");
        }
        assert!(slots(&BlockEntity::for_block("minecraft:sign").unwrap()).is_empty());
    }

    #[test]
    fn structure_lookup_refuses_paths_outside_template_roots() {
        for id in [
            "minecraft:../secret",
            "../minecraft:template",
            "minecraft:/etc/passwd",
            "..:secret",
        ] {
            assert!(!structure_exists(Path::new("/"), Path::new("/"), id));
        }
    }
}
