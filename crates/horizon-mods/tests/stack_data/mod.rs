use super::*;
use horizon_mods::{StackInfo, StorageLocation as L, StorageStack, StorageView, WorldView};
struct View(StorageView);
impl WorldView for View {
    fn block(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
        None
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
        if item == "minecraft:stick" && (1..=64).contains(&count) {
            Ok(())
        } else {
            Err("invalid stack".into())
        }
    }
    fn storage(&mut self) -> Option<&mut StorageView> {
        Some(&mut self.0)
    }
}
fn stack(value: Option<&str>) -> Option<StorageStack> {
    Some(StorageStack {
        info: StackInfo {
            item: "minecraft:stick".into(),
            count: 12,
            display_key: value.unwrap_or("other-only").into(),
            custom_name: None,
        },
        max: 64,
        data: [
            ("test_api20_server".into(), Ok(value.map(str::to_owned))),
            ("other_mod".into(), Ok(Some("private".into()))),
        ]
        .into(),
    })
}
fn setup() -> (Mods, ServerPlayer) {
    let mut mods = load("horizon_mods_test_api20_server_guest.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.wants_storage());
    assert_eq!(mods.stack_data_mods(), ["test_api20_server"]);
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
    let loc = |x| L::Container {
        dimension: p.dimension.clone(),
        pos: (x, 64, 0),
    };
    let mut inv = vec![None; 36];
    inv[0] = stack(Some("own"));
    inv[4] = stack(Some("held"));
    inv[35] = stack(None);
    mods.lend_world(Box::new(View(StorageView {
        slots: [
            (loc(0), vec![stack(Some("chest")), None]),
            (loc(1), vec![stack(Some("special"))]),
            (L::Player(p.id.clone()), inv),
        ]
        .into(),
        read_only: [loc(1)].into(),
        held: [(p.id.clone(), 4)].into(),
    })));
    mods.set_snapshot(Snapshot {
        tick: 1,
        players: vec![p.clone()],
        ..Default::default()
    });
    mods.drain();
    (mods, p)
}
fn command(mods: &mut Mods, p: &ServerPlayer, mode: &str) -> Vec<Output> {
    assert!(mods.server_command(p, &format!("data29 {mode}")));
    assert_eq!(mods.entries()[0].status, Status::Running);
    let out = mods.drain();
    assert!(
        out.iter().any(
            |o| matches!(o, Output::Reply { text, error: false, .. } if text == "data checked")
        ),
        "{out:?}"
    );
    assert!(
        !out.iter().any(|o| matches!(
            o,
            Output::Reply { error: true, .. } | Output::Stopped { .. }
        )),
        "{out:?}"
    );
    out
}
#[test]
fn api29_stack_data_reads_and_all_validation_cross_the_sandbox() {
    let (mut mods, p) = setup();
    for mode in ["read", "invalid"] {
        let out = command(&mut mods, &p, mode);
        assert_eq!(out.len(), 1);
    }
    let mut world = mods.take_world().unwrap();
    assert_eq!(
        world.storage().unwrap().slots[&L::Player(p.id)][0]
            .as_ref()
            .unwrap()
            .data["other_mod"],
        Ok(Some("private".into()))
    );
}
#[test]
fn api29_stack_data_writes_are_scoped_deferred_and_keep_preimages() {
    let (mut mods, p) = setup();
    let out = command(&mut mods, &p, "write");
    let plans: Vec<_> = out
        .iter()
        .filter_map(|o| {
            if let Output::StorageDataWrite(p) = o {
                Some(p)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(plans.len(), 3);
    assert!(plans.iter().all(|p| p.mod_id == "test_api20_server"));
    assert_eq!(plans[0].slot, 0);
    assert_eq!(plans[0].value.as_deref(), Some("new"));
    assert_eq!(plans[1].slot, 4);
    assert!(plans[1].held);
    assert_eq!(plans[1].value, None);
    assert_eq!(plans[2].value.as_deref(), Some("new chest"));
    for plan in plans {
        assert_eq!(plan.before.data["other_mod"], Ok(Some("private".into())));
    }
    command(&mut mods, &p, "read"); // queued writes have not changed storage
}
#[test]
fn api29_stack_data_budget_is_shared_with_moves_and_resets_next_tick() {
    let (mut mods, p) = setup();
    for tick in [1, 2] {
        mods.set_snapshot(Snapshot {
            tick,
            players: vec![p.clone()],
            ..Default::default()
        });
        let out = command(&mut mods, &p, "budget");
        assert_eq!(
            out.iter()
                .filter(|o| matches!(o, Output::StorageDataWrite(_)))
                .count(),
            63
        );
    }
}
#[test]
fn api29_stack_data_creation_preserves_scope_and_shares_spawn_cap() {
    let (mut mods, p) = setup();
    let out = command(&mut mods, &p, "create");
    let gives: Vec<_> = out
        .iter()
        .filter_map(|o| {
            if let Output::GiveItemWithData {
                player,
                mod_id,
                count,
                data,
                ..
            } = o
            {
                Some((player, mod_id, count, data))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(gives.len(), 2);
    assert_eq!(
        gives[0],
        (
            &p.id,
            &"test_api20_server".to_owned(),
            &6400,
            &Some("é".into())
        )
    );
    assert_eq!(gives[1].3, &None);
    assert_eq!(
        out.iter()
            .filter(|o| matches!(o, Output::SpawnItem { .. }))
            .count(),
        8
    );
    assert_eq!(out.iter().filter(|o| matches!(o, Output::SpawnItemWithData { mod_id, data: Some(data), count: 64, .. } if mod_id == "test_api20_server" && data == "é")).count(), 8);
}

#[test]
fn api29_item_callback_can_read_main_hand_data_without_an_abi_change() {
    let (mut mods, p) = setup();
    assert!(mods.server_command(&p, "behaviour29"));
    mods.drain();
    assert!(mods.item_used(&p, "test:read_data"));
    let out = mods.drain();
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Tell { text, .. } if text == "data-use held")),
        "{out:?}"
    );
}
#[test]
fn api29_demo_lectern_click_queues_counter_and_command_reads_current_data() {
    let (mut source, p) = setup();
    let mut world = source.take_world().unwrap();
    let storage = world.storage().unwrap();
    storage.slots.get_mut(&L::Player(p.id.clone())).unwrap()[4]
        .as_mut()
        .unwrap()
        .data
        .insert("behaviour_demo".into(), Ok(Some("41".into())));
    let mut demo = load("behaviour_demo.hmod");
    demo.set_snapshot(Snapshot {
        tick: 1,
        players: vec![p.clone()],
        ..Default::default()
    });
    demo.lend_world(world);
    demo.drain();
    assert!(demo.block_interacted(
        &p,
        BlockPos { x: 0, y: 64, z: 0 },
        "minecraft:lectern",
        1,
        [0.; 3],
        horizon_mods::BehaviourHand::Main,
        "minecraft:stick",
        false
    ));
    let out = demo.drain();
    assert!(out.iter().any(|o| matches!(o, Output::StorageDataWrite(plan) if plan.held && plan.slot == 4 && plan.mod_id == "behaviour_demo" && plan.value.as_deref() == Some("42"))), "{out:?}");
    assert!(demo.server_command(&p, "stickdata"));
    let out = demo.drain();
    assert!(
        out.iter().any(
            |o| matches!(o, Output::Reply { text, error: false, .. } if text == "stickdata: 41")
        ),
        "{out:?}"
    );
}
