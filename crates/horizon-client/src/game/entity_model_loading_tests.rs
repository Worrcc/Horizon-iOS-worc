//! Model-batch equivalence tests and an opt-in benchmark of the actual loaders.
use super::{armor_stands::ArmorStandAssets, boats::BoatModels, minecarts::CartModels};
use horizon_client::mobs::{EntityModelDocument, Part, load_model};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, hint::black_box, path::Path, time::Instant};

const WOODS: [&str; 11] = [
    "oak",
    "spruce",
    "birch",
    "jungle",
    "acacia",
    "dark_oak",
    "mangrove",
    "cherry",
    "bamboo",
    "pale_oak",
    "test_wood",
];
const CARTS: [&str; 7] = [
    "minecart",
    "chest_minecart",
    "hopper_minecart",
    "furnace_minecart",
    "tnt_minecart",
    "command_block_minecart",
    "spawner_minecart",
];

fn fixture() -> Value {
    let mut all = serde_json::Map::new();
    let part = json!({
        "pose": [1,2,3,0.25,-0.5,0.75,1,2,0.5],
        "polygons": [[[0,0,0,0,0],[1,0,0,1,0],[1,1,0,1,1],[0,1,0,0,1]]],
        "normals": [[0,0,1]],
        "children": {"root": {"children": {"bone": {"pose": [4,5,6]}}}, "other": {}}
    });
    for wood in WOODS {
        for prefix in ["boat", "chest_boat"] {
            all.insert(format!("minecraft:{prefix}/{wood}#main"), part.clone());
        }
    }
    for cart in CARTS {
        all.insert(format!("minecraft:{cart}#main"), part.clone());
    }
    for model in ["player", "armor_stand", "armor_stand_small"] {
        for layer in ["helmet", "chestplate", "leggings", "boots"] {
            all.insert(format!("minecraft:{model}#{layer}"), part.clone());
        }
    }
    for model in ["armor_stand", "armor_stand_small"] {
        all.insert(format!("minecraft:{model}#main"), part.clone());
    }
    // Discovery must exclude other models and non-main boat layers.
    all.insert("minecraft:boat/oak#water_patch".into(), part.clone());
    all.insert("minecraft:unrelated#main".into(), part);
    Value::Object(all)
}

struct FixtureRoot(std::path::PathBuf);
impl FixtureRoot {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let next = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "horizon-entity-models-{}-{next}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}
impl Drop for FixtureRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

// Preserve the previous production algorithm: one discovery parse, then a full
// file read/parse through load_model for each of the 42 selected layers.
fn baseline(root: &Path) -> BTreeMap<String, Part> {
    let mut models = BTreeMap::new();
    if let Ok(bytes) = fs::read(root.join("entity-models.json"))
        && let Ok(Value::Object(all)) = serde_json::from_slice::<Value>(&bytes)
    {
        for key in all.keys() {
            for (prefix, label) in [
                ("minecraft:boat/", "boat"),
                ("minecraft:chest_boat/", "chest_boat"),
            ] {
                if let Some(wood) = key
                    .strip_prefix(prefix)
                    .and_then(|k| k.strip_suffix("#main"))
                    && let Some(part) = load_model(root, key)
                {
                    models.insert(format!("{label}/{wood}"), part);
                }
            }
        }
    }
    for cart in CARTS {
        if let Some(part) = load_model(root, &format!("minecraft:{cart}#main")) {
            models.insert(format!("cart/{cart}"), part);
        }
    }
    for slot in ["helmet", "chestplate", "leggings", "boots"] {
        for model in ["player", "armor_stand", "armor_stand_small"] {
            if let Some(part) = load_model(root, &format!("minecraft:{model}#{slot}")) {
                models.insert(format!("{model}/{slot}"), part);
            }
        }
    }
    // The fixture contains only entity-models.json; armor equipment is absent.
    let _ = fs::read_dir(root.join("assets/minecraft/equipment"));
    for model in ["armor_stand", "armor_stand_small"] {
        if let Some(part) = load_model(root, &format!("minecraft:{model}#main")) {
            models.insert(format!("{model}/main"), part);
        }
    }
    models
}

fn optimized(root: &Path) -> BTreeMap<String, Part> {
    let document = EntityModelDocument::load(root).ok();
    let boats = BoatModels::load(document.as_ref());
    let carts = CartModels::load(document.as_ref());
    let armor = ArmorStandAssets::load(root, document.as_ref());
    let mut models = BTreeMap::new();
    for (label, group) in [("boat", boats.models), ("chest_boat", boats.chest_models)] {
        for (wood, part) in group {
            models.insert(format!("{label}/{wood}"), part);
        }
    }
    for (kind, part) in carts.models {
        models.insert(
            format!("cart/{}", kind.id().trim_start_matches("minecraft:")),
            part,
        );
    }
    for ((slot, small), part) in armor.layers {
        let model = if small {
            "armor_stand_small"
        } else {
            "armor_stand"
        };
        models.insert(
            format!("{model}/{slot}"),
            std::sync::Arc::unwrap_or_clone(part),
        );
    }
    for (slot, part) in armor.player {
        models.insert(format!("player/{slot}"), part);
    }
    for (model, part) in [
        ("armor_stand", armor.main),
        ("armor_stand_small", armor.small),
    ] {
        if let Some(part) = part {
            models.insert(
                format!("{model}/main"),
                std::sync::Arc::unwrap_or_clone(part),
            );
        }
    }
    models
}

fn assert_equivalent(root: &Path) {
    // Debug includes every part name, pose, polygon normal/vertex and child.
    assert_eq!(
        format!("{:?}", baseline(root)),
        format!("{:?}", optimized(root))
    );
}

#[test]
fn entity_model_batches_preserve_parts_missing_layers_and_load_failures() {
    let root = FixtureRoot::new();
    assert_equivalent(&root.0); // missing file
    let file = root.0.join("entity-models.json");
    for bytes in [b"{".as_slice(), b"null", b"[]", b"{}"] {
        fs::write(&file, bytes).unwrap();
        assert_equivalent(&root.0);
    }
    let mut all = fixture();
    fs::write(&file, serde_json::to_vec(&all).unwrap()).unwrap();
    assert_equivalent(&root.0);
    assert_eq!(optimized(&root.0).len(), 43);
    all.as_object_mut()
        .unwrap()
        .remove("minecraft:armor_stand_small#main");
    all.as_object_mut()
        .unwrap()
        .remove("minecraft:boat/oak#main");
    fs::write(&file, serde_json::to_vec(&all).unwrap()).unwrap();
    assert_equivalent(&root.0);
    assert_eq!(optimized(&root.0).len(), 41);
    let document = EntityModelDocument::load(&root.0).unwrap();
    let mut first = document.model("minecraft:armor_stand#main").unwrap();
    first.children.clear();
    assert!(
        !document
            .model("minecraft:armor_stand#main")
            .unwrap()
            .children
            .is_empty()
    );
}

#[test]
fn armor_equipment_loads_without_an_entity_model_document() {
    let root = FixtureRoot::new();
    let equipment = root.0.join("assets/minecraft/equipment");
    fs::create_dir_all(&equipment).unwrap();
    fs::write(
        equipment.join("leather.json"),
        serde_json::to_vec(&json!({"layers": {"humanoid": [
            {"texture": "minecraft:leather", "dyeable": {"color_when_undyed": 0x123456}},
            {"texture": "minecraft:leather_overlay"}
        ]}}))
        .unwrap(),
    )
    .unwrap();
    fs::write(equipment.join("invalid.json"), b"{").unwrap();
    // A content root: content mods' namespaces' equipment loads (`gear` is one).
    fs::write(
        root.0.join(horizon_items::content_namespaces::MARKER),
        br#"["gear"]"#,
    )
    .unwrap();
    let custom = root.0.join("assets/gear/equipment/metal");
    fs::create_dir_all(&custom).unwrap();
    fs::write(
        custom.join("fire.json"),
        serde_json::to_vec(&json!({"layers": {
            "humanoid": [{"texture":"gear:metal/fire"}],
            "humanoid_leggings": [{"texture":"gear:metal/fire_legs"}]
        }}))
        .unwrap(),
    )
    .unwrap();
    let assets = ArmorStandAssets::load(&root.0, None);
    assert!(assets.main.is_none());
    assert!(assets.layers.is_empty());
    assert_eq!(
        assets.equipment["minecraft:leather"]["humanoid"][0].texture,
        "entity/equipment/humanoid/leather"
    );
    assert_eq!(
        assets.equipment["minecraft:leather"]["humanoid"][0].undyed,
        Some(0x123456)
    );
    assert!(
        assets.equipment["minecraft:leather"]["humanoid"][0]
            .model
            .is_none()
    );
    assert!(!assets.equipment.contains_key("minecraft:invalid"));
    assert_eq!(
        assets.equipment["gear:metal/fire"]["humanoid"][0].texture,
        "gear:entity/equipment/humanoid/metal/fire"
    );
    assert_eq!(
        assets.equipment["gear:metal/fire"]["humanoid_leggings"][0].texture,
        "gear:entity/equipment/humanoid_leggings/metal/fire_legs"
    );
}

#[test]
fn content_mods_equipment_loads_under_its_namespace() {
    let root = FixtureRoot::new();
    let equipment = root.0.join("assets/gems/equipment");
    fs::create_dir_all(&equipment).unwrap();
    fs::write(
        equipment.join("ruby.json"),
        serde_json::to_vec(&json!({"layers": {
            "humanoid": [{"texture": "gems:ruby"}],
            "humanoid_leggings": [{"texture": "gems:ruby"}]
        }}))
        .unwrap(),
    )
    .unwrap();
    // Not a content root: other namespaces stay unread, as before.
    assert!(ArmorStandAssets::load(&root.0, None).equipment.is_empty());
    fs::write(
        root.0.join(horizon_items::content_namespaces::MARKER),
        br#"["gems"]"#,
    )
    .unwrap();
    let assets = ArmorStandAssets::load(&root.0, None);
    assert_eq!(
        assets.equipment["gems:ruby"]["humanoid_leggings"],
        vec![super::armor_stands::EquipmentLayer {
            texture: "gems:entity/equipment/humanoid_leggings/ruby".into(),
            undyed: None,
            model: None
        }]
    );
}

#[test]
#[ignore = "opt-in benchmark; requires ENTITY_MODEL_BENCH_DOCUMENT"]
fn entity_model_document_benchmark() {
    let input = std::env::var("ENTITY_MODEL_BENCH_DOCUMENT").unwrap();
    let root = FixtureRoot::new();
    let bytes = fs::read(&input).unwrap();
    fs::write(root.0.join("entity-models.json"), &bytes).unwrap();
    assert_equivalent(&root.0);
    let model_count = optimized(&root.0).len();
    let repetitions: usize = std::env::var("ENTITY_MODEL_BENCH_REPETITIONS")
        .unwrap_or_else(|_| "15".into())
        .parse()
        .unwrap();
    assert!(repetitions > 0);
    for _ in 0..3 {
        black_box(baseline(black_box(&root.0)));
        black_box(optimized(black_box(&root.0)));
    }
    let mut before = Vec::new();
    let mut after = Vec::new();
    for i in 0..repetitions {
        for optimized_first in [i % 2 == 0, i % 2 != 0] {
            let started = Instant::now();
            if optimized_first {
                black_box(optimized(black_box(&root.0)));
                after.push(started.elapsed().as_secs_f64() * 1000.0);
            } else {
                black_box(baseline(black_box(&root.0)));
                before.push(started.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
    before.sort_by(f64::total_cmp);
    after.sort_by(f64::total_cmp);
    let old = before[repetitions / 2];
    let new = after[repetitions / 2];
    println!(
        "{}",
        json!({
            "document_bytes": bytes.len(), "selected_models": model_count,
            "warmup_pairs": 3, "repetitions": repetitions,
            "baseline_ms": before, "optimized_ms": after,
            "baseline_median_ms": old, "optimized_median_ms": new, "speedup": old / new
        })
    );
}

#[test]
fn armor_geo_equipment_caches_geometry_and_reloads_layers() {
    let root = FixtureRoot::new();
    let geo = root.0.join("assets/armor_fixture/geo/armor");
    fs::create_dir_all(&geo).unwrap();
    fs::write(
        geo.join("horn.geo.json"),
        include_str!("../../tests/fixtures/armor-geo/assets/armor_fixture/geo/armor/horn.geo.json"),
    )
    .unwrap();
    fs::write(
        root.0.join(horizon_items::content_namespaces::MARKER),
        br#"["armor_fixture"]"#,
    )
    .unwrap();
    let dir = root.0.join("assets/armor_fixture/equipment");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join("horn.json");
    let equipment = json!({"layers":{"humanoid":[
        {"texture":"armor_fixture:horn","model":"armor_fixture:armor/horn"},
        {"texture":"armor_fixture:overlay","model":"armor_fixture:armor/horn"}
    ]}});
    fs::write(&file, equipment.to_string()).unwrap();
    let mut assets = ArmorStandAssets::load(&root.0, None);
    assert_eq!(assets.models.len(), 1);
    assert_eq!(assets.equipment["armor_fixture:horn"]["humanoid"].len(), 2);
    let helmet = &assets.models["armor_fixture:armor/horn"]["helmet"];
    assert_eq!(helmet.children[0].children[0].name, "horn");
    // Geometry works even without vanilla equipment models, and is refreshed on reload.
    assert!(assets.player.is_empty());
    fs::remove_file(&file).unwrap();
    assets.reload_trims(&root.0);
    assert!(assets.models.is_empty());
    assert!(assets.equipment.is_empty());
}
