//! API 26 storage transfers. Plan with metadata; apply using only real stacks.
use super::{Game, lan::LanHost, server_mods::run::uuid_string};
use horizon_client::{containers::BlockEntity, items::ItemStack, menu::Menu};
use horizon_mods::{StackInfo, StorageLocation, StorageMove, StorageStack, StorageView};
use sha2::{Digest, Sha256};

// Fixed-width big-endian lengths and values; no lossy text or platform Hash encoding.
fn hash_tag(hash: &mut Sha256, tag: &horizon_world::nbt::Tag) {
    use horizon_world::nbt::Tag;
    hash.update([tag.id()]);
    let length = |hash: &mut Sha256, n: usize| hash.update((n as u64).to_be_bytes());
    let string = |hash: &mut Sha256, s: &horizon_world::nbt::JavaString| {
        length(hash, s.0.len());
        for unit in &s.0 {
            hash.update(unit.to_be_bytes());
        }
    };
    match tag {
        Tag::End => {}
        Tag::Byte(v) => hash.update(v.to_be_bytes()),
        Tag::Short(v) => hash.update(v.to_be_bytes()),
        Tag::Int(v) => hash.update(v.to_be_bytes()),
        Tag::Long(v) => hash.update(v.to_be_bytes()),
        Tag::Float(v) => hash.update(v.to_be_bytes()),
        Tag::Double(v) => hash.update(v.to_be_bytes()),
        Tag::String(s) => string(hash, s),
        Tag::ByteArray(v) => {
            length(hash, v.len());
            for v in v {
                hash.update(v.to_be_bytes());
            }
        }
        Tag::IntArray(v) => {
            length(hash, v.len());
            for v in v {
                hash.update(v.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            length(hash, v.len());
            for v in v {
                hash.update(v.to_be_bytes());
            }
        }
        Tag::List(v) => {
            length(hash, v.len());
            for v in v {
                hash_tag(hash, v);
            }
        }
        Tag::Compound(v) => {
            length(hash, v.len());
            for (k, v) in v {
                string(hash, k);
                hash_tag(hash, v);
            }
        }
    }
}

fn display_key(stack: &ItemStack) -> String {
    // Lossless UTF-16 and float bits; compounds have sorted keys.
    // Including component removals distinguishes an explicit removal from a default.
    let tag = horizon_world::nbt::Tag::compound([
        (
            "item",
            horizon_world::nbt::Tag::String(stack.item.as_ref().into()),
        ),
        (
            "components",
            horizon_client::item_data::encode_components(stack)
                .unwrap_or_else(|| horizon_world::nbt::Tag::Compound(Default::default())),
        ),
    ]);
    let mut digest = Sha256::new();
    hash_tag(&mut digest, &tag);
    format!("{:x}", digest.finalize())
}

fn description(stack: &ItemStack, items: &horizon_client::items::Items) -> StorageStack {
    StorageStack {
        info: StackInfo {
            item: stack.item.to_string(),
            count: stack.count,
            display_key: display_key(stack),
            custom_name: stack.custom_name(),
        },
        max: Menu::item_max_stack(items, stack),
    }
}
fn describe(
    slots: &[Option<ItemStack>],
    items: &horizon_client::items::Items,
) -> Vec<Option<StorageStack>> {
    slots
        .iter()
        .map(|s| s.as_ref().map(|s| description(s, items)))
        .collect()
}
/// Ordinary bulk containers have no slot-specific item rules. Special machine
/// slots are read-only until the API can express their rules faithfully.
fn transferable(entity: &BlockEntity) -> bool {
    matches!(entity, BlockEntity::Chest(_))
}

fn transfer(
    source: &mut [Option<ItemStack>],
    slot: usize,
    target: &mut [Option<ItemStack>],
    to_slot: Option<u32>,
    count: u32,
    max: u32,
) -> Result<(), String> {
    let Some(stack) = source.get(slot).and_then(Option::as_ref).cloned() else {
        return Err("source empty".into());
    };
    if count == 0 || stack.count < count {
        return Err("source changed".into());
    }
    let mut after = target.to_vec();
    let mut left = count;
    for empty in [false, true] {
        for (i, dst) in after.iter_mut().enumerate() {
            if to_slot.is_some_and(|s| s as usize != i) {
                continue;
            }
            let room = match dst {
                Some(d) if !empty && d.same_item_same_components(&stack) => {
                    max.saturating_sub(d.count)
                }
                None if empty => max,
                _ => 0,
            };
            let moved = left.min(room);
            if moved == 0 {
                continue;
            }
            if let Some(d) = dst {
                d.count += moved;
            } else {
                *dst = Some(ItemStack {
                    count: moved,
                    ..stack.clone()
                });
            }
            left -= moved;
        }
    }
    if left != 0 {
        return Err("destination capacity changed".into());
    }
    target.clone_from_slice(&after);
    if stack.count == count {
        source[slot] = None;
    } else {
        source[slot].as_mut().unwrap().count -= count;
    }
    Ok(())
}
impl Game<'_> {
    pub(super) fn mod_storage_snapshot(&self, host: Option<&LanHost>) -> StorageView {
        let mut view = StorageView::default();
        let items = self.res.items();
        let mut add = |dim: &str, world: &horizon_client::world::World| {
            for (&pos, entity) in &world.block_entities {
                let slots = super::server_mod_world::slots(entity);
                if !slots.is_empty() && world.chunks.contains_key(&(pos.0 >> 4, pos.2 >> 4)) {
                    let loc = StorageLocation::Container {
                        dimension: dim.into(),
                        pos,
                    };
                    if !transferable(entity) {
                        view.read_only.insert(loc.clone());
                    }
                    view.slots.insert(loc, describe(slots, items));
                }
            }
        };
        add(self.dim.id(), &self.world);
        for (dim, world) in self.levels.all_parked_worlds() {
            add(dim.id(), world);
        }
        if let Some(host) = host.or(self.lan.as_ref()) {
            for g in host.hub.guests.iter().filter(|g| g.placed) {
                view.slots.insert(
                    StorageLocation::Player(uuid_string(g.uuid)),
                    describe(&g.inventory, items),
                );
            }
        }
        if self.options.execution_role.is_client() && self.remote.is_none() && self.player.is_some()
        {
            view.slots.insert(
                StorageLocation::Player(self.player_uuid_string()),
                describe(&self.inventory, items),
            );
        }
        view
    }
    fn storage_slots(
        &self,
        loc: &StorageLocation,
        host: Option<&LanHost>,
    ) -> Result<Vec<Option<ItemStack>>, String> {
        match loc {
            StorageLocation::Container { dimension, pos } => {
                if dimension != self.dim.id()
                    || !self.world.chunks.contains_key(&(pos.0 >> 4, pos.2 >> 4))
                {
                    return Err("container unloaded".into());
                }
                let entity = self
                    .world
                    .block_entities
                    .get(pos)
                    .ok_or("container missing")?;
                if !transferable(entity) {
                    return Err("storage moves currently require a bulk container (chest, barrel, hopper, dispenser or shulker)".into());
                }
                Ok(entity.slots().to_vec())
            }
            StorageLocation::Player(id) => {
                if self.options.execution_role.is_client()
                    && self.remote.is_none()
                    && self.player.is_some()
                    && *id == self.player_uuid_string()
                {
                    return Ok(self.inventory.clone());
                }
                host.or(self.lan.as_ref())
                    .and_then(|h| {
                        h.hub
                            .guests
                            .iter()
                            .find(|g| g.placed && uuid_string(g.uuid) == *id)
                    })
                    .map(|g| g.inventory.clone())
                    .ok_or_else(|| "player offline".into())
            }
        }
    }
    fn set_storage_slots(
        &mut self,
        loc: &StorageLocation,
        new: Vec<Option<ItemStack>>,
        host: Option<&mut LanHost>,
    ) {
        match loc {
            StorageLocation::Container { pos, .. } => self
                .world
                .block_entities
                .get_mut(pos)
                .unwrap()
                .slots_mut()
                .clone_from_slice(&new),
            StorageLocation::Player(id) => {
                if self.options.execution_role.is_client()
                    && self.remote.is_none()
                    && self.player.is_some()
                    && *id == self.player_uuid_string()
                {
                    self.inventory = new;
                } else if let Some(host) = host.or(self.lan.as_mut()) {
                    let i = host
                        .hub
                        .guests
                        .iter()
                        .position(|g| g.placed && uuid_string(g.uuid) == *id)
                        .unwrap();
                    host.hub.guests[i].inventory = new;
                    host.hub.send_inventory(i);
                }
            }
        }
    }
    pub(super) fn apply_mod_storage_move(
        &mut self,
        plan: StorageMove,
        mut host: Option<&mut LanHost>,
    ) -> Result<(), String> {
        let mut source = self.storage_slots(&plan.from, host.as_deref())?;
        let mut target = self.storage_slots(&plan.to, host.as_deref())?;
        if describe(&source, self.res.items()) != plan.before_from
            || describe(&target, self.res.items()) != plan.before_to
        {
            return Err("storage endpoint changed; reservation cancelled".into());
        }
        let stack = source
            .get(plan.from_slot as usize)
            .and_then(Option::as_ref)
            .ok_or("source slot missing")?;
        let max = Menu::item_max_stack(self.res.items(), stack);
        if plan.from == plan.to {
            // Mask the source from the destination's capacity, then restore its remainder.
            target[plan.from_slot as usize] = Some(ItemStack {
                count: max,
                ..stack.clone()
            });
            transfer(
                &mut source,
                plan.from_slot as usize,
                &mut target,
                plan.to_slot,
                plan.count,
                max,
            )?;
            target[plan.from_slot as usize] = source[plan.from_slot as usize].clone();
        } else {
            transfer(
                &mut source,
                plan.from_slot as usize,
                &mut target,
                plan.to_slot,
                plan.count,
                max,
            )?;
            self.set_storage_slots(&plan.from, source, host.as_deref_mut());
        }
        self.set_storage_slots(&plan.to, target, host.as_deref_mut());
        for loc in [&plan.from, &plan.to] {
            if let StorageLocation::Container { pos, .. } = loc {
                if let Some(chunk) = self.world.chunks.get_mut(&(pos.0 >> 4, pos.2 >> 4)) {
                    let chunk = std::sync::Arc::make_mut(chunk);
                    if let Some(saved) = chunk.saved.as_mut() {
                        horizon_client::save::consume_container_loot(
                            std::sync::Arc::make_mut(saved),
                            *pos,
                        );
                    }
                }
                self.container_changed(*pos);
            }
        }
        if let Some(host) = host.or(self.lan.as_mut()) {
            host.hub.sync_containers(&self.world);
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_keys_ignore_count_and_preserve_saved_component_identity() {
        let plain = ItemStack::new("minecraft:stone", 1);
        let mut renamed = plain.clone();
        renamed
            .data_mut()
            .set_loot_name("minecraft:custom_name", &serde_json::json!("Named"));
        assert_ne!(display_key(&plain), display_key(&renamed));
        let mut more = renamed.clone();
        more.count = 64;
        assert_eq!(display_key(&renamed), display_key(&more));
        let mut enchanted = renamed.clone();
        enchanted
            .data_mut()
            .enchantments
            .push(("minecraft:unbreaking".into(), 3));
        assert_ne!(display_key(&renamed), display_key(&enchanted));
        let mut removed = plain.clone();
        removed.data_mut().set_other(
            "!minecraft:max_stack_size",
            Some(horizon_world::nbt::Tag::Compound(Default::default())),
        );
        assert_ne!(display_key(&plain), display_key(&removed));
    }
    #[test]
    fn transfers_preserve_components_and_fail_atomically_without_space() {
        let mut named = ItemStack::new("minecraft:stone", 12);
        named
            .data_mut()
            .set_loot_name("minecraft:custom_name", &serde_json::json!("Named"));
        let mut src = vec![Some(named.clone())];
        let mut dst = vec![Some(ItemStack::new("minecraft:stone", 64)), None];
        transfer(&mut src, 0, &mut dst, None, 5, 64).unwrap();
        assert_eq!(src[0].as_ref().unwrap().count, 7);
        assert_eq!(dst[1].as_ref().unwrap().count, 5);
        assert!(dst[1].as_ref().unwrap().same_item_same_components(&named));
        let before_src = src.clone();
        let before_dst = dst.clone();
        assert!(transfer(&mut src, 0, &mut dst, Some(0), 7, 64).is_err());
        assert_eq!(src, before_src);
        assert_eq!(dst, before_dst);
    }
}
