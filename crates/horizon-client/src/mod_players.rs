//! Capability `players`: authoritative inventories moved into the mod runtime during hooks.
use crate::items::ItemStack;
use horizon_mods::PlayerInventories;
use std::{collections::HashMap, sync::Arc};

#[derive(Default)]
pub struct Inventory {
    pub main: Vec<Option<ItemStack>>,
    pub armor: [Option<ItemStack>; 4],
    pub offhand: Option<ItemStack>,
    pub selected: usize,
    pub creative: bool,
    pub changed: bool,
    pub drops: Vec<ItemStack>,
}
/// Registry ids and stack limits include vanilla and loaded content mods.
pub struct Inventories {
    pub items: Arc<HashMap<String, u32>>,
    pub players: HashMap<String, Inventory>,
}
impl Inventories {
    fn checked(&mut self, player: &str, item: &str) -> Result<(&mut Inventory, u32), String> {
        let max = self
            .items
            .get(item)
            .copied()
            .ok_or_else(|| format!("unknown item {item}"))?;
        let inv = self
            .players
            .get_mut(player)
            .ok_or_else(|| format!("no online player {player}"))?;
        Ok((inv, max))
    }
}
impl PlayerInventories for Inventories {
    fn take_item(&mut self, player: &str, item: &str, count: u32) -> Result<u32, String> {
        let (inv, _) = self.checked(player, item)?;
        if inv.creative {
            return Ok(count);
        }
        if count == 0 {
            return Ok(0);
        }
        let mut left = count;
        let selected = inv.selected.min(inv.main.len());
        let (main, rest) = inv.main.split_at_mut(selected);
        let (held, after) = if rest.is_empty() {
            (rest, &mut [][..])
        } else {
            rest.split_at_mut(1)
        };
        // Selected hand, other inventory slots, armor, then off hand. Match ids, not components.
        for slot in held
            .iter_mut()
            .chain(main)
            .chain(after)
            .chain(&mut inv.armor)
            .chain(std::iter::once(&mut inv.offhand))
        {
            if let Some(stack) = slot
                && &*stack.item == item
            {
                let taken = left.min(stack.count);
                stack.count -= taken;
                left -= taken;
                if stack.count == 0 {
                    *slot = None;
                }
            }
            if left == 0 {
                break;
            }
        }
        inv.changed |= left != count;
        Ok(count - left)
    }
    fn give_item(&mut self, player: &str, item: &str, count: u32) -> Result<(), String> {
        let (inv, max) = self.checked(player, item)?;
        if count > max.saturating_mul(100) {
            return Err("give-item is at most 100 stacks per call".into());
        }
        if count == 0 {
            return Ok(());
        }
        let mut stack = ItemStack::new(item, count);
        let selected = inv.selected.min(inv.main.len());
        let (main, rest) = inv.main.split_at_mut(selected);
        let (held, after) = if rest.is_empty() {
            (rest, &mut [][..])
        } else {
            rest.split_at_mut(1)
        };
        for slot in held
            .iter_mut()
            .chain(std::iter::once(&mut inv.offhand))
            .chain(main)
            .chain(after)
            .flatten()
        {
            if slot.same_item_same_components(&stack) && slot.count < max {
                let moved = (max - slot.count).min(stack.count);
                slot.count += moved;
                stack.count -= moved;
                inv.changed |= moved > 0;
                if stack.count == 0 {
                    return Ok(());
                }
            }
        }
        for slot in inv.main.iter_mut().filter(|s| s.is_none()) {
            let moved = max.min(stack.count);
            *slot = Some(ItemStack {
                count: moved,
                ..stack.clone()
            });
            stack.count -= moved;
            inv.changed = true;
            if stack.count == 0 {
                return Ok(());
            }
        }
        // Item entities carry legal stack sizes, including nonstackable items.
        while stack.count > 0 {
            let moved = max.min(stack.count);
            inv.drops.push(ItemStack {
                count: moved,
                ..stack.clone()
            });
            stack.count -= moved;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ALICE: &str = "00000000-0000-4000-8000-000000000001";
    const BOB: &str = "00000000-0000-4000-8000-000000000002";
    fn inventories() -> Inventories {
        Inventories {
            items: Arc::new(
                [
                    ("minecraft:beef".into(), 64),
                    ("minecraft:stone".into(), 64),
                    ("ruby:scale".into(), 1),
                ]
                .into(),
            ),
            players: [
                (
                    ALICE.into(),
                    Inventory {
                        main: vec![None; 36],
                        selected: 5,
                        ..Default::default()
                    },
                ),
                (
                    BOB.into(),
                    Inventory {
                        main: vec![None; 36],
                        selected: 0,
                        ..Default::default()
                    },
                ),
            ]
            .into(),
        }
    }
    #[test]
    fn taking_prefers_main_hand_then_rest_preserves_components_and_never_double_spends() {
        let mut inv = inventories();
        let a = inv.players.get_mut(ALICE).unwrap();
        a.main[0] = Some(ItemStack::new("minecraft:beef", 3));
        a.main[5] = Some(ItemStack::new("minecraft:beef", 3));
        a.main[5].as_mut().unwrap().damage = 7;
        a.armor[1] = Some(ItemStack::new("minecraft:beef", 2));
        a.offhand = Some(ItemStack::new("minecraft:beef", 1));
        inv.give_item(BOB, "minecraft:beef", 8).unwrap();
        assert_eq!(inv.take_item(ALICE, "minecraft:beef", 2).unwrap(), 2);
        let a = &inv.players[ALICE];
        assert_eq!(a.main[0].as_ref().unwrap().count, 3);
        assert_eq!(
            (
                a.main[5].as_ref().unwrap().count,
                a.main[5].as_ref().unwrap().damage
            ),
            (1, 7)
        );
        assert_eq!(inv.take_item(ALICE, "minecraft:beef", 99).unwrap(), 7);
        assert_eq!(inv.take_item(ALICE, "minecraft:beef", 99).unwrap(), 0);
        assert!(inv.players[ALICE].main.iter().all(Option::is_none));
        assert!(inv.players[ALICE].offhand.is_none());
        assert!(inv.players[ALICE].armor.iter().all(Option::is_none));
        assert_eq!(inv.take_item(BOB, "minecraft:beef", 99).unwrap(), 8);
        inv.give_item(ALICE, "minecraft:beef", 4).unwrap();
        assert_eq!(inv.take_item(ALICE, "minecraft:beef", 4).unwrap(), 4);
    }
    #[test]
    fn creative_takes_are_free_and_ids_and_players_are_checked_even_for_zero() {
        let mut inv = inventories();
        inv.players.get_mut(ALICE).unwrap().creative = true;
        assert_eq!(
            inv.take_item(ALICE, "minecraft:beef", u32::MAX).unwrap(),
            u32::MAX
        );
        assert!(!inv.players[ALICE].changed);
        assert!(inv.take_item(ALICE, "minecraft:missing", 0).is_err());
        assert!(inv.give_item(ALICE, "minecraft:missing", 0).is_err());
        assert!(inv.take_item("offline", "minecraft:beef", 0).is_err());
        assert!(inv.give_item("offline", "minecraft:beef", 0).is_err());
        inv.give_item(ALICE, "ruby:scale", 3).unwrap();
        assert_eq!(
            inv.players[ALICE]
                .main
                .iter()
                .flatten()
                .map(|s| s.count)
                .sum::<u32>(),
            3
        );
        assert_eq!(inv.take_item(ALICE, "ruby:scale", 2).unwrap(), 2);
        assert_eq!(
            inv.players[ALICE]
                .main
                .iter()
                .flatten()
                .map(|s| s.count)
                .sum::<u32>(),
            3
        );
    }
    #[test]
    fn giving_merges_selected_and_offhand_then_empty_slots_and_drops_legal_stacks() {
        let mut inv = inventories();
        let a = inv.players.get_mut(ALICE).unwrap();
        a.main.fill(Some(ItemStack::new("minecraft:stone", 64)));
        a.main[5] = Some(ItemStack::new("minecraft:beef", 60));
        a.offhand = Some(ItemStack::new("minecraft:beef", 60));
        a.main[10] = None;
        inv.give_item(ALICE, "minecraft:beef", 10).unwrap();
        let a = &inv.players[ALICE];
        assert_eq!(a.main[5].as_ref().unwrap().count, 64);
        assert_eq!(a.offhand.as_ref().unwrap().count, 64);
        assert_eq!(a.main[10].as_ref().unwrap().count, 2);
        assert!(a.drops.is_empty());
        inv.give_item(ALICE, "ruby:scale", 3).unwrap();
        assert_eq!(
            inv.players[ALICE].drops,
            vec![ItemStack::new("ruby:scale", 1); 3]
        );
        let a = &inv.players[ALICE];
        let before = (a.main.clone(), a.drops.clone());
        assert!(inv.give_item(ALICE, "ruby:scale", 101).is_err());
        assert_eq!(
            (
                inv.players[ALICE].main.clone(),
                inv.players[ALICE].drops.clone()
            ),
            before
        );
    }
    #[test]
    fn giving_does_not_merge_stacks_with_different_components_or_discard_creative_overflow() {
        let mut inv = inventories();
        let a = inv.players.get_mut(ALICE).unwrap();
        a.creative = true;
        a.main.fill(Some(ItemStack::new("minecraft:stone", 64)));
        a.main[5] = Some(ItemStack::new("minecraft:beef", 1));
        a.main[5].as_mut().unwrap().damage = 1;
        inv.give_item(ALICE, "minecraft:beef", 65).unwrap();
        assert_eq!(inv.players[ALICE].main[5].as_ref().unwrap().count, 1);
        assert_eq!(
            inv.players[ALICE].drops,
            vec![
                ItemStack::new("minecraft:beef", 64),
                ItemStack::new("minecraft:beef", 1)
            ]
        );
        assert!(!inv.players[ALICE].changed);
    }
}
