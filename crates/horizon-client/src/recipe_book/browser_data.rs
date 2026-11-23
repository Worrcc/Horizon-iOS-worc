//! Component facts and recipe files for recipe browsers, built with their item table.
use crate::{
    enchant::Enchantments,
    items::{ItemStack, Items},
    providers::Providers,
};
use serde_json::Value;
use std::collections::HashMap;

pub struct ItemFacts {
    pub item: u32,
    pub burn_ticks: u32,
    pub compost_chance: f32,
    pub repaired_by: Vec<u32>,
}

pub fn item_facts(
    items: &Items,
    enchantments: &Enchantments,
    providers: &Providers,
) -> Vec<ItemFacts> {
    let index: HashMap<&str, u32> = items
        .registry_order
        .iter()
        .enumerate()
        .map(|(i, id)| (&**id, i as u32))
        .collect();
    items
        .registry_order
        .iter()
        .enumerate()
        .filter_map(|(item, id)| {
            let info = items.get(id)?;
            let burn_ticks = providers
                .burn_time(items, &ItemStack::new(id, 1), false)
                .max(0) as u32;
            let compost_chance = info
                .compostable
                .as_ref()
                .map_or(0.0, |v| providers.compost_chance(v));
            let mut repaired_by = Vec::new();
            for spec in enchantments
                .props(id)
                .into_iter()
                .flat_map(|p| &p.repairable)
            {
                if let Some(tag) = spec.strip_prefix('#') {
                    repaired_by.extend(
                        items
                            .item_tags
                            .get(tag)
                            .into_iter()
                            .flatten()
                            .filter_map(|id| index.get(id.as_str()).copied()),
                    );
                } else if let Some(&i) = index.get(spec.as_str()) {
                    repaired_by.push(i);
                }
            }
            repaired_by.sort_unstable();
            repaired_by.dedup();
            (burn_ticks > 0 || compost_chance > 0.0 || !repaired_by.is_empty()).then_some(
                ItemFacts {
                    item: item as u32,
                    burn_ticks,
                    compost_chance,
                    repaired_by,
                },
            )
        })
        .collect()
}

pub struct OtherRecipe {
    pub id: String,
    pub kind: String,
    pub json: String,
}

/// Whether the typed recipe API cannot express this recipe type.
pub fn other_recipe_type(kind: &str) -> bool {
    !kind.starts_with("minecraft:crafting_special_")
        && !matches!(
            kind,
            "minecraft:crafting_shaped"
                | "minecraft:crafting_shapeless"
                | "minecraft:crafting_transmute"
                | "minecraft:crafting_dye"
                | "minecraft:crafting_imbue"
                | "minecraft:smelting"
                | "minecraft:blasting"
                | "minecraft:smoking"
                | "minecraft:campfire_cooking"
                | "minecraft:stonecutting"
                | "minecraft:smithing_transform"
                | "minecraft:smithing_trim"
                | "minecraft:brewing"
                | "minecraft:crafting_decorated_pot"
        )
}

/// Recipe content received from a Horizon server (the wire representation is parsed JSON).
pub fn other_recipes(sources: Vec<(String, Value)>) -> Vec<OtherRecipe> {
    other_recipe_files(sources.into_iter().map(|(id, v)| {
        let text = v.to_string();
        (id, v, text)
    }))
}

/// Same effective files as the recipe tables, after pack overrides, conditions and filters.
pub fn other_recipe_files(
    sources: impl IntoIterator<Item = (String, Value, String)>,
) -> Vec<OtherRecipe> {
    sources
        .into_iter()
        .filter_map(|(id, v, json)| {
            let kind = v["type"].as_str()?;
            (other_recipe_type(kind) && json.len() <= 16 * 1024).then(|| OtherRecipe {
                id: crate::crafting::qualified(&id),
                kind: kind.to_owned(),
                json,
            })
        })
        .take(4096)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crafting::Recipes;
    use horizon_world::loot::PackOverlay;
    use serde_json::json;
    use std::fs;

    #[test]
    fn composter_probability_uses_weights_constants_and_level_one() {
        let root = std::env::temp_dir();
        let providers = Providers::load(&root.join("horizon-no-provider-root"));
        let weighted = json!({"type":"minecraft:weighted_list", "distribution":[
            {"data":1,"weight":30}, {"data":0,"weight":70}
        ]});
        assert_eq!(providers.compost_chance(&weighted), 0.3);
        assert_eq!(providers.compost_chance(&json!(1)), 1.0);
        assert_eq!(
            providers.compost_chance(&json!({"type":"minecraft:constant","value":1})),
            1.0
        );
        assert_eq!(providers.compost_chance(&json!(0)), 0.0);
        assert_eq!(providers.compost_chance(&json!({
            "type":"minecraft:number_dispatcher", "cases":[{
                "condition":{"type":"minecraft:match_block","blocks":"minecraft:composter","state":{"level":"0"}},
                "value":1
            }], "default":weighted
        })), 0.3);
        assert_eq!(
            providers.compost_chance(&json!({"type":"minecraft:weighted_list","distribution":[]})),
            0.0
        );
    }

    #[test]
    fn furnace_and_repair_facts_use_components_and_content_tags() {
        let root =
            std::env::temp_dir().join(format!("horizon-browser-facts-{}", std::process::id()));
        let write = |name: &str, v: Value| {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, v.to_string()).unwrap();
        };
        write(
            horizon_items::content_namespaces::MARKER,
            json!(["example"]),
        );
        write(
            "data/c/tags/item/ingots/silver.json",
            json!({"values":["example:silver"]}),
        );
        write(
            "data/example/context_int_provider/fuel.json",
            json!({"type":"minecraft:conditional",
            "condition":"minecraft:block/fast_cooking", "on_true":800, "on_false":1600}),
        );
        write(
            "data/example/context_int_provider/layers.json",
            json!({"type":"minecraft:weighted_list",
            "distribution":[{"data":1,"weight":65},{"data":0,"weight":35}]}),
        );
        write(
            "items.json",
            json!({
                "minecraft:coal":{"minecraft:cooking_fuel":{"burn_time":"example:fuel","speed_multiplier":1}},
                "example:leaf":{"minecraft:compostable":{"layers":"example:layers"}},
                "example:tool":{"minecraft:repairable":{"items":["#c:ingots/silver","example:silver","missing:item"]}},
                "example:direct":{"minecraft:repairable":{"items":"example:silver"}},
                "example:silver":{}, "example:none":{}
            }),
        );
        let items = Items::catalog(&root, &PackOverlay::NONE).unwrap();
        let enchantments = Enchantments::load(&root);
        let providers = Providers::load(&root);
        let facts = item_facts(&items, &enchantments, &providers);
        let index = |id: &str| {
            items
                .registry_order
                .iter()
                .position(|s| &**s == id)
                .unwrap() as u32
        };
        let fact = |id: &str| facts.iter().find(|f| f.item == index(id)).unwrap();
        assert_eq!(facts.len(), 4);
        assert_eq!(fact("minecraft:coal").burn_ticks, 1600);
        assert_eq!(
            providers.burn_time(&items, &ItemStack::new("minecraft:coal", 1), true),
            800
        );
        assert_eq!(fact("example:leaf").compost_chance, 0.65);
        assert_eq!(fact("example:tool").repaired_by, [index("example:silver")]);
        assert_eq!(
            fact("example:direct").repaired_by,
            [index("example:silver")]
        );
        assert_eq!(fact("example:tool").burn_ticks, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn other_files_follow_namespaces_packs_conditions_and_limits() {
        let root =
            std::env::temp_dir().join(format!("horizon-browser-files-{}", std::process::id()));
        fs::create_dir_all(root.join("data/minecraft/recipe")).unwrap();
        fs::create_dir_all(root.join("data/example/recipe/sub")).unwrap();
        fs::write(
            root.join(horizon_items::content_namespaces::MARKER),
            r#"["example"]"#,
        )
        .unwrap();
        let original = "{ \"type\": \"iceandfire:dragonforge\", \"heat\": 9 }\n";
        fs::write(root.join("data/example/recipe/sub/forge.json"), original).unwrap();
        fs::write(
            root.join("data/minecraft/recipe/overridden.json"),
            r#"{"type":"minecraft:smelting"}"#,
        )
        .unwrap();
        fs::write(
            root.join("data/example/recipe/absent.json"),
            r#"{"type":"example:no","neoforge:conditions":[{"type":"neoforge:false"}]}"#,
        )
        .unwrap();
        fs::write(
            root.join("data/example/recipe/huge.json"),
            format!("{{\"type\":\"example:huge\"}}{}", " ".repeat(16 * 1024)),
        )
        .unwrap();
        let packs = [(
            "recipe/minecraft:overridden".into(),
            vec![br#"{"type":"example:pack"}"#.to_vec()],
        )];
        let recipes = other_recipe_files(
            Recipes::source_files(&root, &PackOverlay { files: &packs }).unwrap(),
        );
        assert_eq!(recipes.len(), 2);
        let forge = recipes
            .iter()
            .find(|r| r.id == "example:sub/forge")
            .unwrap();
        assert_eq!(forge.kind, "iceandfire:dragonforge");
        assert_eq!(forge.json, original);
        assert!(
            recipes
                .iter()
                .any(|r| r.id == "minecraft:overridden" && r.kind == "example:pack")
        );
        for kind in [
            "crafting_shaped",
            "crafting_shapeless",
            "crafting_transmute",
            "crafting_dye",
            "crafting_imbue",
            "smelting",
            "blasting",
            "smoking",
            "campfire_cooking",
            "stonecutting",
            "smithing_transform",
            "smithing_trim",
            "brewing",
            "crafting_special_repairitem",
            "crafting_decorated_pot",
        ] {
            assert!(!other_recipe_type(&format!("minecraft:{kind}")), "{kind}");
        }
        assert!(other_recipe_type("minecraft:future_recipe"));
        assert_eq!(
            other_recipes(
                (0..4098)
                    .map(|i| (format!("example:{i}"), json!({"type":"example:custom"})))
                    .collect()
            )
            .len(),
            4096
        );
        fs::remove_dir_all(root).unwrap();
    }
}
