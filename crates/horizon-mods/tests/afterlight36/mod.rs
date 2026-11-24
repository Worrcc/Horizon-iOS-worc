//! API 36: server recipe queries (`world.recipes-for-item`).
use super::*;
use horizon_mods::{
    GameMode, Recipe, RecipeItem, RecipeKind, RecipeStack, Recipes, ServerPlayer, WorldView,
};
use std::sync::Arc;

struct View;
impl WorldView for View {
    fn block(&self, dim: &str, x: i32, y: i32, z: i32) -> Option<String> {
        (dim == "minecraft:overworld" && x == 0 && y == 64 && z == 0)
            .then(|| "minecraft:oak_planks".into())
    }
    fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
        None
    }
    fn has_dimension(&self, dim: &str) -> bool {
        dim == "minecraft:overworld"
    }
    fn check_state(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
}

fn stack(item: u32, count: u32) -> RecipeStack {
    RecipeStack {
        item,
        count,
        components: String::new(),
    }
}

fn recipes() -> Recipes {
    Recipes {
        items: vec![
            RecipeItem {
                id: "minecraft:oak_log".into(),
                name: "Oak Log".into(),
                tags: vec![],
            },
            RecipeItem {
                id: "minecraft:oak_planks".into(),
                name: "Oak Planks".into(),
                tags: vec!["minecraft:planks".into()],
            },
            RecipeItem {
                id: "minecraft:stick".into(),
                name: "Stick".into(),
                tags: vec![],
            },
        ],
        recipes: vec![
            Recipe {
                id: "minecraft:oak_planks".into(),
                kind: RecipeKind::CraftingShaped,
                category: String::new(),
                group: String::new(),
                width: 1,
                height: 1,
                slots: vec![vec![stack(0, 1)]],
                results: vec![stack(1, 4)],
                cooking_time: 0,
                experience: 0.0,
                book_id: -1,
            },
            Recipe {
                id: "minecraft:stick".into(),
                kind: RecipeKind::CraftingShaped,
                category: String::new(),
                group: String::new(),
                width: 1,
                height: 2,
                slots: vec![vec![stack(2, 1)], vec![stack(2, 1)]],
                results: vec![stack(2, 4)],
                cooking_time: 0,
                experience: 0.0,
                book_id: -1,
            },
        ],
        ..Default::default()
    }
}

fn setup() -> (Mods, ServerPlayer) {
    let mut mods = load("horizon_mods_test_api20_server_guest.hmod");
    let p = ServerPlayer {
        id: "alice".into(),
        name: "Alice".into(),
        x: 0.,
        y: 64.,
        z: 0.,
        dimension: "minecraft:overworld".into(),
        game_mode: GameMode::Creative,
        health: 20.,
    };
    mods.set_snapshot(Snapshot {
        tick: 1,
        players: vec![p.clone()],
        recipes: Some(Arc::new(recipes())),
        ..Default::default()
    });
    mods.lend_world(Box::new(View));
    mods.drain();
    (mods, p)
}

fn run(mods: &mut Mods, p: &ServerPlayer, cmd: &str) -> Vec<String> {
    mods.drain();
    assert!(mods.server_command(p, cmd), "{cmd}");
    mods.drain().into_iter()
        .filter_map(|o| match o {
            Output::Tell { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

#[test]
fn api36_recipes_for_item_pages_and_resolves_ids() {
    let (mut m, p) = setup();
    let lines = run(&mut m, &p, "recipes36 minecraft:oak_planks");
    assert_eq!(
        lines,
        ["recipes minecraft:oak_planks [minecraft:oak_planks:minecraft:oak_planks:4]"]
    );
    assert_eq!(
        run(&mut m, &p, "recipes36 minecraft:oak_planks 1 10"),
        ["recipes minecraft:oak_planks []"]
    );
    assert_eq!(
        run(&mut m, &p, "recipes36 minecraft:stick"),
        ["recipes minecraft:stick [minecraft:stick:minecraft:stick:4]"]
    );
    let mut errors = Vec::new();
    m.drain();
    assert!(m.server_command(&p, "recipes36 minecraft:stone"));
    for o in m.drain() {
        if let Output::Reply { text, error: true, .. } = o {
            errors.push(text);
        }
    }
    assert!(errors.iter().any(|e| e.contains("unknown item")), "{errors:?}");
}

#[test]
fn api36_recipe_query_is_level_36_and_needs_world() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "world.recipes-for-item"));
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        36
    );
    let manifest = horizon_mods::Manifest::parse(
        "id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"players\", \"world\"]\nexperimental = [\"players\", \"world\"]\n",
    )
    .unwrap();
    assert!(manifest
        .check_imports(["horizon:mods/world@0.1.0"])
        .is_ok());
    let no_world = horizon_mods::Manifest::parse(
        "id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"players\"]\nexperimental = [\"players\"]\n",
    )
    .unwrap();
    assert!(no_world
        .check_imports(["horizon:mods/world@0.1.0"])
        .is_err());
}
