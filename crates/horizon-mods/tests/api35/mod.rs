//! API 35 exercised across the real SDK guest/component boundary.
use super::*;
use horizon_mods::{
    BarKind, BlockTable, ChunkColumns, ClientDimension, ClientTexture, ClientWorldView,
    ColumnLayer, EntityCategory, EntityDot, FoodState, HudBar, RenderRect,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct FoodView(Arc<AtomicUsize>);
impl ClientWorldView for FoodView {
    fn dimension(&self) -> Option<ClientDimension> {
        None
    }
    fn block_table(&mut self) -> BlockTable {
        BlockTable {
            version: 0,
            blocks: vec![],
            sprites: vec![],
            state_block: vec![],
            state_flags: vec![],
            state_top: vec![],
            state_side: vec![],
            constant_tint: vec![],
            map_color: vec![],
        }
    }
    fn texture(&mut self, _: &str) -> Option<ClientTexture> {
        None
    }
    fn columns(&mut self, _: (i32, i32), _: ColumnLayer, _: u8) -> Option<ChunkColumns> {
        None
    }
    fn changes(&mut self, _: u64) -> (u64, Vec<(i32, i32)>) {
        (0, vec![])
    }
    fn light(&self, _: i32, _: i32, _: i32) -> Option<u8> {
        None
    }
    fn biome(&self, _: i32, _: i32, _: i32) -> Option<String> {
        None
    }
    fn food_value(&self, item: &str) -> Result<Option<(u32, f32, bool)>, String> {
        self.0.fetch_add(1, Ordering::Relaxed);
        horizon_mods::client_queries::food_value(
            match item {
                "minecraft:apple" => Some((4, 2.4)),
                "minecraft:golden_apple" => Some((4, 9.6)),
                _ => None,
            },
            item == "minecraft:golden_apple",
        )
    }
}
fn snapshot() -> Snapshot {
    let mut s = player();
    let p = s.player.as_mut().unwrap();
    p.x = 0.0;
    p.y = 0.0;
    p.z = 0.0;
    s.client_entity_kinds = Arc::from(["minecraft:cow".into(), "minecraft:zombie".into()]);
    s.food_stats = Some(FoodState {
        food: 18,
        saturation: 3.5,
        exhaustion: None,
        health: 16.0,
        max_health: 40.0,
        absorption: 4.0,
    });
    s.hud_bars = Some(vec![HudBar {
        kind: BarKind::Food,
        bounds: RenderRect {
            x: 170.0,
            y: 201.0,
            w: 81.0,
            h: 9.0,
        },
    }]);
    s
}
#[test]
fn api35_entities_validate_numeric_inputs_caps_categories_and_frame_budget() {
    let mut mods = load("hud_demo.hmod");
    let mut s = snapshot();
    s.client_entities = (0..300)
        .map(|i| EntityDot {
            x: i as f64 / 10.0,
            y: 0.0,
            z: 0.0,
            yaw: 90.0,
            kind: 0,
            category: EntityCategory::Passive,
        })
        .collect();
    mods.set_snapshot(s.clone());
    for (args, error) in [
        ("-1 1", "radius"),
        ("NaN 1", "radius"),
        ("inf 1", "radius"),
        ("65 1", "radius"),
        ("64 257", "max"),
    ] {
        let (chat, errors) = said(&mut mods, &format!("hud35 entities {args}"));
        assert!(
            chat.is_empty() && errors.len() == 1 && errors[0].contains(error),
            "{args}: {errors:?}"
        );
    }
    let (chat, errors) = said(&mut mods, "hud35 entities 64 256");
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        chat[0].starts_with("entities 256 first 0 last 25.5 "),
        "{chat:?}"
    );
    let (chat, errors) = said(&mut mods, "hud35 entities 1 256");
    assert!(errors.is_empty(), "{errors:?}");
    assert!(
        chat[0].starts_with("entities 11 first 0 last 1 "),
        "{chat:?}"
    );
    assert_eq!(
        said(&mut mods, "hud35 entities 0 0").0,
        ["entities 0 first 0 last 0 categories "]
    );
    assert!(said(&mut mods, "hud35 entities 1 1").1.is_empty());
    // New tick/snapshot and commands cannot reset the per-frame budget.
    s.tick += 1;
    mods.set_snapshot(s);
    mods.tick(1);
    assert!(said(&mut mods, "hud35 entities 1 1").1[0].contains("4 calls per frame"));
    mods.frame(0.016);
    assert!(said(&mut mods, "hud35 entities 1 1").1.is_empty());
    mods.frame(0.016);
    assert!(said(&mut mods, "hud35 budget").1[0].contains("4 calls per frame"));
    mods.frame(0.016);
    let mut s = snapshot();
    s.client_entities = [
        EntityCategory::Hostile,
        EntityCategory::Passive,
        EntityCategory::Neutral,
        EntityCategory::Water,
        EntityCategory::Ambient,
        EntityCategory::Player,
        EntityCategory::OwnTamed,
        EntityCategory::OtherTamed,
        EntityCategory::Other,
    ]
    .into_iter()
    .map(|category| EntityDot {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        yaw: 0.0,
        kind: 1,
        category,
    })
    .collect();
    mods.set_snapshot(s);
    let (chat, errors) = said(&mut mods, "hud35 entities 64 256");
    assert!(errors.is_empty());
    for name in [
        "Hostile",
        "Passive",
        "Neutral",
        "Water",
        "Ambient",
        "Player",
        "OwnTamed",
        "OtherTamed",
        "Other",
    ] {
        assert!(chat[0].contains(name), "{name}");
    }
    assert_eq!(
        said(&mut mods, "hud35 kinds").0,
        ["[\"minecraft:cow\", \"minecraft:zombie\"]"]
    );
    assert_eq!(mods.entries()[0].status, Status::Running);
}
#[test]
fn api35_food_value_validates_ids_and_preserves_component_points() {
    let mut mods = load("hud_demo.hmod");
    let calls = Arc::new(AtomicUsize::new(0));
    mods.lend_client_world(Box::new(FoodView(calls.clone())));
    for id in [
        "apple",
        "Minecraft:apple",
        "minecraft:../apple",
        "minecraft:apple/../stone",
        "minecraft:",
        &format!("minecraft:{}", "x".repeat(256)),
    ] {
        let (chat, errors) = said(&mut mods, &format!("hud35 food {id}"));
        assert!(chat.is_empty() && errors[0].contains("invalid namespaced item id"));
    }
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        said(&mut mods, "hud35 food minecraft:apple").0,
        ["Some(FoodInfo { nutrition: 4, saturation: 2.4, always-edible: false })"]
    );
    assert_eq!(
        said(&mut mods, "hud35 food minecraft:golden_apple").0,
        ["Some(FoodInfo { nutrition: 4, saturation: 9.6, always-edible: true })"]
    );
    for id in ["minecraft:stone", "missing:item"] {
        assert_eq!(said(&mut mods, &format!("hud35 food {id}")).0, ["None"]);
    }
    assert_eq!(calls.load(Ordering::Relaxed), 4);
    assert_eq!(mods.entries()[0].status, Status::Running);
}
#[test]
fn api35_food_stats_optional_exhaustion_and_unavailable_hud() {
    let mut mods = load("hud_demo.hmod");
    assert_eq!(said(&mut mods, "hud35 stats").0, ["None"]);
    assert!(said(&mut mods, "hud35 bars").1[0].contains("vanilla HUD layout unavailable"));
    let mut s = snapshot();
    mods.set_snapshot(s.clone());
    let (chat, errors) = said(&mut mods, "hud35 stats");
    assert!(errors.is_empty());
    for field in [
        "food: 18",
        "saturation: 3.5",
        "exhaustion: None",
        "health: 16.0",
        "max-health: 40.0",
        "absorption: 4.0",
    ] {
        assert!(chat[0].contains(field));
    }
    s.food_stats.as_mut().unwrap().exhaustion = Some(2.75);
    mods.set_snapshot(s.clone());
    assert!(said(&mut mods, "hud35 stats").0[0].contains("exhaustion: Some(2.75)"));
    assert!(said(&mut mods, "hud35 bars").0[0].contains("x: 170.0, y: 201.0, w: 81.0, h: 9.0"));
    s.hud_bars = Some(vec![HudBar {
        kind: BarKind::MountHealth,
        bounds: RenderRect {
            x: 170.0,
            y: 191.0,
            w: 81.0,
            h: 19.0,
        },
    }]);
    mods.set_snapshot(s.clone());
    let chat = said(&mut mods, "hud35 bars").0;
    assert!(chat[0].contains("MountHealth") && !chat[0].contains("kind: Food"));
    s.hud_bars = Some(vec![]);
    mods.set_snapshot(s);
    assert_eq!(said(&mut mods, "hud35 bars").0, ["[]"]);
}
#[test]
fn api35_official_demo_reads_all_calls_and_draws_then_clears() {
    let mut mods = load("hud_demo.hmod");
    let mut state = snapshot();
    state.held = Some(HeldItem {
        id: "minecraft:apple".into(),
        count: 1,
        slot: 0,
    });
    mods.set_snapshot(state);
    let calls = Arc::new(AtomicUsize::new(0));
    mods.lend_client_world(Box::new(FoodView(calls.clone())));
    let (chat, errors) = said(&mut mods, "hud35 on");
    assert!(errors.is_empty(), "{errors:?}");
    assert!(chat[0].contains("entities 0 kinds 2"));
    let shapes: Vec<_> = mods.hud_shapes().collect();
    assert_eq!(shapes.len(), 1);
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    let Shape::Fill(fill) = shapes[0] else {
        panic!("expected saturation fill");
    };
    assert!((fill.rect.w - 23.895).abs() < 0.001);
    assert_eq!((fill.rect.x, fill.rect.y, fill.rect.h), (170.0, 210.0, 2.0));
    assert_eq!(said(&mut mods, "hud35 off").0, ["off"]);
    assert_eq!(mods.hud_shapes().count(), 0);
    let wasm = Package::read(&mods_dir().join("hud_demo.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}
