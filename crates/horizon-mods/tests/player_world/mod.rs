use super::*;
use horizon_mods::{
    ActiveEffect, BlastKind, BlockChangePos, EntityResources, EntitySound, ItemFilter, PlayerDeath,
    PlayerInventories, PlayerPoint, PlayerRespawn, PlayerVitals, WorldPoint, WorldView,
};
use std::sync::{Arc, Mutex};

struct View;
impl WorldView for View {
    fn block(&self, dim: &str, x: i32, y: i32, z: i32) -> Option<String> {
        if !self.has_dimension(dim) {
            return None;
        }
        match (x, y, z) {
            (0, 64, 0) => Some("minecraft:sand".into()),
            (0, 63, 0) => Some("minecraft:bedrock".into()),
            (1, 64, 0) => Some(
                "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=true]"
                    .into(),
            ),
            _ => None,
        }
    }
    fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
        None
    }
    fn has_dimension(&self, dim: &str) -> bool {
        dim == "minecraft:overworld"
    }
    fn check_state(&self, state: &str) -> Result<(), String> {
        if state.starts_with("minecraft:") {
            Ok(())
        } else {
            Err("unknown block".into())
        }
    }
    fn block_has_tag(&self, block: &str, tag: &str) -> bool {
        block == "minecraft:bedrock" && tag == "minecraft:wither_immune"
    }
    fn fluid_flow(&self, dim: &str, pos: BlockChangePos) -> Option<[f64; 3]> {
        (self.has_dimension(dim) && (pos.x, pos.y, pos.z) == (2, 64, 0)).then_some([0.0, 0.0, 0.5])
    }
}

type Calls = Arc<Mutex<Vec<(String, ItemFilter, String, u32)>>>;
struct Inventories(Calls);
impl PlayerInventories for Inventories {
    fn take_item(&mut self, _: &str, _: &str, count: u32) -> Result<u32, String> {
        Ok(count)
    }
    fn give_item(&mut self, _: &str, _: &str, _: u32) -> Result<(), String> {
        Ok(())
    }
    fn clear_items(
        &mut self,
        player: &str,
        filter: &ItemFilter,
        mod_id: &str,
        max: u32,
    ) -> Result<u32, String> {
        let _ = max;
        self.0
            .lock()
            .unwrap()
            .push((player.into(), filter.clone(), mod_id.into(), max));
        Ok(3)
    }
}

fn pt(x: f64, y: f64, z: f64) -> PlayerPoint {
    PlayerPoint { x, y, z }
}
fn alice() -> ServerPlayer {
    ServerPlayer {
        id: "alice".into(),
        name: "Alice".into(),
        x: 0.,
        y: 64.,
        z: 0.,
        dimension: "minecraft:overworld".into(),
        game_mode: GameMode::Survival,
        health: 20.,
    }
}
fn snapshot(tick: u64) -> Snapshot {
    let mut resources = EntityResources::default();
    resources.particles.insert("minecraft:flame".into(), 1);
    resources
        .sounds
        .insert("minecraft:entity.experience_orb.pickup".into());
    Snapshot {
        tick,
        players: vec![alice()],
        entity_resources: Arc::new(resources),
        player_deaths: vec![PlayerDeath {
            player: "alice".into(),
            dimension: "minecraft:overworld".into(),
            at: pt(1.0, 64.0, -2.5),
            cause: Some("death.attack.lava".into()),
        }]
        .into(),
        player_respawns: vec![PlayerRespawn {
            player: "alice".into(),
            dimension: "minecraft:the_nether".into(),
            at: pt(8.0, 70.0, 8.0),
        }]
        .into(),
        player_vitals: vec![(
            "alice".to_owned(),
            PlayerVitals {
                food: 17,
                saturation: 3.5,
                xp_level: 4,
                xp_progress: 0.25,
                xp_total: 40,
            },
            vec![ActiveEffect {
                id: "minecraft:speed".into(),
                ticks: 600,
                amplifier: 1,
            }],
        )]
        .into(),
        ..Default::default()
    }
}
fn setup() -> (Mods, ServerPlayer, Calls) {
    let mut mods = load("horizon_mods_test_api20_server_guest.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    let p = alice();
    mods.set_snapshot(snapshot(1));
    mods.lend_world(Box::new(View));
    let calls = Calls::default();
    mods.lend_inventories(Box::new(Inventories(calls.clone())));
    mods.drain();
    (mods, p, calls)
}
fn command(mods: &mut Mods, p: &ServerPlayer, mode: &str) -> Vec<Output> {
    assert!(mods.server_command(p, &format!("pw31 {mode}")));
    assert_eq!(mods.entries()[0].status, Status::Running);
    let out = mods.drain();
    assert!(
        out.iter().any(
            |o| matches!(o, Output::Reply { text, error: false, .. } if text == "pw31 checked")
        ),
        "{out:?}"
    );
    out
}

#[test]
fn api31_invalid_input_queues_nothing() {
    let (mut m, p, calls) = setup();
    let out = command(&mut m, &p, "invalid");
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(calls.lock().unwrap().is_empty());
}

#[test]
fn api31_queue_has_the_exact_outputs() {
    let (mut m, p, calls) = setup();
    let out = command(&mut m, &p, "queue");
    assert_eq!(out.len(), 11, "{out:?}");
    assert_eq!(
        out[0],
        Output::PlayerSetHealth {
            player: "alice".into(),
            health: 10.0
        }
    );
    assert_eq!(
        out[1],
        Output::PlayerHurt {
            player: "alice".into(),
            amount: 2.0,
            damage_type: "minecraft:generic".into()
        }
    );
    assert_eq!(
        out[2],
        Output::PlayerSetFood {
            player: "alice".into(),
            food: 10,
            saturation: 5.0
        }
    );
    assert_eq!(
        out[3],
        Output::PlayerGiveExperience {
            player: "alice".into(),
            points: 7
        }
    );
    assert_eq!(
        out[4],
        Output::PlayerGiveLevels {
            player: "alice".into(),
            levels: 2
        }
    );
    assert_eq!(
        out[5],
        Output::PlayerClearEffects {
            player: "alice".into(),
            ids: vec!["minecraft:speed".into()]
        }
    );
    assert_eq!(
        out[6],
        Output::PlaySoundTo {
            player: "alice".into(),
            sound: EntitySound {
                name: "minecraft:entity.experience_orb.pickup".into(),
                volume: 1.5,
                pitch: 0.75
            }
        }
    );
    // clear-items is synchronous: no Output, one inventory call.
    let Output::SpawnParticles {
        dimension,
        particles,
    } = &out[7]
    else {
        panic!("{out:?}")
    };
    assert_eq!(dimension, "minecraft:overworld");
    assert_eq!(particles.name, "minecraft:flame");
    assert_eq!(particles.count, 12);
    assert_eq!(particles.speed, 0.5);
    assert_eq!(
        particles.delta,
        WorldPoint {
            x: 0.5,
            y: 0.5,
            z: 0.5
        }
    );
    assert_eq!(
        out[8],
        Output::Explode {
            dimension: "minecraft:overworld".into(),
            center: WorldPoint {
                x: 0.0,
                y: 64.0,
                z: 0.0
            },
            radius: 3.0,
            fire: true,
            blast: BlastKind::Tnt
        }
    );
    assert_eq!(
        out[9],
        Output::MakeBlockFall {
            dimension: "minecraft:overworld".into(),
            pos: BlockChangePos { x: 0, y: 64, z: 0 },
            expected: "minecraft:sand".into(),
            velocity: WorldPoint {
                x: 0.0,
                y: 0.4,
                z: 0.0
            },
            hurts: true
        }
    );
    assert!(matches!(out[10], Output::Reply { .. }));
    assert_eq!(calls.lock().unwrap().len(), 1);
}

#[test]
fn api31_clear_items_passes_the_filter_and_this_mods_id() {
    let (mut m, p, calls) = setup();
    command(&mut m, &p, "queue");
    assert_eq!(
        *calls.lock().unwrap(),
        [(
            "alice".to_owned(),
            ItemFilter::Item("minecraft:stone".into()),
            "test_api20_server".to_owned(),
            5
        )]
    );
}

#[test]
fn api31_budgets_are_per_tick_and_reset_when_the_tick_advances() {
    let (mut m, p, calls) = setup();
    for tick in [1, 2] {
        m.set_snapshot(snapshot(tick));
        let out = command(&mut m, &p, "caps");
        let count = |f: fn(&Output) -> bool| out.iter().filter(|o| f(o)).count();
        assert_eq!(count(|o| matches!(o, Output::PlayerSetHealth { .. })), 64);
        assert_eq!(count(|o| matches!(o, Output::PlayerHurt { .. })), 0);
        assert_eq!(count(|o| matches!(o, Output::Explode { .. })), 4);
        assert_eq!(count(|o| matches!(o, Output::MakeBlockFall { .. })), 16);
        assert_eq!(count(|o| matches!(o, Output::SpawnParticles { .. })), 16);
    }
    assert_eq!(calls.lock().unwrap().len(), 128);
}

#[test]
fn api31_reads_match_the_snapshot() {
    let (mut m, p, _) = setup();
    let out = command(&mut m, &p, "reads");
    let text = out
        .iter()
        .find_map(|o| match o {
            Output::Tell { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("a tell");
    assert_eq!(
        text,
        "deaths=[\"alice@minecraft:overworld(1,64,-2.5)death.attack.lava\"] \
         respawns=[\"alice@minecraft:the_nether(8,70,8)\"] \
         vitals=17/3.5/4/0.25/40 effects=[\"minecraft:speed/600/1\"] flow=0,0,0.5"
    );
}

#[test]
fn phase_c_demo_loads_and_replies_to_vitals() {
    let mut mods = load("phase_c_demo.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    let p = alice();
    mods.set_snapshot(snapshot(1));
    mods.lend_world(Box::new(View));
    mods.drain();
    assert!(mods.server_command(&p, "pcd vitals"));
    assert_eq!(mods.entries()[0].status, Status::Running);
    let out = mods.drain();
    assert!(
        out.iter().any(|o| matches!(o, Output::Reply { text, error: false, .. }
            if text.starts_with("food 17 (sat 3.5), level 4") && text.contains("minecraft:speed x2"))),
        "{out:?}"
    );
    // The tick announces the death and welcomes the respawn.
    mods.set_snapshot(snapshot(2));
    mods.server_tick(2);
    let out = mods.drain();
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Broadcast { text, .. }
            if text == "Alice died (death.attack.lava)")),
        "{out:?}"
    );
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Tell { text, .. } if text == "welcome back")),
        "{out:?}"
    );
}
