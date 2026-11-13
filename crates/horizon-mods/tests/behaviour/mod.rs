use super::*;
fn actor() -> ServerPlayer {
    ServerPlayer {
        id: "alice".into(),
        name: "Alice".into(),
        x: 0.0,
        y: 64.0,
        z: 0.0,
        dimension: "minecraft:overworld".into(),
        game_mode: GameMode::Survival,
        health: 20.0,
    }
}
fn setup() -> (Mods, ServerPlayer, BlockPos) {
    let mut m = load("behaviour_demo.hmod");
    assert_eq!(m.entries()[0].status, Status::Running);
    let p = actor();
    m.set_snapshot(Snapshot {
        tick: 1,
        players: vec![p.clone()],
        ..Default::default()
    });
    m.drain();
    (m, p, BlockPos { x: 0, y: 64, z: 0 })
}
#[test]
fn api29_break_place_filters_cancellation_and_independent_budgets() {
    let (mut m, p, pos) = setup();
    assert!(!m.block_breaking(&p, pos, "minecraft:stone"));
    assert!(!m.block_placing(&p, pos, "minecraft:stone", "minecraft:stone"));
    assert!(m.drain().is_empty());
    for _ in 0..32 {
        assert!(m.block_breaking(&p, pos, "minecraft:obsidian"));
    }
    assert!(!m.block_breaking(&p, pos, "minecraft:obsidian"));
    assert_eq!(
        m.drain()
            .iter()
            .filter(|o| matches!(o, Output::Tell { .. }))
            .count(),
        32
    );
    assert!(m.block_placing(&p, pos, "minecraft:tnt[unstable=false]", "minecraft:tnt"));
    assert!(!m.block_placing(&p, pos, "behaviour_demo:block", "behaviour_demo:item"));
    assert_eq!(
        m.drain()
            .iter()
            .filter(|o| matches!(o, Output::Tell { .. }))
            .count(),
        2
    );
    m.set_snapshot(Snapshot {
        tick: 2,
        players: vec![p.clone()],
        ..Default::default()
    });
    assert!(m.block_breaking(&p, pos, "minecraft:obsidian"));
}
#[test]
fn api29_broken_guests_allow_the_action_and_stop_once() {
    for state in ["test:trap", "test:spin"] {
        let mut m = load("horizon_mods_test_api20_server_guest.hmod");
        let p = actor();
        m.set_snapshot(Snapshot {
            tick: 1,
            players: vec![p.clone()],
            ..Default::default()
        });
        assert!(m.server_command(&p, "behaviour29"));
        m.drain();
        assert!(!m.block_breaking(&p, BlockPos { x: 0, y: 64, z: 0 }, state));
        assert!(matches!(m.entries()[0].status, Status::Failed(_)));
        assert_eq!(
            m.drain()
                .iter()
                .filter(|o| matches!(o, Output::Stopped { .. }))
                .count(),
            1
        );
        assert!(!m.block_breaking(&p, BlockPos { x: 0, y: 64, z: 0 }, state));
        assert!(m.drain().is_empty());
    }
}
#[test]
fn api29_interaction_preserves_context_and_has_its_own_filter_and_budget() {
    use horizon_mods::BehaviourHand;
    let (mut m, p, pos) = setup();
    assert!(!m.block_interacted(
        &p,
        pos,
        "minecraft:stone",
        1,
        [0.25, 1.0, 0.75],
        BehaviourHand::Off,
        "minecraft:stick",
        true
    ));
    assert!(m.drain().is_empty());
    for _ in 0..32 {
        assert!(m.block_interacted(
            &p,
            pos,
            "minecraft:lectern[facing=north]",
            1,
            [0.25, 1.0, 0.75],
            BehaviourHand::Off,
            "minecraft:stick",
            true
        ));
    }
    assert!(!m.block_interacted(
        &p,
        pos,
        "minecraft:lectern",
        1,
        [0.0; 3],
        BehaviourHand::Main,
        "minecraft:air",
        false
    ));
    let out = m.drain();
    assert_eq!(
        out.iter()
            .filter(|o| matches!(o, Output::Tell { .. }))
            .count(),
        32
    );
    assert!(out.iter().any(|o| matches!(o,Output::Tell { text,.. } if text.ends_with("1 0.25 1 0.75 Hand::Off minecraft:stick true"))));
    assert!(m.block_breaking(&p, pos, "minecraft:obsidian"));
}
fn ticking() -> (Mods, ServerPlayer, BlockPos) {
    struct View;
    impl horizon_mods::WorldView for View {
        fn block(&self, d: &str, x: i32, _: i32, _: i32) -> Option<String> {
            if d != "minecraft:overworld" {
                return None;
            }
            match x {
                0 => Some("test:block".into()),
                1 => Some("minecraft:stone".into()),
                _ => None,
            }
        }
        fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn has_dimension(&self, d: &str) -> bool {
            d == "minecraft:overworld"
        }
        fn check_state(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    let mut m = load("horizon_mods_test_api20_server_guest.hmod");
    let p = actor();
    m.set_snapshot(Snapshot {
        tick: 1,
        players: vec![p.clone()],
        ..Default::default()
    });
    assert!(m.server_command(&p, "behaviour29"));
    m.lend_world(Box::new(View));
    m.drain();
    (m, p, BlockPos { x: 0, y: 64, z: 0 })
}
#[test]
fn api29_neighbour_and_random_observations_are_filtered_and_bounded() {
    use horizon_mods::BehaviourFamily as F;
    let (mut m, p, pos) = ticking();
    assert!(m.server_command(&p, "filters29"));
    for f in [F::Neighbour, F::Random] {
        m.observe_behaviour_block(f, 1, &p.dimension, pos, "minecraft:stone", "minecraft:tnt");
        for _ in 0..40 {
            m.observe_behaviour_block(
                f,
                1,
                &p.dimension,
                pos,
                "test:block[prop=true]",
                "minecraft:tnt",
            );
        }
    }
    m.flush_behaviour_ticks();
    let out = m.drain();
    assert_eq!(
        out.iter()
            .filter(|o| matches!(o, Output::Broadcast { .. }))
            .count(),
        64
    );
    assert!(out.iter().any(|o| matches!(o,Output::Broadcast { text,.. } if text == "neighbour minecraft:overworld 0 test:block[prop=true] minecraft:tnt")));
    assert!(out.iter().any(|o| matches!(o,Output::Broadcast { text,.. } if text == "random minecraft:overworld 0 test:block[prop=true]")));
    m.flush_behaviour_ticks();
    assert!(m.drain().is_empty());
}
#[test]
fn api29_scheduling_validates_bounds_and_caps_and_fires_once_when_due() {
    let (mut m, p, _) = ticking();
    assert!(m.server_command(&p, "schedule29 invalid"));
    assert!(m.drain().is_empty());
    assert!(m.server_command(&p, "schedule29 1"));
    m.flush_behaviour_ticks();
    assert!(m.drain().is_empty());
    m.set_snapshot(Snapshot {
        tick: 2,
        players: vec![p.clone()],
        ..Default::default()
    });
    m.flush_behaviour_ticks();
    assert_eq!(m.drain().iter().filter(|o| matches!(o,Output::Broadcast { text,.. } if text == "scheduled minecraft:overworld 0 test:block")).count(),1);
    m.flush_behaviour_ticks();
    assert!(m.drain().is_empty());
    assert!(m.server_command(&p, "schedule29 caps"));
    assert!(m.drain().is_empty());
    m.set_snapshot(Snapshot {
        tick: 22,
        players: vec![p.clone()],
        ..Default::default()
    });
    m.flush_behaviour_ticks();
    assert_eq!(
        m.drain()
            .iter()
            .filter(|o| matches!(o, Output::Broadcast { .. }))
            .count(),
        32
    );
    m.set_snapshot(Snapshot {
        tick: 23,
        players: vec![p],
        ..Default::default()
    });
    m.flush_behaviour_ticks();
    assert!(m.drain().is_empty());
}
#[test]
fn api29_scheduled_ticks_discard_changed_blocks_and_unloaded_chunks_even_if_restored() {
    for unload in [false, true] {
        let (mut m, p, pos) = ticking();
        assert!(m.server_command(&p, "schedule29 1"));
        if unload {
            m.behaviour_chunk_unloaded(&p.dimension, [0, 0]);
        } else {
            m.behaviour_block_changed(&p.dimension, pos);
        }
        m.set_snapshot(Snapshot {
            tick: 2,
            players: vec![p],
            ..Default::default()
        });
        m.flush_behaviour_ticks();
        assert!(m.drain().is_empty());
    }
    let (mut m, p, _) = ticking();
    assert!(m.server_command(&p, "schedule29 1"));
    m.take_world();
    m.set_snapshot(Snapshot {
        tick: 2,
        players: vec![p],
        ..Default::default()
    });
    m.flush_behaviour_ticks();
    assert!(m.drain().is_empty());
}
#[test]
fn api29_item_hooks_match_items_and_cancel_independently_with_full_context() {
    let (mut m, p, pos) = ticking();
    assert!(!m.item_used_on_block(&p, "minecraft:stick", pos, "test:block", 4));
    assert!(!m.item_used(&p, "minecraft:stick"));
    assert!(!m.item_hit_entity(&p, "minecraft:stick", "minecraft:cow", 42));
    m.item_use_finished(&p, "minecraft:stick");
    assert!(m.drain().is_empty());
    for _ in 0..32 {
        assert!(m.item_used(&p, "minecraft:ender_pearl"));
    }
    assert!(!m.item_used(&p, "minecraft:ender_pearl"));
    assert_eq!(
        m.drain()
            .iter()
            .filter(|o| matches!(o, Output::Tell { .. }))
            .count(),
        32
    );
    assert!(m.item_used_on_block(&p, "test:wand", pos, "minecraft:obsidian", 4));
    assert!(m.item_hit_entity(&p, "test:wand", "minecraft:cow", 42));
    m.item_use_finished(&p, "test:wand");
    let out = m.drain();
    for expected in [
        "item-on test:wand 64 minecraft:obsidian 4",
        "item-hit test:wand minecraft:cow 42",
        "item-finish test:wand",
    ] {
        assert!(
            out.iter()
                .any(|o| matches!(o,Output::Tell {text,..} if text==expected)),
            "{expected}: {out:?}"
        );
    }
    for _ in 0..31 {
        assert!(m.item_used_on_block(&p, "test:wand", pos, "minecraft:stone", 1));
        m.item_use_finished(&p, "test:wand");
        assert!(m.item_hit_entity(&p, "test:wand", "minecraft:cow", 42));
    }
    m.drain();
    assert!(!m.item_used_on_block(&p, "test:wand", pos, "minecraft:stone", 1));
    assert!(!m.item_hit_entity(&p, "test:wand", "minecraft:cow", 42));
    m.item_use_finished(&p, "test:wand");
    assert!(m.drain().is_empty());
}
#[test]
fn api29_chat_and_pickup_flags_need_no_ids_and_fail_open_on_budget_or_bad_output() {
    let (mut m, p, _) = ticking();
    assert!(m.item_picked_up(&p, "minecraft:unwatched"));
    assert!(
        m.drain()
            .iter()
            .any(|o| matches!(o,Output::Tell {text,..} if text=="pickup minecraft:unwatched"))
    );
    assert_eq!(m.chat_sent_filter(&p, "hello").as_deref(), Some("hello"));
    assert_eq!(m.chat_sent_filter(&p, "cancel"), None);
    assert_eq!(
        m.chat_sent_filter(&p, "replace").as_deref(),
        Some("replacement")
    );
    assert_eq!(m.chat_sent_filter(&p, "large").as_deref(), Some("large"));
    assert_eq!(
        m.chat_sent_filter(&p, "illegal").as_deref(),
        Some("illegal")
    );
    assert_eq!(m.chat_sent_filter(&p, "unicode"), Some("é".repeat(256)));
    for _ in 0..26 {
        assert_eq!(m.chat_sent_filter(&p, "cancel"), None);
    }
    assert_eq!(m.chat_sent_filter(&p, "cancel").as_deref(), Some("cancel"));
    for _ in 0..31 {
        assert!(m.item_picked_up(&p, "minecraft:unwatched"));
    }
    m.drain();
    assert!(!m.item_picked_up(&p, "minecraft:unwatched"));
    assert!(m.drain().is_empty());
    m.set_snapshot(Snapshot {
        tick: 2,
        players: vec![p.clone()],
        ..Default::default()
    });
    assert_eq!(m.chat_sent_filter(&p, "cancel"), None);
    assert!(m.item_picked_up(&p, "minecraft:unwatched"));
}
#[test]
fn api29_demo_replaces_words_and_cancels_ender_pearls() {
    let (mut m, p, _) = setup();
    assert_eq!(
        m.chat_sent_filter(&p, "oh heck! heck heckling").as_deref(),
        Some("oh hmm! hmm heckling")
    );
    assert_eq!(m.chat_sent_filter(&p, "fine").as_deref(), Some("fine"));
    assert!(m.item_used(&p, "minecraft:ender_pearl"));
    assert!(!m.item_used(&p, "minecraft:snowball"));
}
#[test]
fn api29_legacy_server_exports_load_and_missing_behaviour_callbacks_allow() {
    let mut m = load("legacy_server28.hmod");
    let p = actor();
    assert_eq!(m.entries()[0].status, Status::Running);
    m.set_snapshot(Snapshot {
        tick: 1,
        players: vec![p.clone()],
        ..Default::default()
    });
    assert!(m.server_command(&p, "behaviour29"));
    assert!(!m.wants_behaviour(horizon_mods::BehaviourFamily::Breaking, "test:block"));
    assert!(!m.block_breaking(&p, BlockPos { x: 0, y: 64, z: 0 }, "test:trap"));
    assert_eq!(m.entries()[0].status, Status::Running);
}
#[test]
fn api29_demo_lectern_schedules_gold_above_after_twenty_ticks() {
    struct Lectern;
    impl horizon_mods::WorldView for Lectern {
        fn block(&self, _: &str, x: i32, y: i32, z: i32) -> Option<String> {
            (x == 0 && y == 64 && z == 0)
                .then(|| "minecraft:lectern[facing=north,has_book=false,powered=false]".into())
        }
        fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn has_dimension(&self, d: &str) -> bool {
            d == "minecraft:overworld"
        }
        fn check_state(&self, s: &str) -> Result<(), String> {
            if s == "minecraft:gold_block" {
                Ok(())
            } else {
                Err("invalid block".into())
            }
        }
    }
    let (mut m, p, pos) = setup();
    m.lend_world(Box::new(Lectern));
    assert!(m.block_interacted(
        &p,
        pos,
        "minecraft:lectern[facing=north,has_book=false,powered=false]",
        1,
        [0.5, 1.0, 0.5],
        horizon_mods::BehaviourHand::Main,
        "minecraft:air",
        false
    ));
    m.drain();
    m.set_snapshot(Snapshot {
        tick: 20,
        players: vec![p.clone()],
        ..Default::default()
    });
    m.flush_behaviour_ticks();
    assert!(m.drain().is_empty());
    m.set_snapshot(Snapshot {
        tick: 21,
        players: vec![p],
        ..Default::default()
    });
    m.flush_behaviour_ticks();
    assert!(m.drain().iter().any(|o| matches!(o, Output::SetBlocks {dimension,blocks,..} if dimension=="minecraft:overworld" && blocks.len()==1 && blocks[0].x==0 && blocks[0].y==65 && blocks[0].z==0 && blocks[0].state=="minecraft:gold_block")));
    m.flush_behaviour_ticks();
    assert!(m.drain().is_empty());
}
