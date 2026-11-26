use super::*;
use horizon_mods::{
    BlockChangePos, BlockDataImage, MachineStack, MenuLease, MenuOperation, WorldView,
};
struct View;
impl WorldView for View {
    fn block(&self, dim: &str, x: i32, y: i32, z: i32) -> Option<String> {
        (self.has_dimension(dim) && x == 0 && y == 64 && z == 0).then(|| "demo:crate".into())
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
    fn check_item_stack(&self, item: &str, count: u32) -> Result<(), String> {
        if matches!(item, "minecraft:stick" | "minecraft:diamond_block")
            && (1..=64).contains(&count)
        {
            Ok(())
        } else {
            Err("invalid item".into())
        }
    }
    fn block_data(&self, _: &str, _: BlockChangePos, id: &str) -> Result<BlockDataImage, String> {
        assert_eq!(id, "test_api20_server");
        Ok(BlockDataImage {
            state: "demo:crate".into(),
            token: "incarnation".into(),
            value: Some("counter".into()),
        })
    }
}
fn setup() -> (Mods, ServerPlayer) {
    let mut mods = load("horizon_mods_test_api20_server_guest.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
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
        ..Default::default()
    });
    mods.lend_world(Box::new(View));
    mods.drain();
    (mods, p)
}
fn command(mods: &mut Mods, p: &ServerPlayer, mode: &str) -> Vec<Output> {
    assert!(mods.server_command(p, &format!("machines30 {mode}")));
    assert_eq!(mods.entries()[0].status, Status::Running);
    let out = mods.drain();
    assert!(
        out.iter()
            .any(|o| matches!(o,Output::Reply{text,error:false,..}if text=="machines checked")),
        "{out:?}"
    );
    out
}
#[test]
fn api30_menu_validation_and_deferred_copies() {
    let (mut m, p) = setup();
    assert_eq!(command(&mut m, &p, "menu-invalid").len(), 1);
    let out = command(&mut m, &p, "menu");
    assert_eq!(
        out.iter().filter(|o| matches!(o, Output::Menu(_))).count(),
        3
    );
    let Output::Menu(MenuOperation::Open { lease, items, .. }) = &out[0] else {
        panic!("{out:?}")
    };
    assert_eq!(lease.owner, "test_api20_server");
    assert_eq!(lease.rows, 3);
    assert_eq!(items[0].as_ref().unwrap().lore, ["line"]);
}
#[test]
fn api30_block_data_reads_preimages_and_caps() {
    let (mut m, p) = setup();
    assert_eq!(command(&mut m, &p, "data-invalid").len(), 1);
    let out = command(&mut m, &p, "data");
    let plans: Vec<_> = out
        .iter()
        .filter_map(|o| {
            if let Output::BlockDataWrite(p) = o {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(plans.len(), 2);
    assert_eq!(plans[0].mod_id, "test_api20_server");
    assert_eq!(plans[0].before.value.as_deref(), Some("counter"));
    assert_eq!(plans[0].value.as_deref(), Some("next"));
    assert_eq!(plans[1].value, None);
}
#[test]
fn api30_write_budgets_span_hooks_and_reset_next_tick() {
    for (mode, kind) in [("menu-cap", 0), ("data-cap", 1), ("progress-cap", 2)] {
        let (mut m, p) = setup();
        for tick in [1, 2] {
            m.set_snapshot(Snapshot {
                tick,
                players: vec![p.clone()],
                ..Default::default()
            });
            let out = command(&mut m, &p, mode);
            assert_eq!(
                out.iter()
                    .filter(|o| match kind {
                        0 => matches!(o, Output::Menu(_)),
                        1 => matches!(o, Output::BlockDataWrite(_)),
                        _ => matches!(o, Output::BreakProgress { .. }),
                    })
                    .count(),
                64
            );
        }
    }
}
#[test]
fn api30_machine_host_results_validation_and_caps() {
    let (mut m, p) = setup();
    assert_eq!(command(&mut m, &p, "destroy-invalid").len(), 1);
    let mut requests = Vec::new();
    m.with_machines(
        |r, _| {
            let collect = r.collect;
            requests.push(r);
            Ok(if collect {
                vec![MachineStack {
                    item: "minecraft:stick".into(),
                    count: 3,
                    components: None,
                }]
            } else {
                vec![]
            })
        },
        |m| {
            command(m, &p, "destroy");
        },
    );
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].tool.as_deref(), Some("minecraft:stick"));
    assert_eq!(requests[0].mod_id, "test_api20_server");
    let (mut m, p) = setup();
    let mut count = 0;
    m.with_machines(
        |_, _| {
            count += 1;
            Ok(vec![])
        },
        |m| {
            command(m, &p, "destroy-cap");
        },
    );
    assert_eq!(count, 64);
    let (mut m, p) = setup();
    let out = command(&mut m, &p, "progress");
    assert_eq!(
        out.iter()
            .filter(|o| matches!(o, Output::BreakProgress { .. }))
            .count(),
        11
    );
}
#[test]
fn api30_clicks_are_owned_validated_capped_and_close_once() {
    let (mut m, p) = setup();
    m.menu_opened(MenuLease {
        owner: "test_api20_server".into(),
        rows: 3,
        player: p.clone(),
        anchor: Some((p.dimension.clone(), BlockPos { x: 0, y: 64, z: 0 })),
    });
    for click in [(63, 0, 0), (-1, 0, 0), (0, 0, 7), (0, -1, 0)] {
        m.menu_event(&p, Some(click));
    }
    let mut far = p.clone();
    far.x = 20.;
    m.menu_event(&far, Some((0, 0, 0)));
    assert!(m.drain().is_empty());
    for _ in 0..65 {
        m.menu_event(&p, Some((0, 0, 0)));
    }
    assert_eq!(
        m.drain()
            .iter()
            .filter(|o| matches!(o,Output::Tell{text,..}if text=="menu-click 0 0 0"))
            .count(),
        64
    );
    m.menu_event(&p, None);
    m.menu_event(&p, None);
    assert_eq!(
        m.drain()
            .iter()
            .filter(|o| matches!(o,Output::Tell{text,..}if text=="menu-close"))
            .count(),
        1
    );
}

#[test]
fn api30_demo_cancels_lectern_allows_machine_and_gives_real_diamond() {
    let (_, p) = setup();
    let mut m = load("machine_demo.hmod");
    m.set_snapshot(Snapshot {
        tick: 1,
        players: vec![p.clone()],
        ..Default::default()
    });
    m.lend_world(Box::new(View));
    let inventories = horizon_mods::testing::InventoryFixture {
        players: [(p.id.clone(), false)].into(),
        ..Default::default()
    };
    let counts = inventories.counts.clone();
    m.lend_inventories(Box::new(inventories));
    m.drain();
    let pos = BlockPos { x: 0, y: 64, z: 0 };
    assert!(m.block_interacted(
        &p,
        pos,
        "minecraft:lectern",
        1,
        [0.5, 1., 0.5],
        horizon_mods::BehaviourHand::Main,
        "minecraft:air",
        false
    ));
    let out = m.drain();
    let lease = out
        .iter()
        .find_map(|o| {
            if let Output::Menu(MenuOperation::Open { lease, items, .. }) = o {
                assert_eq!(items[13].as_ref().unwrap().item, "minecraft:diamond_block");
                Some(lease.clone())
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(lease.anchor.as_ref().unwrap().0, p.dimension);
    m.menu_opened(lease);
    m.menu_event(&p, Some((13, 0, 0)));
    assert_eq!(
        counts.lock().unwrap()[&(p.id.clone(), "minecraft:diamond".into())],
        1
    );
    assert!(!m.block_interacted(
        &p,
        pos,
        "minecraft:lectern",
        1,
        [0.5, 1., 0.5],
        horizon_mods::BehaviourHand::Off,
        "minecraft:air",
        false
    ));
    assert!(!m.block_interacted(
        &p,
        pos,
        "demo:smelter",
        1,
        [0.5, 1., 0.5],
        horizon_mods::BehaviourHand::Main,
        "minecraft:air",
        false
    ));
    assert_eq!(m.entries()[0].status, Status::Running);
}
