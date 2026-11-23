//! Item stacks in NBT (`ItemStack.CODEC`): id, count and components, alone or in
//! slot lists.
use horizon_client_core::nbt_value::{number, string, text};
use horizon_world::nbt::Tag;
use std::sync::Arc;

pub fn encode_item(stack: &crate::items::ItemStack) -> Vec<(&'static str, Tag)> {
    let mut entries = vec![
        ("id", string(&stack.item)),
        ("count", Tag::Int(stack.count as i32)),
    ];
    if let Some(components) = crate::item_data::encode_components(stack) {
        entries.push(("components", components));
    }
    entries
}
/// ItemStack.CODEC (id, count, components).
pub fn item_to_nbt(stack: &crate::items::ItemStack) -> Tag {
    Tag::compound(encode_item(stack))
}
/// ItemStack.CODEC's count range is independent of the item's stack limit;
/// an out-of-range count uses the field's default of one. The id is read as an
/// `Identifier`: `stone` is `minecraft:stone` (a summoned display's or a
/// command's NBT often leaves the namespace out), and an id that is no valid
/// identifier makes no stack.
pub fn item_from_nbt(tag: &Tag) -> Option<crate::items::ItemStack> {
    let (namespace, path) = horizon_world::datafix::identifier(&text(tag.get("id"))?)?;
    let id = format!("{namespace}:{path}");
    if id == "minecraft:air" {
        return None;
    }
    let count = number(tag.get("count"))
        .or_else(|| number(tag.get("Count")))
        .filter(|count| (1..=99).contains(count))
        .unwrap_or(1) as u32;
    let (damage, extra) = crate::item_data::decode_components(tag.get("components"));
    Some(crate::items::ItemStack {
        item: Arc::from(id.as_str()),
        count,
        damage,
        extra,
    })
}
pub fn encode_items(slots: &[Option<crate::items::ItemStack>]) -> Tag {
    Tag::List(
        slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let mut entries = vec![("Slot", Tag::Byte(i as i8))];
                entries.extend(encode_item(s.as_ref()?));
                Some(Tag::compound(entries))
            })
            .collect(),
    )
}
pub fn decode_items(tag: Option<&Tag>, size: usize) -> Vec<Option<crate::items::ItemStack>> {
    let mut slots = vec![None; size];
    if let Some(Tag::List(items)) = tag {
        for it in items {
            let slot = number(it.get("Slot")).unwrap_or(0) as u8 as usize;
            if let Some(cell) = slots.get_mut(slot) {
                *cell = item_from_nbt(it);
            }
        }
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mod_data_survives_binary_nbt_slot_lists_for_containers_and_players() {
        use crate::items::ItemStack;
        use horizon_world::nbt::{Limits, read_network, write_network};
        // Both block-entity Items and player Inventory use this slot-list codec.
        for size in [27, 36] {
            let mut stack = ItemStack::new("minecraft:stick", 12);
            stack
                .set_mod_data("first", Some("{\"counter\":42,\"text\":\"é🦀\"}"))
                .unwrap();
            stack.set_mod_data("other", Some("opaque")).unwrap();
            let mut slots = vec![None; size];
            slots[size - 1] = Some(stack.clone());
            let bytes = write_network(&encode_items(&slots), Limits::default()).unwrap();
            let (tag, consumed) = read_network(&bytes, Limits::default()).unwrap();
            assert_eq!(consumed, bytes.len());
            let loaded = decode_items(Some(&tag), size);
            assert_eq!(loaded, slots);
            assert_eq!(
                loaded[size - 1].as_ref().unwrap().mod_data("first"),
                stack.mod_data("first")
            );
            assert_eq!(
                loaded[size - 1]
                    .as_ref()
                    .unwrap()
                    .mod_data("other")
                    .unwrap()
                    .as_deref(),
                Some("opaque")
            );
        }
    }

    #[test]
    fn mod_data_is_scoped_bounded_and_removal_restores_component_equality() {
        use crate::items::ItemStack;
        let plain = ItemStack::new("minecraft:stick", 1);
        let mut stack = plain.clone();
        stack.set_mod_data("other", Some("keep")).unwrap();
        stack
            .data_mut()
            .set_other("minecraft:lore", Some(Tag::List(vec![string("Keep lore")])));
        let before = stack.clone();
        stack.set_mod_data("first", Some(&"é".repeat(512))).unwrap();
        assert_eq!(stack.mod_data("first").unwrap().unwrap().len(), 1024);
        let tagged = stack.clone();
        assert!(stack.set_mod_data("first", Some(&"é".repeat(513))).is_err());
        assert_eq!(stack, tagged);
        assert_ne!(stack, before);
        stack.set_mod_data("first", None).unwrap();
        assert_eq!(stack, before);
        assert_eq!(stack.mod_data("other").unwrap().as_deref(), Some("keep"));
        let mut only = plain.clone();
        only.set_mod_data("first", Some("")).unwrap();
        assert_eq!(only.mod_data("first").unwrap(), Some(String::new()));
        only.set_mod_data("first", None).unwrap();
        assert_eq!(only, plain);
        assert!(only.extra.is_none());
        for mut empty in [
            ItemStack::new("minecraft:air", 1),
            ItemStack::new("minecraft:stick", 0),
        ] {
            assert!(empty.mod_data("first").is_err());
            assert!(empty.set_mod_data("first", Some("x")).is_err());
        }
        let mut malformed = plain;
        malformed
            .data_mut()
            .set_other("minecraft:custom_data", Some(Tag::Int(7)));
        let before = malformed.clone();
        assert!(malformed.mod_data("first").is_err());
        assert!(malformed.set_mod_data("first", None).is_err());
        assert_eq!(malformed, before);
    }

    #[test]
    fn nbt_count_defaults_to_one_outside_codec_range() {
        for item in ["minecraft:stone", "minecraft:diamond_sword"] {
            for (raw, expected) in [
                (None, 1),
                (Some(Tag::Int(-1)), 1),
                (Some(Tag::Int(0)), 1),
                (Some(Tag::Int(1)), 1),
                (Some(Tag::Int(2)), 2),
                (Some(Tag::Int(64)), 64),
                (Some(Tag::Int(99)), 99),
                (Some(Tag::Int(100)), 1),
                (Some(Tag::Int(255)), 1),
                (Some(Tag::Int(256)), 1),
                (Some(Tag::Int(i32::MAX)), 1),
                (Some(Tag::Byte(1)), 1),
                (Some(Tag::Float(1.5_f32.to_bits())), 1),
                (Some(string("1")), 1),
            ] {
                let mut tag = Tag::compound([("id", string(item))]);
                if let (Tag::Compound(map), Some(raw)) = (&mut tag, &raw) {
                    map.insert("count".into(), raw.clone());
                }
                let stack = item_from_nbt(&tag).unwrap();
                assert_eq!(stack.count, expected, "{item}: {raw:?}");
                assert_eq!(
                    number(item_to_nbt(&stack).get("count")),
                    Some(i64::from(expected))
                );
            }
        }
    }

    /// `/summon item_display ~ ~ ~ {item:{id:"stone"}}` shows stone in vanilla:
    /// the id reads as an Identifier, with the default namespace (#8570).
    #[test]
    fn an_id_without_a_namespace_is_a_minecraft_item() {
        let read = |id: &str| item_from_nbt(&Tag::compound([("id", string(id))]));
        assert_eq!(
            read("stone").map(|s| s.item),
            Some("minecraft:stone".into())
        );
        assert_eq!(
            read(":diamond").map(|s| s.item),
            Some("minecraft:diamond".into())
        );
        assert_eq!(read("nexo:ruby").map(|s| s.item), Some("nexo:ruby".into()));
        assert!(read("air").is_none());
        assert!(read("Stone").is_none(), "not a valid identifier");
    }

    #[test]
    fn runtime_counts_remain_independent_of_the_nbt_decode_range() {
        let stack = crate::items::ItemStack::new("minecraft:diamond_sword", 1000);
        assert_eq!(stack.count, 1000);
        assert_eq!(number(item_to_nbt(&stack).get("count")), Some(1000));
    }
}
