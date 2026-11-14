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
    fn equipment(&self, player: &str) -> Option<horizon_mods::EquipmentSlots> {
        let inv = self.players.get(player)?;
        let id = |slot: &Option<ItemStack>| {
            slot.as_ref()
                .filter(|s| s.count > 0)
                .map(|s| s.item.to_string())
        };
        Some(horizon_mods::EquipmentSlots {
            head: id(&inv.armor[0]),
            chest: id(&inv.armor[1]),
            legs: id(&inv.armor[2]),
            feet: id(&inv.armor[3]),
            main_hand: inv.main.get(inv.selected).and_then(id),
            off_hand: id(&inv.offhand),
            hotbar: (0..9).map(|i| inv.main.get(i).and_then(id)).collect(),
        })
    }
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
        self.give_item_with_data(player, item, count, "", None)
    }
    fn give_item_with_data(
        &mut self,
        player: &str,
        item: &str,
        count: u32,
        mod_id: &str,
        data: Option<&str>,
    ) -> Result<(), String> {
        let (inv, max) = self.checked(player, item)?;
        if !mod_id.is_empty() && item == "minecraft:air" {
            return Err("empty/air stacks cannot carry components".into());
        }
        if max == 0 {
            return Err("item cannot carry a nonempty stack".into());
        }
        if data.is_some_and(|s| s.len() > 1024) {
            return Err("stack data exceeds 1024 UTF-8 bytes".into());
        }
        if count > max.saturating_mul(100) {
            return Err("give-item is at most 100 stacks per call".into());
        }
        if count == 0 {
            return Ok(());
        }
        let mut stack = ItemStack::new(item, count);
        if !mod_id.is_empty() {
            stack.set_mod_data(mod_id, data)?;
        }
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
    #[test]
    fn giving_data_splits_legal_stacks_merges_only_equal_data_and_keeps_creative_real() {
        let mut all = inventories();
        let inv = all.players.get_mut(ALICE).unwrap();
        inv.main = vec![None; 36];
        inv.selected = 0;
        inv.offhand = None;
        inv.creative = true;
        inv.main[0] = Some(ItemStack::new("minecraft:beef", 60));
        all.give_item_with_data(ALICE, "minecraft:beef", 70, "demo", Some("one"))
            .unwrap();
        let inv = &all.players[ALICE];
        assert_eq!(inv.main[0].as_ref().unwrap().count, 60);
        assert_eq!(inv.main[1].as_ref().unwrap().count, 64);
        assert_eq!(inv.main[2].as_ref().unwrap().count, 6);
        assert_eq!(
            inv.main[1]
                .as_ref()
                .unwrap()
                .mod_data("demo")
                .unwrap()
                .as_deref(),
            Some("one")
        );
        all.give_item_with_data(ALICE, "minecraft:beef", 3, "demo", Some("one"))
            .unwrap();
        assert_eq!(all.players[ALICE].main[2].as_ref().unwrap().count, 9);
        all.give_item_with_data(ALICE, "minecraft:beef", 2, "demo", Some("two"))
            .unwrap();
        assert_eq!(all.players[ALICE].main[3].as_ref().unwrap().count, 2);
        all.give_item_with_data(ALICE, "minecraft:beef", 2, "demo", None)
            .unwrap();
        assert_eq!(all.players[ALICE].main[0].as_ref().unwrap().count, 62);
        let saved = all.players[ALICE].main.clone();
        assert!(
            all.give_item_with_data(ALICE, "minecraft:beef", 6401, "demo", Some("one"))
                .is_err()
        );
        assert!(
            all.give_item_with_data(ALICE, "minecraft:beef", 1, "demo", Some(&"x".repeat(1025)))
                .is_err()
        );
        assert_eq!(all.players[ALICE].main, saved);
        all.give_item_with_data(ALICE, "ruby:scale", 100, "demo", Some("opaque"))
            .unwrap();
        let inv = &all.players[ALICE];
        assert_eq!(inv.drops.len(), 68);
        assert!(
            inv.drops
                .iter()
                .all(|s| s.count == 1 && s.mod_data("demo").unwrap().as_deref() == Some("opaque"))
        );
        assert_eq!(
            inv.main.iter().flatten().map(|s| s.count).sum::<u32>()
                + inv.drops.iter().map(|s| s.count).sum::<u32>(),
            237
        );
    }
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
    fn equipment_reads_guest_armor_selected_hand_offhand_and_nine_slots() {
        let mut inv = inventories();
        let a = inv.players.get_mut(ALICE).unwrap();
        for (i, id) in ["ruby:helmet", "ruby:chest", "ruby:legs", "ruby:feet"]
            .iter()
            .enumerate()
        {
            a.armor[i] = Some(ItemStack::new(id, 1));
        }
        a.main[5] = Some(ItemStack::new("minecraft:beef", 2));
        a.main[8] = Some(ItemStack::new("ruby:scale", 1));
        a.offhand = Some(ItemStack::new("minecraft:stone", 1));
        let gear = inv.equipment(ALICE).unwrap();
        assert_eq!(gear.head.as_deref(), Some("ruby:helmet"));
        assert_eq!(gear.chest.as_deref(), Some("ruby:chest"));
        assert_eq!(gear.legs.as_deref(), Some("ruby:legs"));
        assert_eq!(gear.feet.as_deref(), Some("ruby:feet"));
        assert_eq!(gear.main_hand.as_deref(), Some("minecraft:beef"));
        assert_eq!(gear.off_hand.as_deref(), Some("minecraft:stone"));
        assert_eq!(gear.hotbar.len(), 9);
        assert_eq!(gear.hotbar[8].as_deref(), Some("ruby:scale"));
        assert!(gear.hotbar[0].is_none());
        inv.take_item(ALICE, "minecraft:beef", 2).unwrap();
        assert!(inv.equipment(ALICE).unwrap().main_hand.is_none());
        assert!(inv.equipment("offline").is_none());
        assert!(
            inv.equipment(BOB)
                .unwrap()
                .hotbar
                .iter()
                .all(Option::is_none)
        );
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
