//! Content declarations become the same components the vanilla loaders consume.
use super::{Item, J, NewItem};
use serde_json::json;

/// Armour and tools start from the matching vanilla class, even when a
/// generator previously emitted a stick placeholder. Its other components
/// (weapon wear, block transformers, sword-specific mining rules) stay intact.
pub(super) fn template(item: &Item) -> String {
    if let Some(a) = &item.armor {
        let suffix = match a.slot.as_str() {
            "head" => "helmet",
            "chest" => "chestplate",
            "legs" => "leggings",
            _ => "boots", // The declaration reader validates the slot.
        };
        if item.template.ends_with(&format!("_{suffix}")) {
            item.template.clone()
        } else {
            format!("minecraft:iron_{suffix}")
        }
    } else if let Some(t) = &item.tool {
        if item.template.ends_with(&format!("_{}", t.kind)) {
            item.template.clone()
        } else {
            format!("minecraft:iron_{}", t.kind)
        }
    } else {
        item.template.clone()
    }
}

impl J {
    fn set_json(&mut self, key: &str, value: serde_json::Value) {
        self.set(key, serde_json::from_value(value).expect("JSON component"));
    }
    fn remove(&mut self, key: &str) {
        if let J::Obj(o) = self {
            o.retain(|(k, _)| k != key);
        }
    }
}

fn attribute(kind: &str, id: &str, amount: f64, slot: &str) -> serde_json::Value {
    json!({"type": format!("minecraft:{kind}"), "id": id,
           "amount": amount, "operation": "add_value", "slot": slot})
}

impl NewItem<'_> {
    pub(super) fn components(&self, v: &mut J) {
        let Some(item) = self.decl else { return };
        let p = &item.properties;
        // Behaviour is metadata for Horizon, independent of the model and id.
        // Keep the real stack id through use, wear, projectile pickup and saves.
        if matches!(
            self.rename.from.as_str(),
            "minecraft:bow" | "minecraft:crossbow" | "minecraft:trident"
        ) {
            v.set_json("horizon:template", json!(self.rename.from));
        }
        let mut durability = None;
        if let Some(a) = &item.armor {
            durability = Some(a.durability);
            let suffix = match a.slot.as_str() {
                "head" => "helmet",
                "chest" => "chestplate",
                "legs" => "leggings",
                _ => "boots",
            };
            let id = format!("minecraft:armor.{suffix}");
            v.set_json(
                "minecraft:attribute_modifiers",
                json!([
                    attribute("armor", &id, a.defense, &a.slot),
                    attribute("armor_toughness", &id, a.toughness, &a.slot),
                    attribute("knockback_resistance", &id, a.knockback_resistance, &a.slot)
                ]),
            );
            // Retain the class's equipment sounds and default equip flags.
            if let Some(equip) = v.get_mut("minecraft:equippable") {
                equip.set_json("slot", json!(a.slot));
                // The asset's ordered layers (including optional geometry models)
                // are validated by Content and copied intact into the resource root.
                if let Some(texture) = &a.texture {
                    equip.set_json("asset_id", json!(texture));
                }
            }
        }
        if let Some(t) = &item.tool {
            durability = Some(t.durability);
            v.set_json(
                "minecraft:attribute_modifiers",
                json!([
                    attribute(
                        "attack_damage",
                        "minecraft:base_attack_damage",
                        t.attack_damage,
                        "mainhand"
                    ),
                    attribute(
                        "attack_speed",
                        "minecraft:base_attack_speed",
                        t.attack_speed,
                        "mainhand"
                    )
                ]),
            );
            // ToolMaterial.applyToolProperties: incorrect tier rule precedes
            // the mineable rule. Swords retain their fixed cobweb/efficient rules.
            if t.kind != "sword" {
                v.set_json(
                    "minecraft:tool",
                    json!({"rules": [
                    {"blocks": format!("#{}", t.incorrect_blocks_for_drops.trim_start_matches('#')),
                     "correct_for_drops": false},
                    {"blocks": format!("#minecraft:mineable/{}", t.kind),
                     "speed": t.mining_speed, "correct_for_drops": true}
                ], "default_mining_speed": 1.0, "damage_per_block": 1}),
                );
            }
            if t.enchantability == 0 {
                v.remove("minecraft:enchantable");
            } else {
                v.set_json("minecraft:enchantable", json!({"value": t.enchantability}));
            }
            v.set_json("minecraft:repairable", json!({"items": t.repair_item}));
        }
        if let Some(n) = p.max_damage.or(durability) {
            v.set_json("minecraft:max_damage", json!(n));
            v.set_json("minecraft:damage", json!(0));
        }
        // Vanilla damageable items cannot stack, including inherited durability.
        if v.get("minecraft:max_damage").is_some() {
            v.set_json("minecraft:max_stack_size", json!(1));
        }
        if let Some(n) = p.enchantability {
            if n == 0 {
                v.remove("minecraft:enchantable");
            } else {
                v.set_json("minecraft:enchantable", json!({"value": n}));
            }
        }
        if let Some(id) = &p.repair_item {
            v.set_json("minecraft:repairable", json!({"items": id}));
        }
        if let Some(fire) = p.fire_resistant {
            if fire {
                v.set_json(
                    "minecraft:damage_resistant",
                    json!({"types": "#minecraft:is_fire"}),
                );
            } else {
                v.remove("minecraft:damage_resistant");
            }
        }
        // Useful for a ranged weapon with different melee stats.
        for (kind, amount) in [
            ("attack_damage", p.attack_damage),
            ("attack_speed", p.attack_speed),
        ] {
            if let Some(amount) = amount {
                let key = "minecraft:attribute_modifiers";
                if v.get(key).is_none() {
                    v.set_json(key, json!([]));
                }
                if let Some(J::Arr(modifiers)) = v.get_mut(key) {
                    let id = format!("minecraft:base_{kind}");
                    modifiers.retain(|m| m.get("id") != Some(&J::Str(id.clone())));
                    modifiers.push(
                        serde_json::from_value(attribute(kind, &id, amount, "mainhand"))
                            .expect("attribute"),
                    );
                }
            }
        }
    }
}
