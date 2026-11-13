//! API 28 storage transfers. Plan with metadata; apply using only real stacks.
use super::{Game, lan::LanHost, server_mods::run::uuid_string};
use horizon_client::{containers::BlockEntity, items::ItemStack, menu::Menu};
use horizon_mods::{
    StackInfo, StorageDataWrite, StorageLocation, StorageMove, StorageStack, StorageView,
};
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

fn stack_info(stack: &ItemStack) -> StackInfo {
    StackInfo {
        item: stack.item.to_string(),
        count: stack.count,
        display_key: display_key(stack),
        custom_name: stack.custom_name(),
    }
}
fn apply_stack_data(
    stack: Option<&mut ItemStack>,
    before: &StackInfo,
    mod_id: &str,
    value: Option<&str>,
) -> Result<(), String> {
    let stack = stack.ok_or("slot empty; data write cancelled")?;
    if stack_info(stack) != *before {
        return Err("storage slot changed; data write cancelled".into());
    }
    stack.set_mod_data(mod_id, value)
}
fn description(stack: &ItemStack, items: &horizon_client::items::Items) -> StorageStack {
    StorageStack {
        info: stack_info(stack),
        max: Menu::item_max_stack(items, stack),
        data: Default::default(),
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
fn describe_data(
    slots: &[Option<ItemStack>],
    items: &horizon_client::items::Items,
    mod_ids: &[String],
) -> Vec<Option<StorageStack>> {
    slots
        .iter()
        .map(|s| {
            s.as_ref().map(|s| {
                let mut stack = description(s, items);
                for id in mod_ids {
                    stack.data.insert(id.clone(), s.mod_data(id));
                }
                stack
            })
        })
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
        let data_mods = self.mods.mods.stack_data_mods();
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
                    view.slots
                        .insert(loc, describe_data(slots, items, &data_mods));
                }
            }
        };
        add(self.dim.id(), &self.world);
        for (dim, world) in self.levels.all_parked_worlds() {
            add(dim.id(), world);
        }
        if let Some(host) = host.or(self.lan.as_ref()) {
            for g in host.hub.guests.iter().filter(|g| g.placed) {
                view.held.insert(uuid_string(g.uuid), g.selected as u32);
                view.slots.insert(
                    StorageLocation::Player(uuid_string(g.uuid)),
                    describe_data(&g.inventory, items, &data_mods),
                );
            }
        }
        if self.options.execution_role.is_client() && self.remote.is_none() && self.player.is_some()
        {
            view.slots.insert(
                StorageLocation::Player(self.player_uuid_string()),
                describe_data(&self.inventory, items, &data_mods),
            );
        }
        // During native item callbacks as_guest has swapped the actor into Game.
        // The guest record then holds the server's parked inventory.
        if let Some(uuid) = self.lan_acting_uuid {
            let id = uuid_string(uuid);
            view.slots.insert(
                StorageLocation::Player(id.clone()),
                describe_data(&self.inventory, items, &data_mods),
            );
            view.held.insert(id, self.selected as u32);
        } else if self.options.execution_role.is_client()
            && self.remote.is_none()
            && self.player.is_some()
        {
            view.held
                .insert(self.player_uuid_string(), self.selected as u32);
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
    pub(super) fn apply_mod_give_data(
        &mut self,
        player: &str,
        item: &str,
        count: u32,
        mod_id: &str,
        data: Option<&str>,
        mut host: Option<&mut LanHost>,
    ) -> Result<(), String> {
        use horizon_client::mod_players::{Inventories, Inventory};
        use horizon_mods::PlayerInventories;
        let mut result = Err("unknown or offline player".into());
        self.with_mod_player(host.as_deref_mut(), player, |g| {
            let Some(info) = g.res.items().get(item) else {
                result = Err(format!("unknown item {item}"));
                return;
            };
            let mut inventories = Inventories {
                items: std::sync::Arc::new([(item.to_owned(), info.max_stack)].into()),
                players: [(
                    player.to_owned(),
                    Inventory {
                        main: g.inventory.clone(),
                        offhand: g.offhand.clone(),
                        selected: g.selected,
                        creative: g.creative,
                        ..Default::default()
                    },
                )]
                .into(),
            };
            result = inventories.give_item_with_data(player, item, count, mod_id, data);
            if result.is_ok()
                && let Some(inv) = inventories.players.remove(player)
            {
                g.inventory = inv.main;
                g.offhand = inv.offhand;
                let at = g
                    .player
                    .as_ref()
                    .map(|p| [p.position.x, p.position.y, p.position.z]);
                if let Some(at) = at {
                    for stack in inv.drops {
                        g.spawn_item(stack, at, [0.0, 0.1, 0.0], 10);
                    }
                }
            }
        });
        if result.is_ok()
            && let Some(host) = host.or(self.lan.as_mut())
            && let Some(i) = host
                .hub
                .guests
                .iter()
                .position(|g| g.placed && uuid_string(g.uuid) == player)
        {
            host.hub.send_inventory(i);
        }
        result
    }
    pub(super) fn apply_mod_stack_data(
        &mut self,
        plan: StorageDataWrite,
        mut host: Option<&mut LanHost>,
    ) -> Result<(), String> {
        let mut slots = self.storage_slots(&plan.location, host.as_deref())?;
        if plan.held {
            let StorageLocation::Player(id) = &plan.location else {
                return Err("held write requires player".into());
            };
            let selected = if self.options.execution_role.is_client()
                && self.remote.is_none()
                && *id == self.player_uuid_string()
            {
                Some(self.selected)
            } else {
                host.as_deref()
                    .or(self.lan.as_ref())
                    .and_then(|h| {
                        h.hub
                            .guests
                            .iter()
                            .find(|g| g.placed && uuid_string(g.uuid) == *id)
                    })
                    .map(|g| g.selected)
            };
            if selected != Some(plan.slot as usize) {
                return Err("held slot changed; data write cancelled".into());
            }
        }
        apply_stack_data(
            slots.get_mut(plan.slot as usize).and_then(Option::as_mut),
            &plan.before.info,
            &plan.mod_id,
            plan.value.as_deref(),
        )?;
        self.set_storage_slots(&plan.location, slots, host.as_deref_mut());
        self.storage_changed(&plan.location);
        if let Some(host) = host.or(self.lan.as_mut()) {
            host.hub.sync_containers(&self.world);
        }
        Ok(())
    }
    fn storage_changed(&mut self, loc: &StorageLocation) {
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
            self.storage_changed(loc);
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
    fn data_writes_cancel_changed_or_empty_slots_and_preserve_other_keys() {
        let mut original = ItemStack::new("minecraft:stick", 12);
        original.set_mod_data("other", Some("keep")).unwrap();
        let before = stack_info(&original);
        let mut changed = original.clone();
        changed.count -= 1;
        let mut component_changed = original.clone();
        component_changed
            .set_mod_data("other", Some("different"))
            .unwrap();
        for mut stack in [
            changed,
            component_changed,
            ItemStack::new("minecraft:stone", 12),
        ] {
            let saved = stack.clone();
            assert!(apply_stack_data(Some(&mut stack), &before, "demo", Some("x")).is_err());
            assert_eq!(stack, saved);
        }
        assert!(apply_stack_data(None, &before, "demo", None).is_err());
        let mut stack = original.clone();
        apply_stack_data(Some(&mut stack), &before, "demo", Some("x")).unwrap();
        assert_eq!(stack.mod_data("other").unwrap().as_deref(), Some("keep"));
        assert_eq!(stack.mod_data("demo").unwrap().as_deref(), Some("x"));
        let before = stack_info(&stack);
        apply_stack_data(Some(&mut stack), &before, "demo", None).unwrap();
        assert_eq!(stack, original);
    }
    #[test]
    fn data_stacks_move_chest_player_chest_and_merge_only_equal_data() {
        let mut tagged = ItemStack::new("minecraft:stick", 12);
        tagged.set_mod_data("demo", Some("one")).unwrap();
        tagged.set_mod_data("other", Some("keep")).unwrap();
        let mut chest = vec![Some(tagged.clone()), None];
        let mut player = vec![None; 36];
        transfer(&mut chest, 0, &mut player, None, 5, 64).unwrap();
        assert_eq!(
            player[0]
                .as_ref()
                .unwrap()
                .mod_data("demo")
                .unwrap()
                .as_deref(),
            Some("one")
        );
        transfer(&mut player, 0, &mut chest, None, 5, 64).unwrap();
        assert!(player.iter().all(Option::is_none));
        assert_eq!(chest, [Some(tagged.clone()), None]);
        let mut different = tagged.clone();
        different.count = 4;
        different.set_mod_data("demo", Some("two")).unwrap();
        assert_ne!(display_key(&tagged), display_key(&different));
        let mut source = vec![Some(different.clone())];
        assert!(transfer(&mut source, 0, &mut chest, Some(0), 4, 64).is_err());
        assert_eq!(source[0], Some(different));
        transfer(&mut source, 0, &mut chest, None, 4, 64).unwrap();
        assert_eq!(chest[0].as_ref().unwrap().count, 12);
        assert_eq!(chest[1].as_ref().unwrap().count, 4);
        for stack in chest.iter_mut().flatten() {
            stack.set_mod_data("demo", None).unwrap();
        }
        let removed = chest[1].clone();
        let mut source = vec![removed];
        chest[1] = None;
        transfer(&mut source, 0, &mut chest, None, 4, 64).unwrap();
        assert_eq!(chest[0].as_ref().unwrap().count, 16);
        assert!(chest[1].is_none());
        assert_eq!(
            chest[0]
                .as_ref()
                .unwrap()
                .mod_data("other")
                .unwrap()
                .as_deref(),
            Some("keep")
        );
    }
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
