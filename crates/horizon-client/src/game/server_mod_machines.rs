//! API 30 bridges. Menus use the existing display-only hub path; machines
//! use the existing native engine helper, on the owning game thread.
use super::{
    Game,
    lan::LanHost,
    server_mods::run::{state_string, uuid_string},
};
use horizon_client::{
    containers::{self, BlockEntity},
    items::ItemStack,
    text::Text,
};
use horizon_mods::{BlockDataImage, BlockDataWrite, DestroyRequest, MenuLease, MenuOperation};
use std::collections::HashMap;
type BlockDataSnapshot = HashMap<(String, (i32, i32, i32), String), Result<BlockDataImage, String>>;

impl Game<'_> {
    pub(super) fn mod_block_data_snapshot(&self, ids: &[String]) -> BlockDataSnapshot {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let mut add = |dimension: &str, world: &horizon_client::world::World| {
            for (&pos, entity) in &world.block_entities {
                let BlockEntity::Mod(m) = entity else {
                    continue;
                };
                if containers::content_container(&m.id).is_none() || !world.loaded(pos.0, pos.2) {
                    continue;
                }
                for id in ids {
                    out.insert(
                        (dimension.into(), pos, id.clone()),
                        m.mod_data(id).map(|value| BlockDataImage {
                            state: state_string(
                                self.pack.assets.registry,
                                world.block(pos.0, pos.1, pos.2),
                            ),
                            token: format!("{:?}", m.incarnation()),
                            value,
                        }),
                    );
                }
            }
        };
        add(self.dim.id(), &self.world);
        for (dim, world) in self.levels.all_parked_worlds() {
            add(dim.id(), world);
        }
        out
    }
    pub(super) fn apply_mod_block_data(&mut self, plan: BlockDataWrite) -> Result<(), String> {
        let p = plan.pos;
        let key = (p.x, p.y, p.z);
        if !self.world.loaded(p.x, p.z)
            || state_string(self.pack.assets.registry, self.world.block(p.x, p.y, p.z))
                != plan.before.state
        {
            return Err("block data conflict: block changed or unloaded".into());
        }
        let Some(BlockEntity::Mod(m)) = self.world.block_entities.get_mut(&key) else {
            return Err("block data conflict: entity removed".into());
        };
        if containers::content_container(&m.id).is_none()
            || format!("{:?}", m.incarnation()) != plan.before.token
            || m.mod_data(&plan.mod_id)? != plan.before.value
        {
            return Err("block data conflict: entity changed".into());
        }
        m.set_mod_data(&plan.mod_id, plan.value.as_deref())?;
        self.container_changed(key);
        Ok(())
    }
    pub(super) fn wasm_destroy(
        &mut self,
        request: DestroyRequest,
        mut host: Option<&mut LanHost>,
    ) -> Result<Vec<horizon_mods::MachineStack>, String> {
        let old = self.dim;
        // A machine has no player. Park the borrowed actor while switching
        // levels, then restore it before the surrounding player hook resumes.
        let player = self.player.take();
        let acting = self.lan_acting.take();
        let acting_uuid = self.lan_acting_uuid.take();
        let result = self.wasm_destroy_without_actor(request, host.as_deref_mut(), old);
        self.player = player;
        self.lan_acting = acting;
        self.lan_acting_uuid = acting_uuid;
        result
    }
    fn wasm_destroy_without_actor(
        &mut self,
        request: DestroyRequest,
        mut host: Option<&mut LanHost>,
        old: super::Dim,
    ) -> Result<Vec<horizon_mods::MachineStack>, String> {
        self.enter_level(host.as_deref_mut(), super::Dim::from_id(&request.dimension))
            .map_err(|e| format!("{e:?}"))?;
        let p = request.pos;
        let result = if !self.world.loaded(p.x, p.z) {
            Err("unloaded position".into())
        } else {
            let tool = request.tool.map(|item| ItemStack {
                item: item.into(),
                count: 1,
                damage: 0,
                extra: None,
            });
            if self
                .res
                .facts()
                .get(self.world.block(p.x, p.y, p.z))
                .hardness
                < 0.
            {
                Err("unbreakable block".into())
            } else {
                match self.native_machine_destroy_block(
                    [p.x, p.y, p.z],
                    tool.as_ref(),
                    request.collect,
                ) {
                    None => Err("air or unavailable block".into()),
                    Some(drops) => match encode_collected(&drops) {
                        Ok(stacks) => Ok(stacks),
                        Err(error) => {
                            for stack in drops {
                                self.pop_resource([p.x, p.y, p.z], stack);
                            }
                            Err(error)
                        }
                    },
                }
            }
        };
        self.enter_level(host, old).map_err(|e| format!("{e:?}"))?;
        result
    }
    pub(super) fn apply_wasm_menu(&mut self, host: Option<&mut LanHost>, operation: MenuOperation) {
        if let Some(host) = host {
            self.apply_wasm_menu_with_host(host, operation);
        } else if let Some(mut host) = self.lan.take() {
            self.apply_wasm_menu_with_host(&mut host, operation);
            self.lan = Some(host);
        }
    }
    fn apply_wasm_menu_with_host(&mut self, host: &mut LanHost, operation: MenuOperation) {
        let player = match &operation {
            MenuOperation::Open { lease, .. } => &lease.player.id,
            MenuOperation::Set { player, .. } | MenuOperation::Close { player, .. } => player,
        };
        let Some(g) = host
            .hub
            .guests
            .iter()
            .find(|g| g.placed && !g.dead && uuid_string(g.uuid) == *player)
        else {
            return;
        };
        let (conn, uuid) = (g.conn, g.uuid);
        match operation {
            MenuOperation::Open {
                lease,
                title,
                items,
            } => {
                if g.dimension != lease.player.dimension || !menu_in_range(&lease, g.position) {
                    return;
                }
                self.guest_close_all(host, conn);
                if let Some((_, old)) = self.mods.machine_menus.remove(&uuid) {
                    self.mods.menu_closed.push(old);
                }
                let items = display_stacks(items);
                if let Some(window) = host.hub.open_native_menu(
                    conn,
                    lease.rows as u8,
                    horizon_client::lan::data::component_nbt(&Text::tr(&title, &[]).component()),
                    items,
                ) {
                    self.mods.mods.menu_opened(lease.clone());
                    self.mods.machine_menus.insert(uuid, (window, lease));
                }
            }
            MenuOperation::Set { owner, items, .. } => {
                if let Some((window, lease)) = self.mods.machine_menus.get(&uuid)
                    && lease.owner == owner
                    && g.menu.as_ref().is_some_and(|m| {
                        m.window == *window && m.slots == horizon_client::lan::menu::Slots::Native
                    })
                    && menu_in_range(lease, g.position)
                {
                    host.hub.set_native_menu(conn, display_stacks(items));
                }
            }
            MenuOperation::Close { owner, .. } => {
                if self.mods.machine_menus.get(&uuid).is_some_and(|(w, l)| {
                    l.owner == owner && g.menu.as_ref().is_some_and(|m| m.window == *w)
                }) {
                    host.hub.close_container(conn);
                }
            }
        }
    }
    pub(super) fn wasm_menu_event(
        &mut self,
        uuid: u128,
        window: i32,
        click: Option<(i32, i32, i32)>,
    ) {
        let Some((expected, lease)) = self.mods.machine_menus.get(&uuid).cloned() else {
            return;
        };
        if expected != window {
            return;
        }
        let player = self
            .server_players(None)
            .into_iter()
            .find(|p| p.id == lease.player.id)
            .unwrap_or_else(|| lease.player.clone());
        if click.is_some() {
            let open = self
                .lan
                .as_ref()
                .and_then(|h| h.hub.guests.iter().find(|g| g.uuid == uuid))
                .is_some_and(|g| {
                    !g.dead
                        && g.dimension == lease.player.dimension
                        && g.menu.as_ref().is_some_and(|m| {
                            m.window == window
                                && m.slots == horizon_client::lan::menu::Slots::Native
                        })
                        && menu_in_range(&lease, g.position)
                });
            if !open {
                return;
            }
        } else {
            self.mods.machine_menus.remove(&uuid);
        }
        // Reuse the server hook loan/flush path; no native mod event is consumed.
        self.dispatch_wasm_menu(&player, click);
    }
    pub(super) fn maintain_wasm_menus(&mut self) {
        let close: Vec<_> = self
            .mods
            .machine_menus
            .iter()
            .filter_map(|(&uuid, (window, lease))| {
                let g = self
                    .lan
                    .as_ref()?
                    .hub
                    .guests
                    .iter()
                    .find(|g| g.uuid == uuid)?;
                (!self.mods.mods.entries().iter().any(|e| {
                    e.status == horizon_mods::Status::Running
                        && e.manifest.as_ref().is_some_and(|m| m.id == lease.owner)
                }) || g.dead
                    || g.dimension != lease.player.dimension
                    || !menu_in_range(lease, g.position))
                .then_some((g.conn, *window))
            })
            .collect();
        if let Some(host) = self.lan.as_mut() {
            for (conn, window) in close {
                if host
                    .hub
                    .guest(conn)
                    .and_then(|g| g.menu.as_ref())
                    .is_some_and(|m| m.window == window)
                {
                    host.hub.close_container(conn);
                }
            }
        }
    }
}
fn menu_in_range(lease: &MenuLease, at: [f64; 3]) -> bool {
    lease.anchor.as_ref().is_none_or(|(dimension, p)| {
        if dimension != &lease.player.dimension {
            return false;
        }
        (at[0] - (f64::from(p.x) + 0.5)).powi(2)
            + (at[1] - (f64::from(p.y) + 0.5)).powi(2)
            + (at[2] - (f64::from(p.z) + 0.5)).powi(2)
            <= 64.
    })
}
fn display_stacks(items: Vec<Option<horizon_mods::MenuItem>>) -> Vec<Option<ItemStack>> {
    use horizon_world::nbt::Tag;
    items
        .into_iter()
        .map(|item| {
            item.map(|item| {
                let mut stack = ItemStack {
                    item: item.item.into(),
                    count: item.count,
                    damage: 0,
                    extra: None,
                };
                if let Some(name) = item.name {
                    stack.data_mut().custom_name = Some(horizon_client::lan::data::component_nbt(
                        &Text::tr(&name, &[]).component(),
                    ));
                }
                if !item.lore.is_empty() {
                    stack.data_mut().set_other(
                        "minecraft:lore",
                        Some(Tag::List(
                            item.lore
                                .into_iter()
                                .map(|s| {
                                    horizon_client::lan::data::component_nbt(
                                        &Text::tr(&s, &[]).component(),
                                    )
                                })
                                .collect(),
                        )),
                    );
                }
                stack
            })
        })
        .collect()
}

fn encode_collected(drops: &[ItemStack]) -> Result<Vec<horizon_mods::MachineStack>, String> {
    if drops.len() > 256 {
        return Err("collected drops exceed 256 stacks; drops spawned".into());
    }
    let mut total = 0;
    drops
        .iter()
        .map(|s| {
            let components = horizon_client::item_data::encode_components(s)
                .map(|tag| {
                    let limits = horizon_world::nbt::Limits {
                        max_bytes: 65536,
                        ..Default::default()
                    };
                    horizon_world::nbt::write_network(&tag, limits).map_err(|_| {
                        "collected components exceed 65536 bytes; drops spawned".to_owned()
                    })
                })
                .transpose()?;
            total += components.as_ref().map_or(0, Vec::len) + s.item.len() + 8;
            if total > 1 << 20 {
                return Err("collected drops exceed 1048576 bytes; drops spawned".into());
            }
            Ok(horizon_mods::MachineStack {
                item: s.item.to_string(),
                count: s.count,
                components,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api30_collected_stacks_keep_all_saved_components_and_bound_output() {
        let mut stack = ItemStack::new("minecraft:diamond", 7);
        stack.set_mod_data("one", Some("saved one")).unwrap();
        stack.set_mod_data("two", Some("saved two")).unwrap();
        let encoded = encode_collected(&[stack.clone()]).unwrap();
        assert_eq!(encoded[0].count, 7);
        let bytes = encoded[0].components.as_ref().unwrap();
        let (tag, used) =
            horizon_world::nbt::read_network(bytes, horizon_world::nbt::Limits::default()).unwrap();
        assert_eq!(used, bytes.len());
        assert_eq!(
            Some(tag),
            horizon_client::item_data::encode_components(&stack)
        );
        assert!(encode_collected(&vec![stack.clone(); 257]).is_err());
        stack.data_mut().set_other(
            "minecraft:custom_data",
            Some(horizon_world::nbt::Tag::compound([(
                "large",
                horizon_world::nbt::Tag::ByteArray(vec![0; 65536]),
            )])),
        );
        assert!(encode_collected(&[stack]).is_err());
    }
}
