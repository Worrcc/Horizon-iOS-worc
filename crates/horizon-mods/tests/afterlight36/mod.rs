//! API 36/37: server recipe queries and component-safe machine writes.
use super::*;
use horizon_mods::{
    BlockChangePos, DeathPoint, DeathStack, GameMode, MachineStack, MachineTick, NetMessage,
    PlayerDeathDrops, PlayerPoint, Recipe, RecipeItem, RecipeKind, RecipeStack, Recipes,
    ServerPlayer, WorldView,
};
use std::sync::Arc;

struct View {
    empty_energy: bool,
}
impl WorldView for View {
    fn block(&self, dim: &str, x: i32, y: i32, z: i32) -> Option<String> {
        (dim == "minecraft:overworld" && y == 64 && (x == 0 || x == 1) && z == 0)
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
    fn check_structure(&self, id: &str) -> Result<(), String> {
        if id == "test:temple" {
            Ok(())
        } else {
            Err(format!("no structure template {id}"))
        }
    }
    fn energy_side(&self, dim: &str, x: i32, y: i32, z: i32, _face: i32) -> Option<(u64, u64)> {
        if dim != "minecraft:overworld" || y != 64 || z != 0 {
            return None;
        }
        if self.empty_energy {
            return Some((0, 0));
        }
        match x {
            0 => Some((5, 10)),
            1 => Some((2, 10)),
            _ => None,
        }
    }
    fn machine_stacks(&self, dim: &str, pos: BlockChangePos) -> Option<Vec<Option<MachineStack>>> {
        (dim == "minecraft:overworld" && pos.x == 0 && pos.y == 64 && pos.z == 0).then(|| {
            vec![
                Some(MachineStack {
                    item: "minecraft:stick".into(),
                    count: 4,
                    components: Some(vec![1, 2, 3]),
                }),
                None,
            ]
        })
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
        world_seed: 42,
        generated_chunks: vec![
            horizon_mods::ChunkPos {
                dimension: "minecraft:overworld".into(),
                x: 3,
                z: -2,
            },
            horizon_mods::ChunkPos {
                dimension: "minecraft:overworld".into(),
                x: 2,
                z: -4,
            },
        ]
        .into(),
        ..Default::default()
    });
    mods.lend_world(Box::new(View {
        empty_energy: false,
    }));
    mods.drain();
    (mods, p)
}

fn run(mods: &mut Mods, p: &ServerPlayer, cmd: &str) -> Vec<String> {
    mods.drain();
    assert!(mods.server_command(p, cmd), "{cmd}");
    mods.drain()
        .into_iter()
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
        if let Output::Reply {
            text, error: true, ..
        } = o
        {
            errors.push(text);
        }
    }
    assert!(
        errors.iter().any(|e| e.contains("unknown item")),
        "{errors:?}"
    );
}

#[test]
fn api36_recipe_query_is_available_and_needs_world() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "world.recipes-for-item"));
    // The guest also imports API 37; the recipe query itself is level 36.
    assert_eq!(
        horizon_mods::api_level::level_of("world.recipes-for-item"),
        Some(36)
    );
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
    let manifest = horizon_mods::Manifest::parse(
        "id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"players\", \"world\"]\nexperimental = [\"players\", \"world\"]\n",
    )
    .unwrap();
    assert!(manifest.check_imports(["horizon:mods/world@0.1.0"]).is_ok());
    let no_world = horizon_mods::Manifest::parse(
        "id = \"x\"\nname = \"X\"\nversion = \"0.1.0\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"players\"]\nexperimental = [\"players\"]\n",
    )
    .unwrap();
    assert!(
        no_world
            .check_imports(["horizon:mods/world@0.1.0"])
            .is_err()
    );
}

#[test]
fn api37_machine_stacks_read_components_and_queue_checked_writes() {
    let (mut m, p) = setup();
    assert_eq!(
        run(&mut m, &p, "machine37 read"),
        ["machine [minecraft:stickx4+,_]"]
    );
    // A matching preimage queues a deferred write.
    let out = {
        m.drain();
        assert!(m.server_command(&p, "machine37 write"));
        m.drain()
    };
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Tell { text, .. } if text == "machine queued"))
    );
    let writes: Vec<_> = out
        .iter()
        .filter_map(|o| match o {
            Output::MachineStackWrite(plan) => Some(plan),
            _ => None,
        })
        .collect();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].mod_id, "test_api20_server");
    assert_eq!(writes[0].pos.x, 0);
    assert_eq!(writes[0].changes.len(), 1);
    let (slot, expect, write) = &writes[0].changes[0];
    assert_eq!(*slot, 0);
    assert_eq!(expect.as_ref().unwrap().count, 4);
    assert_eq!(write.as_ref().unwrap().count, 5);
    // A wrong preimage is refused in the sandbox before a write is queued.
    let out = {
        m.drain();
        assert!(m.server_command(&p, "machine37 bad"));
        m.drain()
    };
    assert!(
        !out.iter()
            .any(|o| matches!(o, Output::MachineStackWrite(_)))
    );
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Reply { error: true, .. }))
    );
}

#[test]
fn api37_machine_write_level_and_capability() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "world.machine-stacks"));
    assert!(imports.iter().any(|f| f == "world.set-machine-stacks"));
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}

#[test]
fn api38_claimant_keeps_death_drops_and_only_it_can_read_them() {
    let (mut m, p) = setup();
    run(&mut m, &p, "death38 on");
    run(&mut m, &p, "death38 claim");
    assert_eq!(
        m.death_drop_claimant(),
        Some("test_api20_server".to_owned())
    );
    let at = PlayerPoint {
        x: 1.0,
        y: 64.0,
        z: 2.0,
    };
    let drops = vec![MachineStack {
        item: "minecraft:diamond".into(),
        count: 2,
        components: None,
    }];
    assert!(
        m.player_died(
            &p,
            "minecraft:overworld",
            DeathPoint {
                x: at.x,
                y: at.y,
                z: at.z,
            },
            drops
                .iter()
                .map(|s| DeathStack {
                    item: s.item.clone(),
                    count: s.count,
                    components: s.components.clone(),
                })
                .collect(),
            "test_api20_server"
        )
    );
    m.drain();
    // The game stages a claimed death; the next snapshot carries it.
    m.set_snapshot(Snapshot {
        tick: 2,
        players: vec![p.clone()],
        death_drop_claimant: Some("test_api20_server".into()),
        player_death_drops: vec![PlayerDeathDrops {
            player: p.id.clone(),
            dimension: "minecraft:overworld".into(),
            at,
            drops: drops.clone(),
        }]
        .into(),
        ..Default::default()
    });
    assert_eq!(
        run(&mut m, &p, "death38 read"),
        ["claimed [alice:minecraft:overworld:minecraft:diamond]"]
    );
    // Another mod's snapshot never sees them.
    m.set_snapshot(Snapshot {
        tick: 3,
        players: vec![p.clone()],
        death_drop_claimant: Some("someone_else".into()),
        player_death_drops: vec![PlayerDeathDrops {
            player: p.id.clone(),
            dimension: "minecraft:overworld".into(),
            at,
            drops,
        }]
        .into(),
        ..Default::default()
    });
    assert_eq!(run(&mut m, &p, "death38 read"), ["claimed []"]);
}

#[test]
fn api38_skipped_deaths_leave_vanilla_drops_and_level_is_38() {
    let (mut m, p) = setup();
    run(&mut m, &p, "death38 on");
    run(&mut m, &p, "death38 skip");
    assert!(!m.player_died(
        &p,
        "minecraft:overworld",
        DeathPoint {
            x: 0.0,
            y: 0.0,
            z: 0.0,
        },
        vec![DeathStack {
            item: "minecraft:stick".into(),
            count: 1,
            components: None,
        }],
        "test_api20_server"
    ));
    m.drain();
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "players.claimed-deaths"));
    assert!(imports.iter().any(|f| f == "server-hooks.set-death-drops"));
    assert_eq!(
        horizon_mods::api_level::level_of("players.claimed-deaths"),
        Some(38)
    );
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}

#[test]
fn api39_lookup_registers_discovers_and_removes() {
    let (mut m, p) = setup();
    assert_eq!(
        run(&mut m, &p, "interop39 lookup register"),
        ["lookup register [test_api20_server:Chest energy]"]
    );
    assert_eq!(
        run(&mut m, &p, "interop39 lookup unregister"),
        ["lookup unregister [none]"]
    );
}

#[test]
fn api39_energy_reads_and_queues_checked_changes() {
    let (mut m, p) = setup();
    assert_eq!(
        run(&mut m, &p, "interop39 energy read"),
        ["energy read [5:10]"]
    );
    let out = {
        m.drain();
        assert!(m.server_command(&p, "interop39 energy insert"));
        m.drain()
    };
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Tell { text, .. } if text == "energy insert [4]"))
    );
    let changes: Vec<_> = out
        .iter()
        .filter_map(|o| match o {
            Output::EnergyChange(plan) => Some(plan),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].before, (5, 10));
    assert_eq!(changes[0].amount, 4);
    let out = {
        m.drain();
        assert!(m.server_command(&p, "interop39 energy extract"));
        m.drain()
    };
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::EnergyChange(plan) if plan.amount == 3))
    );
}

#[test]
fn api39_functions_are_level_39_and_need_capabilities() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "energy.energy-side"));
    assert!(imports.iter().any(|f| f == "lookup.find"));
    assert_eq!(
        horizon_mods::api_level::level_of("energy.insert-energy"),
        Some(39)
    );
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
    let manifest = horizon_mods::Manifest::parse(
        "id = \"x\"\nname = \"X\"\nversion = \"1\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"players\", \"energy\", \"lookup\"]\nexperimental = [\"energy\", \"lookup\"]\n",
    )
    .unwrap();
    assert!(
        manifest
            .check_imports(["horizon:mods/energy@0.1.0"])
            .is_ok()
    );
    assert!(
        manifest
            .check_imports(["horizon:mods/lookup@0.1.0"])
            .is_ok()
    );
}

#[test]
fn api40_lifecycle_hooks_are_opt_in_and_routed() {
    let (mut m, p) = setup();
    // Disabled by default: no output from the lifecycle methods.
    m.lifecycle_tick(true, 1);
    assert!(m.drain().is_empty());
    run(&mut m, &p, "lifecycle40 on");
    m.lifecycle_tick(true, 7);
    m.lifecycle_tick(false, 7);
    m.player_respawned_hook(
        &p,
        "minecraft:overworld",
        DeathPoint {
            x: 0.0,
            y: 64.0,
            z: 0.0,
        },
    );
    m.server_save(true, false);
    m.server_save(false, true);
    let lines: Vec<String> = m
        .drain()
        .into_iter()
        .filter_map(|o| match o {
            Output::Broadcast { text, .. } => Some(text),
            Output::Tell { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(
        lines,
        [
            "tick start 7",
            "tick end 7",
            "respawned minecraft:overworld",
            "before save",
            "after save true",
        ]
    );
}

#[test]
fn api40_hook_level_is_40() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(
        imports
            .iter()
            .any(|f| f == "server-hooks.set-lifecycle-hooks")
    );
    assert_eq!(
        horizon_mods::api_level::level_of("server-hooks.set-lifecycle-hooks"),
        Some(40)
    );
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}

#[test]
fn api41_machine_ticks_and_attachments_cross_the_sandbox() {
    let (mut m, p) = setup();
    run(&mut m, &p, "interop41 watch");
    m.machine_ticks(vec![MachineTick {
        dimension: "minecraft:overworld".into(),
        pos: (0, 64, 0),
        state: "test:machine".into(),
    }]);
    assert_eq!(
        m.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Broadcast { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["machine minecraft:overworld 64 test:machine"]
    );
    run(&mut m, &p, "interop41 attach set");
    assert_eq!(
        run(&mut m, &p, "interop41 attach get"),
        ["attach get [[1, 2, 3]]"]
    );
    run(&mut m, &p, "interop41 attach clear");
    assert_eq!(run(&mut m, &p, "interop41 attach get"), ["attach get [[]]"]);
}

#[test]
fn api41_functions_are_level_41() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "attachments.attachment"));
    assert!(imports.iter().any(|f| f == "server-hooks.watch-machines"));
    assert_eq!(
        horizon_mods::api_level::level_of("attachments.set-attachment"),
        Some(41)
    );
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}

#[test]
fn api42_net_registers_sends_and_delivers() {
    let (mut m, p) = setup();
    run(&mut m, &p, "net42 register");
    let out = {
        m.drain();
        assert!(m.server_command(&p, "net42 send"));
        m.drain()
    };
    let messages: Vec<_> = out
        .iter()
        .filter_map(|o| match o {
            Output::NetMessage(message) => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].from, "test_api20_server");
    assert_eq!(messages[0].name, "demo");
    assert_eq!(messages[0].data, vec![1, 2, 3]);
    // A message from another mod is delivered on the next tick.
    m.receive_net(NetMessage {
        from: "other".into(),
        to: None,
        name: "demo".into(),
        data: vec![9],
    });
    m.deliver_net();
    assert_eq!(
        m.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Broadcast { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["net from other demo 1"]
    );
}

#[test]
fn api42_functions_are_level_42() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "net.send-to-all"));
    assert_eq!(horizon_mods::api_level::level_of("net.register"), Some(42));
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}

#[test]
fn api43_worldgen_sites_are_seeded_and_only_for_new_chunks() {
    let (mut m, p) = setup();
    run(&mut m, &p, "worldgen43 register");
    assert_eq!(
        run(&mut m, &p, "worldgen43 query"),
        ["worldgen [38:-55:0:false] nonselected [none]", "seed 42"]
    );

    // Asking about an old/un-generated chunk must not create a site.
    m.set_snapshot(Snapshot {
        tick: 2,
        players: vec![p.clone()],
        world_seed: 42,
        generated_chunks: Vec::new().into(),
        ..Default::default()
    });
    assert_eq!(
        run(&mut m, &p, "worldgen43 query"),
        ["worldgen [none] nonselected [none]", "seed 42"]
    );

    // The deterministic seed is surfaced on the server world interface.
    assert_eq!(
        horizon_mods::api_level::level_of("world.world-seed"),
        Some(43)
    );
}

#[test]
fn api43_worldgen_functions_require_capability_and_raise_api_level() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "worldgen.register-feature"));
    assert!(imports.iter().any(|f| f == "world.world-seed"));
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
    let manifest = horizon_mods::Manifest::parse(
        "id = \"x\"\nname = \"X\"\nversion = \"1\"\napi = \"0.3\"\nside = \"server\"\ncapabilities = [\"worldgen\"]\nexperimental = [\"worldgen\"]\n",
    )
    .unwrap();
    assert!(
        manifest
            .check_imports(["horizon:mods/worldgen@0.1.0"])
            .is_ok()
    );
}

fn setup_fresh() -> (Mods, ServerPlayer) {
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
        world_seed: 42,
        ..Default::default()
    });
    mods.lend_world(Box::new(View { empty_energy: true }));
    mods.drain();
    (mods, p)
}

#[test]
fn api44_fresh_store_can_be_initialized() {
    let (mut m, p) = setup_fresh();
    assert_eq!(
        run(&mut m, &p, "interop39 energy read"),
        ["energy read [0:0]"]
    );
    let out = {
        m.drain();
        assert!(m.server_command(&p, "interop39 energy cap"));
        m.drain()
    };
    let changes: Vec<_> = out
        .iter()
        .filter_map(|o| match o {
            Output::EnergyChange(plan) => Some(plan),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, horizon_mods::EnergyKind::Capacity);
    assert_eq!(changes[0].amount, 10);
}

#[test]
fn api44_reservations_cap_repeated_inserts() {
    let (mut m, p) = setup();
    // Store starts at 5/10; four then four can only add five.
    assert_eq!(
        run(&mut m, &p, "interop39 energy insert-twice"),
        ["energy insert-twice [4:1]"]
    );
}

#[test]
fn api44_transfer_is_one_atomic_operation() {
    let (mut m, p) = setup();
    let out = {
        m.drain();
        assert!(m.server_command(&p, "interop39 energy transfer"));
        m.drain()
    };
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Tell { text, .. } if text == "energy transfer [5]"))
    );
    let changes: Vec<_> = out
        .iter()
        .filter_map(|o| match o {
            Output::EnergyChange(plan) => Some(plan),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, horizon_mods::EnergyKind::Transfer);
    assert_eq!(changes[0].amount, 5);
    assert_eq!(changes[0].to.as_ref().unwrap().1, (1, 64, 0));
}

#[test]
fn api44_machine_watch_matches_ids_without_properties() {
    let (mut m, p) = setup();
    run(&mut m, &p, "interop41 watch");
    m.machine_ticks(vec![MachineTick {
        dimension: "minecraft:overworld".into(),
        pos: (0, 64, 0),
        state: "test:machine[facing=north]".into(),
    }]);
    assert_eq!(
        m.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Broadcast { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>(),
        ["machine minecraft:overworld 64 test:machine[facing=north]"]
    );
}

#[test]
fn api44_level_is_44() {
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod"))
        .unwrap()
        .wasm
        .unwrap();
    let imports = horizon_mods::api_level::imports(&wasm).unwrap();
    assert!(imports.iter().any(|f| f == "energy.transfer-energy"));
    assert_eq!(
        horizon_mods::api_level::level_of("energy.transfer-energy"),
        Some(44)
    );
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}
