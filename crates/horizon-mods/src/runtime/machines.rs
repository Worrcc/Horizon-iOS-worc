//! API 30: bounded display menus, saved block data and live machine requests.
use super::*;
use server_bindings::horizon::mods::menus;
use std::sync::mpsc::{self, Sender, SyncSender};

#[derive(Clone, Debug, PartialEq)]
pub struct BlockDataImage {
    pub state: String,
    pub token: String,
    pub value: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct BlockDataWrite {
    pub mod_id: String,
    pub dimension: String,
    pub pos: BlockChangePos,
    pub before: BlockDataImage,
    pub value: Option<String>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct MenuLease {
    pub owner: String,
    pub rows: u32,
    pub player: ServerPlayer,
    pub anchor: Option<(String, BlockPos)>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum MenuOperation {
    Open {
        lease: MenuLease,
        title: String,
        items: Vec<Option<MenuItem>>,
    },
    Set {
        owner: String,
        player: String,
        items: Vec<Option<MenuItem>>,
    },
    Close {
        owner: String,
        player: String,
    },
}
pub(super) type MenuLeases = Arc<Mutex<HashMap<String, MenuLease>>>;
#[derive(Clone, Debug, PartialEq)]
pub struct DestroyRequest {
    pub mod_id: String,
    pub dimension: String,
    pub pos: BlockChangePos,
    pub tool: Option<String>,
    pub collect: bool,
}
type Reply = Result<Vec<MachineStack>, String>;
type Message = (DestroyRequest, SyncSender<Reply>);
pub(super) type Bridge = Arc<Mutex<Option<Sender<Message>>>>;
#[derive(Default)]
pub(super) struct MachineState {
    bridge: Bridge,
    leases: MenuLeases,
    menu_budget: (Option<u64>, usize),
    click_budget: (Option<u64>, usize),
    destroy_budget: (Option<u64>, usize),
    pub progress_budget: (Option<u64>, usize),
    pub anchor: Option<(String, BlockPos)>,
}
impl MachineState {
    pub fn new(bridge: Bridge, leases: MenuLeases) -> Self {
        Self {
            bridge,
            leases,
            ..Default::default()
        }
    }
}
pub fn validate_block_data(value: Option<&str>) -> Result<(), String> {
    if value.is_some_and(|s| s.len() > 4096) {
        Err("block data exceeds 4096 bytes".into())
    } else {
        Ok(())
    }
}
pub fn menu_inventory_slot(rows: u32, slot: i32) -> Option<u32> {
    if !(1..=6).contains(&rows) {
        return None;
    }
    let slot = u32::try_from(slot).ok()?.checked_sub(rows * 9)?;
    match slot {
        0..=26 => Some(slot + 9),
        27..=35 => Some(slot - 27),
        _ => None,
    }
}
pub fn valid_menu_click(rows: u32, slot: i32, button: i32, mode: i32) -> bool {
    if !(1..=6).contains(&rows) || !(slot == -999 || (0..(rows * 9 + 36) as i32).contains(&slot)) {
        return false;
    }
    match mode {
        0 | 1 | 4 | 6 => matches!(button, 0 | 1),
        2 => (0..=8).contains(&button) || button == 40,
        3 => button == 2,
        5 => matches!(button, 0..=2 | 4..=6 | 8..=10),
        _ => false,
    }
}
pub fn validate_menu(rows: u32, title: &str, items: &[Option<MenuItem>]) -> Result<(), String> {
    if !(1..=6).contains(&rows) {
        return Err("rows must be 1..=6".into());
    }
    if title.len() > 256 || items.len() > (rows * 9) as usize {
        return Err("title exceeds 256 bytes or items exceed menu slots".into());
    }
    for item in items.iter().flatten() {
        if !resource_id(&item.item)
            || item.item.len() > 256
            || item.name.as_ref().is_some_and(|s| s.len() > 256)
            || item.lore.len() > 8
            || item.lore.iter().any(|s| s.len() > 256)
            || !(1..=99).contains(&item.count)
        {
            return Err(
                "invalid menu item, name (256 bytes), lore (8 lines of 256 bytes), or count".into(),
            );
        }
    }
    Ok(())
}
impl State {
    pub(super) fn block_image(
        &self,
        dimension: &str,
        pos: BlockChangePos,
    ) -> Result<BlockDataImage, String> {
        self.machine_position(dimension, pos)?;
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let image = view
            .as_ref()
            .ok_or("world unavailable")?
            .block_data(dimension, pos, &self.id)?;
        validate_block_data(image.value.as_deref())?;
        Ok(image)
    }
    pub(super) fn machine_position(
        &self,
        dimension: &str,
        pos: BlockChangePos,
    ) -> Result<(), String> {
        if !self.server {
            return Err("server only".into());
        }
        if !(-30_000_000..30_000_000).contains(&pos.x)
            || !(-30_000_000..30_000_000).contains(&pos.z)
        {
            return Err("position outside world limits".into());
        }
        let world = self.world.lock().map_err(|_| "world unavailable")?;
        let world = world.as_ref().ok_or("world unavailable")?;
        if !world.has_dimension(dimension) || world.block(dimension, pos.x, pos.y, pos.z).is_none()
        {
            return Err("unknown dimension or unloaded position".into());
        }
        Ok(())
    }
    pub(super) fn machine_destroy(
        &mut self,
        dimension: String,
        pos: BlockChangePos,
        tool: Option<String>,
        collect: bool,
    ) -> Reply {
        self.machine_position(&dimension, pos)?;
        if let Some(tool) = &tool {
            if tool.len() > 256 {
                return Err("tool id exceeds 256 bytes".into());
            }
            self.world
                .lock()
                .map_err(|_| "world unavailable")?
                .as_ref()
                .ok_or("world unavailable")?
                .check_item_stack(tool, 1)?;
        }
        if !channel_host::allowance(&mut self.machines.destroy_budget, self.snapshot.tick, 64) {
            return Err("at most 64 destroy requests per mod per tick".into());
        }
        let bridge = self
            .machines
            .bridge
            .lock()
            .map_err(|_| "machine host unavailable")?
            .clone()
            .ok_or("machine host unavailable in this hook")?;
        let (reply, answer) = mpsc::sync_channel(0);
        bridge
            .send((
                DestroyRequest {
                    mod_id: self.id.clone(),
                    dimension,
                    pos,
                    tool,
                    collect,
                },
                reply,
            ))
            .map_err(|_| "machine host stopped")?;
        answer.recv().map_err(|_| "machine host stopped")?
    }
    fn menu_player(&self, player: &str) -> Result<ServerPlayer, String> {
        self.snapshot
            .players
            .iter()
            .find(|p| p.id == player || p.name == player)
            .filter(|p| p.health > 0.)
            .cloned()
            .ok_or_else(|| "unknown, offline or dead player".into())
    }
    fn menu_allow(&mut self) -> Result<(), String> {
        if !self.server {
            return Err("server only".into());
        }
        if !channel_host::allowance(&mut self.machines.menu_budget, self.snapshot.tick, 64) {
            return Err("at most 64 menu writes per mod per tick".into());
        }
        Ok(())
    }
}
impl menus::Host for State {
    fn open_menu(
        &mut self,
        player: String,
        rows: u32,
        title: String,
        items: Vec<Option<MenuItem>>,
    ) -> Result<(), String> {
        let player = self.menu_player(&player)?;
        validate_menu(rows, &title, &items)?;
        if self.machines.anchor.as_ref().is_some_and(|(dim, pos)| {
            dim != &player.dimension
                || (player.x - (f64::from(pos.x) + 0.5)).powi(2)
                    + (player.y - (f64::from(pos.y) + 0.5)).powi(2)
                    + (player.z - (f64::from(pos.z) + 0.5)).powi(2)
                    > 64.
        }) {
            return Err("player is outside opening block range (8 blocks)".into());
        }
        {
            let view = self.world.lock().map_err(|_| "world unavailable")?;
            let view = view.as_ref().ok_or("world unavailable")?;
            for item in items.iter().flatten() {
                view.check_item_stack(&item.item, item.count)?;
            }
        }
        self.menu_allow()?;
        let lease = MenuLease {
            owner: self.id.clone(),
            rows,
            player,
            anchor: self.machines.anchor.clone(),
        };
        self.out.push(Output::Menu(MenuOperation::Open {
            lease,
            title,
            items,
        }));
        Ok(())
    }
    fn set_menu_items(
        &mut self,
        player: String,
        items: Vec<Option<MenuItem>>,
    ) -> Result<(), String> {
        let player = self.menu_player(&player)?;
        let rows = self
            .machines
            .leases
            .lock()
            .map_err(|_| "menu unavailable")?
            .get(&player.id)
            .filter(|m| m.owner == self.id)
            .map(|m| m.rows)
            .or_else(|| {
                self.out.iter().rev().find_map(|o| match o {
                    Output::Menu(MenuOperation::Open { lease, .. })
                        if lease.player.id == player.id && lease.owner == self.id =>
                    {
                        Some(lease.rows)
                    }
                    _ => None,
                })
            })
            .ok_or("no menu owned by this mod")?;
        validate_menu(rows, "", &items)?;
        {
            let view = self.world.lock().map_err(|_| "world unavailable")?;
            let view = view.as_ref().ok_or("world unavailable")?;
            for item in items.iter().flatten() {
                view.check_item_stack(&item.item, item.count)?;
            }
        }
        self.menu_allow()?;
        self.out.push(Output::Menu(MenuOperation::Set {
            owner: self.id.clone(),
            player: player.id,
            items,
        }));
        Ok(())
    }
    fn close_menu(&mut self, player: String) -> Result<(), String> {
        let player = self.menu_player(&player)?;
        let owns = self.machines.leases.lock().map_err(|_| "menu unavailable")?.get(&player.id).is_some_and(|m| m.owner == self.id) || self.out.iter().any(|o| matches!(o, Output::Menu(MenuOperation::Open { lease, .. }) if lease.player.id == player.id && lease.owner == self.id));
        if !owns {
            return Err("no menu owned by this mod".into());
        }
        self.menu_allow()?;
        self.out.push(Output::Menu(MenuOperation::Close {
            owner: self.id.clone(),
            player: player.id,
        }));
        Ok(())
    }
    fn inventory_slot(&mut self, rows: u32, slot: i32) -> Option<u32> {
        menu_inventory_slot(rows, slot)
    }
}
impl Mods {
    pub fn menu_opened(&mut self, lease: MenuLease) {
        self.menu_leases
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(lease.player.id.clone(), lease);
    }
    pub fn menu_lease(&self, player: &str) -> Option<MenuLease> {
        self.menu_leases.lock().ok()?.get(player).cloned()
    }
    pub fn menu_event(&mut self, player: &ServerPlayer, click: Option<(i32, i32, i32)>) {
        let lease = if click.is_none() {
            self.menu_leases
                .lock()
                .ok()
                .and_then(|mut m| m.remove(&player.id))
        } else {
            self.menu_lease(&player.id)
        };
        let Some(lease) = lease else {
            return;
        };
        if let Some((slot, button, mode)) = click {
            if player.dimension != lease.player.dimension
                || player.health <= 0.
                || !valid_menu_click(lease.rows, slot, button, mode)
            {
                return;
            }
            if lease.anchor.as_ref().is_some_and(|(dim, p)| {
                dim != &player.dimension
                    || (player.x - (f64::from(p.x) + 0.5)).powi(2)
                        + (player.y - (f64::from(p.y) + 0.5)).powi(2)
                        + (player.z - (f64::from(p.z) + 0.5)).powi(2)
                        > 64.
            }) {
                return;
            }
        }
        let Some(i) = self.entries.iter().position(|e| {
            e.run.is_some() && e.manifest.as_ref().is_some_and(|m| m.id == lease.owner)
        }) else {
            return;
        };
        if click.is_some()
            && !channel_host::allowance(
                &mut self.entries[i]
                    .run
                    .as_mut()
                    .unwrap()
                    .store
                    .data_mut()
                    .machines
                    .click_budget,
                self.snapshot.tick,
                64,
            )
        {
            return;
        }
        self.call(i, FUEL_EVENT, |r| {
            if let Hooks::Server(m, _) = &r.hooks {
                if let Some((slot, button, mode)) = click {
                    if let Some(f) = m.menu_clicked {
                        f.call(&mut r.store, (player.clone(), slot, button, mode))?;
                    }
                } else if let Some(f) = m.menu_closed {
                    f.call(&mut r.store, (player.clone(),))?;
                }
            }
            Ok(())
        });
    }
    pub fn wants_machines(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.machine_api))
    }
    pub fn block_data_mods(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter_map(|e| {
                e.run
                    .as_ref()
                    .filter(|r| r.block_data_api)
                    .map(|r| r.store.data().id.clone())
            })
            .collect()
    }
    pub fn close_menu_lease(&mut self, lease: &MenuLease) {
        let Some(i) = self.entries.iter().position(|e| {
            e.run.is_some() && e.manifest.as_ref().is_some_and(|m| m.id == lease.owner)
        }) else {
            return;
        };
        self.call(i, FUEL_EVENT, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.menu_closed
            {
                f.call(&mut r.store, (lease.player.clone(),))?;
            }
            Ok(())
        });
    }
    pub fn with_machines<R: Send>(
        &mut self,
        mut handle: impl FnMut(DestroyRequest, &mut dyn WorldView) -> Reply,
        run: impl FnOnce(&mut Self) -> R + Send,
    ) -> R {
        if !self.wants_machines() {
            return run(self);
        }
        let (sender, requests) = mpsc::channel();
        *self
            .machine_bridge
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(sender);
        let bridge = self.machine_bridge.clone();
        let world = self.world.clone();
        std::thread::scope(|scope| {
            let worker = scope.spawn(move || {
                struct Disconnect(Bridge);
                impl Drop for Disconnect {
                    fn drop(&mut self) {
                        self.0
                            .lock()
                            .unwrap_or_else(std::sync::PoisonError::into_inner)
                            .take();
                    }
                }
                let _disconnect = Disconnect(bridge);
                run(self)
            });
            for (request, reply) in requests {
                let result = match world.lock() {
                    Ok(mut w) => match w.as_mut() {
                        Some(w) => handle(request, w.as_mut()),
                        None => Err("world unavailable".into()),
                    },
                    Err(_) => Err("world unavailable".into()),
                };
                let _ = reply.send(result);
            }
            worker
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn api30_validators_bound_utf8_and_slot_mapping() {
        assert!(validate_block_data(Some(&"é".repeat(2048))).is_ok());
        assert!(validate_block_data(Some(&"é".repeat(2049))).is_err());
        assert!(validate_block_data(Some("")).is_ok());
        for rows in 1..=6 {
            for n in 0..36 {
                assert_eq!(
                    menu_inventory_slot(rows, (rows * 9 + n) as i32),
                    Some(if n < 27 { n + 9 } else { n - 27 })
                );
            }
            assert_eq!(menu_inventory_slot(rows, (rows * 9 - 1) as i32), None);
            assert_eq!(menu_inventory_slot(rows, (rows * 9 + 36) as i32), None);
        }
        assert!(valid_menu_click(3, 54, 40, 2));
        assert!(valid_menu_click(3, -999, 1, 0));
        for (slot, button, mode) in [(63, 0, 0), (-1, 0, 0), (0, 10, 0), (0, 3, 5), (0, 0, 7)] {
            assert!(!valid_menu_click(3, slot, button, mode));
        }
        assert!(validate_menu(1, "", &vec![None; 10]).is_err());
        assert!(validate_menu(6, &"x".repeat(256), &vec![None; 54]).is_ok());
    }
    #[test]
    fn api30_caps_do_not_reset_between_calls() {
        let mut cap = (None, 0);
        for tick in [1, 2] {
            for _ in 0..64 {
                assert!(channel_host::allowance(&mut cap, tick, 64));
            }
            assert!(!channel_host::allowance(&mut cap, tick, 64));
        }
    }
}
