//! Server mod inventory calls: lend real stacks, restore them, then sync changed guests.
use super::{Game, lan::LanHost, server_mods::run::uuid_string};
use horizon_client::{
    items::ItemStack,
    mod_players::{Inventories, Inventory},
};
use std::{collections::HashMap, sync::Arc};

pub(super) struct ItemDrop {
    dimension: String,
    at: [f64; 3],
    stack: ItemStack,
}
impl Game<'_> {
    pub(super) fn lend_mod_inventories(&mut self, host: Option<&mut LanHost>) {
        if !self.mods.mods.wants_players() {
            return;
        }
        let items = self
            .mods
            .item_limits
            .get_or_insert_with(|| {
                Arc::new(
                    self.res
                        .items()
                        .items
                        .iter()
                        .map(|(id, info)| (id.to_string(), info.max_stack))
                        .collect(),
                )
            })
            .clone();
        let mut players = HashMap::new();
        if self.options.execution_role.is_client() && self.remote.is_none() && self.player.is_some()
        {
            players.insert(
                self.player_uuid_string(),
                Inventory {
                    main: std::mem::take(&mut self.inventory),
                    armor: std::mem::take(&mut self.armor),
                    offhand: self.offhand.take(),
                    selected: self.selected,
                    creative: self.creative,
                    ..Inventory::default()
                },
            );
        }
        if let Some(host) = host.or(self.lan.as_mut()) {
            for g in host.hub.guests.iter_mut().filter(|g| g.placed) {
                let creative = g.creative();
                players.insert(
                    uuid_string(g.uuid),
                    Inventory {
                        main: std::mem::take(&mut g.inventory),
                        armor: std::mem::take(&mut g.armor),
                        offhand: g.offhand.take(),
                        selected: g.selected,
                        creative,
                        ..Inventory::default()
                    },
                );
            }
        }
        self.mods
            .mods
            .lend_inventories(Box::new(Inventories { items, players }));
    }
    pub(super) fn return_mod_inventories(&mut self, host: Option<&mut LanHost>) {
        let Some(loan) = self.mods.mods.take_inventories() else {
            return;
        };
        let loan: Box<dyn std::any::Any> = loan;
        let Ok(mut loan) = loan.downcast::<Inventories>() else {
            return;
        };
        if self.options.execution_role.is_client()
            && self.remote.is_none()
            && self.player.is_some()
            && let Some(mut inv) = loan.players.remove(&self.player_uuid_string())
        {
            self.inventory = inv.main;
            self.armor = inv.armor;
            self.offhand = inv.offhand;
            let p = self.player.as_ref().unwrap().position;
            self.mods
                .item_drops
                .extend(inv.drops.drain(..).map(|stack| ItemDrop {
                    dimension: self.dim.id().into(),
                    at: [p.x, p.y, p.z],
                    stack,
                }));
        }
        if let Some(host) = host.or(self.lan.as_mut()) {
            for i in 0..host.hub.guests.len() {
                let g = &mut host.hub.guests[i];
                let Some(inv) = loan.players.remove(&uuid_string(g.uuid)) else {
                    continue;
                };
                g.inventory = inv.main;
                g.armor = inv.armor;
                g.offhand = inv.offhand;
                self.mods
                    .item_drops
                    .extend(inv.drops.into_iter().map(|stack| ItemDrop {
                        dimension: g.dimension.clone(),
                        at: g.position,
                        stack,
                    }));
                if inv.changed {
                    host.hub.send_inventory(i);
                }
            }
        }
    }
    pub(super) fn apply_mod_item_drops(&mut self) {
        for drop in std::mem::take(&mut self.mods.item_drops) {
            if drop.dimension == self.dim.id() {
                self.spawn_item(drop.stack, drop.at, [0.0, 0.1, 0.0], 10);
            } else {
                self.mods.item_drops.push(drop);
            }
        }
    }
}
