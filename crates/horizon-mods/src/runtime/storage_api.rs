//! Deferred, capacity-checked reservations shared by all server mods in a lend.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum StorageLocation {
    Container {
        dimension: String,
        pos: (i32, i32, i32),
    },
    Player(String),
}
#[derive(Clone, Debug, PartialEq)]
pub struct StorageStack {
    pub info: StackInfo,
    pub max: u32,
}
#[derive(Clone, Debug, PartialEq)]
pub struct StorageMove {
    pub from: StorageLocation,
    pub to: StorageLocation,
    pub from_slot: u32,
    pub to_slot: Option<u32>,
    pub count: u32,
    pub before_from: Vec<Option<StorageStack>>,
    pub before_to: Vec<Option<StorageStack>>,
}
#[derive(Default)]
pub struct StorageView {
    pub slots: HashMap<StorageLocation, Vec<Option<StorageStack>>>,
    pub read_only: std::collections::HashSet<StorageLocation>,
}
impl StorageView {
    pub fn reserve(
        &mut self,
        from: StorageLocation,
        slot: u32,
        to: StorageLocation,
        to_slot: Option<u32>,
        count: u32,
    ) -> Result<Option<StorageMove>, String> {
        if self.read_only.contains(&from) || self.read_only.contains(&to) {
            return Err("special machine slots are read-only in the storage API".into());
        }
        let source = self
            .slots
            .get(&from)
            .ok_or("source is not loaded or online")?;
        let target = self
            .slots
            .get(&to)
            .ok_or("destination is not loaded or online")?;
        let stack = source
            .get(slot as usize)
            .ok_or("source slot out of range")?;
        if to_slot.is_some_and(|s| s as usize >= target.len()) {
            return Err("destination slot out of range".into());
        }
        if count == 0 || (from == to && to_slot == Some(slot)) {
            return Ok(None);
        }
        let Some(stack) = stack else {
            return Ok(None);
        };
        let count = count.min(stack.info.count);
        if count == 0 {
            return Ok(None);
        }
        let mut after = target.clone();
        let mut left = count;
        // Fill matching stacks before empty slots, regardless of slot order.
        for empty in [false, true] {
            for (i, dst) in after.iter_mut().enumerate() {
                if to_slot.is_some_and(|s| s as usize != i) || (from == to && slot as usize == i) {
                    continue;
                }
                let capacity = match dst {
                    Some(d)
                        if !empty
                            && d.info.item == stack.info.item
                            && d.info.display_key == stack.info.display_key =>
                    {
                        d.max.min(stack.max).saturating_sub(d.info.count)
                    }
                    None if empty => stack.max,
                    _ => 0,
                };
                let moved = left.min(capacity);
                if moved == 0 {
                    continue;
                }
                if let Some(d) = dst {
                    d.info.count += moved;
                } else {
                    let mut d = stack.clone();
                    d.info.count = moved;
                    *dst = Some(d);
                }
                left -= moved;
            }
        }
        if left != 0 {
            return Ok(None);
        }
        let plan = StorageMove {
            from: from.clone(),
            to: to.clone(),
            from_slot: slot,
            to_slot,
            count,
            before_from: source.clone(),
            before_to: target.clone(),
        };
        self.slots.insert(to, after);
        let source = self.slots.get_mut(&from).unwrap();
        let stack = source[slot as usize].as_mut().unwrap();
        stack.info.count -= count;
        if stack.info.count == 0 {
            source[slot as usize] = None;
        }
        Ok(Some(plan))
    }
}
impl State {
    fn reserve_storage(
        &mut self,
        from: StorageLocation,
        slot: u32,
        to: StorageLocation,
        to_slot: Option<u32>,
        count: u32,
    ) -> Result<u32, String> {
        if !channel_host::allowance(&mut self.storage_move_budget, self.snapshot.tick, 64) {
            return Err("at most 64 storage moves per mod per tick".into());
        }
        let mut view = self.world.lock().map_err(|_| "world unavailable")?;
        let storage = view
            .as_mut()
            .and_then(|w| w.storage())
            .ok_or("storage unavailable")?;
        let Some(plan) = storage.reserve(from, slot, to, to_slot, count)? else {
            return Ok(0);
        };
        let count = plan.count;
        self.out.push(Output::StorageMove(plan));
        Ok(count)
    }
    fn storage_player(&self, player: &str) -> Result<StorageLocation, String> {
        self.snapshot
            .players
            .iter()
            .find(|p| p.id == player || p.name == player)
            .map(|p| StorageLocation::Player(p.id.clone()))
            .ok_or_else(|| "unknown or offline player".into())
    }
}
fn location(dimension: String, pos: BlockChangePos) -> StorageLocation {
    StorageLocation::Container {
        dimension,
        pos: (pos.x, pos.y, pos.z),
    }
}
impl State {
    pub(super) fn storage_player_stacks(
        &mut self,
        player: String,
    ) -> Option<Vec<Option<StackInfo>>> {
        let player = self.storage_player(&player).ok()?;
        let mut view = self.world.lock().ok()?;
        Some(
            view.as_mut()?
                .storage()?
                .slots
                .get(&player)?
                .iter()
                .map(|s| s.as_ref().map(|s| s.info.clone()))
                .collect(),
        )
    }
    pub(super) fn storage_stacks(
        &mut self,
        dimension: String,
        pos: BlockChangePos,
    ) -> Option<Vec<Option<StackInfo>>> {
        let mut view = self.world.lock().ok()?;
        Some(
            view.as_mut()?
                .storage()?
                .slots
                .get(&location(dimension, pos))?
                .iter()
                .map(|s| s.as_ref().map(|s| s.info.clone()))
                .collect(),
        )
    }
    pub(super) fn storage_container_move(
        &mut self,
        dimension: String,
        from: BlockChangePos,
        from_slot: u32,
        to: BlockChangePos,
        to_slot: Option<u32>,
        count: u32,
    ) -> Result<u32, String> {
        self.reserve_storage(
            location(dimension.clone(), from),
            from_slot,
            location(dimension, to),
            to_slot,
            count,
        )
    }
    pub(super) fn storage_to_player(
        &mut self,
        dimension: String,
        pos: BlockChangePos,
        slot: u32,
        count: u32,
        player: String,
    ) -> Result<u32, String> {
        let player = self.storage_player(&player)?;
        self.reserve_storage(location(dimension, pos), slot, player, None, count)
    }
    pub(super) fn storage_from_player(
        &mut self,
        player: String,
        inv_slot: u32,
        dimension: String,
        pos: BlockChangePos,
        count: u32,
    ) -> Result<u32, String> {
        let player = self.storage_player(&player)?;
        self.reserve_storage(player, inv_slot, location(dimension, pos), None, count)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn stack(key: &str, count: u32) -> Option<StorageStack> {
        Some(StorageStack {
            info: StackInfo {
                item: "minecraft:stone".into(),
                count,
                display_key: key.into(),
                custom_name: None,
            },
            max: 64,
        })
    }
    #[test]
    fn reservations_check_capacity_and_share_changes_without_losing_components() {
        let a = StorageLocation::Player("a".into());
        let b = StorageLocation::Player("b".into());
        let mut v = StorageView {
            slots: [
                (a.clone(), vec![stack("renamed", 10)]),
                (
                    b.clone(),
                    vec![stack("plain", 64), stack("renamed", 60), None],
                ),
            ]
            .into(),
            ..Default::default()
        };
        let plan = v
            .reserve(a.clone(), 0, b.clone(), None, 10)
            .unwrap()
            .unwrap();
        assert_eq!(plan.count, 10);
        assert_eq!(v.slots[&a], [None]);
        assert_eq!(
            v.slots[&b],
            [
                stack("plain", 64),
                stack("renamed", 64),
                stack("renamed", 6)
            ]
        );
        assert!(
            v.reserve(b.clone(), 2, a.clone(), Some(0), 7)
                .unwrap()
                .is_some()
        );
        let before = v.slots.clone();
        assert!(
            v.reserve(a.clone(), 0, b.clone(), Some(0), 1)
                .unwrap()
                .is_none()
        );
        assert_eq!(v.slots, before);
        assert!(v.reserve(a.clone(), 99, b, None, 1).is_err());
        assert!(v.reserve(a.clone(), 0, a, Some(0), 1).unwrap().is_none());
    }
}
