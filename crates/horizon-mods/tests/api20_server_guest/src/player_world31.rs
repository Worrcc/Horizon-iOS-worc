//! API 31: player setters, polled events and world effects, queued and refused.
use horizon_mod_sdk::server::{
    BlastKind, BlockChangePos, DustColor, ItemFilter, ParticleBurst, Player, WorldPoint, players,
    world,
};

fn p(x: f64, y: f64, z: f64) -> WorldPoint {
    WorldPoint { x, y, z }
}
fn burst(name: &str, count: u32) -> ParticleBurst {
    ParticleBurst {
        name: name.into(),
        at: p(0.0, 65.0, 0.0),
        delta: p(0.5, 0.5, 0.5),
        speed: 0.0,
        count,
        block: None,
        dust: None,
    }
}

pub fn run(player: &Player, mode: &str) -> Result<Option<String>, String> {
    let id = player.id.as_str();
    let dim = player.dimension.as_str();
    let sand = BlockChangePos { x: 0, y: 64, z: 0 };
    let sound = "minecraft:entity.experience_orb.pickup";
    match mode {
        "invalid" => {
            // Offline players.
            assert!(players::vitals("offline").is_none());
            assert!(players::effects("offline").is_none());
            assert!(players::set_health("offline", 1.0).is_err());
            assert!(players::hurt("offline", 1.0, "minecraft:generic").is_err());
            assert!(players::set_food("offline", 1, 0.0).is_err());
            assert!(players::give_experience("offline", 1).is_err());
            assert!(players::give_levels("offline", 1).is_err());
            assert!(players::clear_effects("offline").is_err());
            assert!(
                players::clear_items("offline", &ItemFilter::Item("minecraft:stone".into()), 1)
                    .is_err()
            );
            assert!(players::play_sound_to("offline", sound, 1.0, 1.0).is_err());
            // Health and damage.
            for health in [f32::NAN, f32::INFINITY, 0.0, -1.0, 1025.0] {
                assert!(players::set_health(id, health).is_err());
            }
            for amount in [f32::NAN, 0.0, 1025.0] {
                assert!(players::hurt(id, amount, "minecraft:generic").is_err());
            }
            assert!(players::hurt(id, 1.0, "generic").is_err());
            // Food, experience, levels.
            assert!(players::set_food(id, 21, 0.0).is_err());
            assert!(players::set_food(id, 10, 11.0).is_err());
            assert!(players::set_food(id, 10, f32::NAN).is_err());
            assert!(players::give_experience(id, 100_001).is_err());
            assert!(players::give_levels(id, 1001).is_err());
            assert!(players::give_levels(id, -1001).is_err());
            // Sound.
            assert!(players::play_sound_to(id, "minecraft:nope", 1.0, 1.0).is_err());
            assert!(players::play_sound_to(id, sound, 5.0, 1.0).is_err());
            assert!(players::play_sound_to(id, sound, 1.0, 0.1).is_err());
            assert!(players::play_sound_to(id, sound, f32::NAN, 1.0).is_err());
            // Clear items.
            assert!(players::clear_items(id, &ItemFilter::Item("bad id".into()), 1).is_err());
            assert!(players::clear_items(id, &ItemFilter::Tag("bad id".into()), 1).is_err());
            let big = "x".repeat(1025);
            assert!(players::clear_items(id, &ItemFilter::ModData(Some(big)), 1).is_err());
            // Particles.
            assert!(world::spawn_particles(dim, &burst("minecraft:flame", 257)).is_err());
            assert!(world::spawn_particles(dim, &burst("minecraft:nope", 1)).is_err());
            assert!(world::spawn_particles("x:none", &burst("minecraft:flame", 1)).is_err());
            let mut b = burst("minecraft:flame", 1);
            b.dust = Some(DustColor {
                rgb: 0xFF0000,
                scale: 1.0,
            });
            assert!(world::spawn_particles(dim, &b).is_err());
            let mut b = burst("minecraft:flame", 1);
            b.block = Some("minecraft:stone".into());
            assert!(world::spawn_particles(dim, &b).is_err());
            let mut b = burst("minecraft:flame", 1);
            b.at = p(f64::NAN, 0.0, 0.0);
            assert!(world::spawn_particles(dim, &b).is_err());
            // Explosions.
            for radius in [0.0, 8.5, f32::NAN] {
                assert!(
                    world::explode(dim, p(0.0, 64.0, 0.0), radius, false, BlastKind::Block)
                        .is_err()
                );
            }
            assert!(
                world::explode("x:none", p(0.0, 64.0, 0.0), 1.0, false, BlastKind::Block).is_err()
            );
            assert!(
                world::explode(dim, p(f64::NAN, 64.0, 0.0), 1.0, false, BlastKind::Keep).is_err()
            );
            // Falling blocks.
            let down = p(0.0, 0.4, 0.0);
            assert!(world::make_block_fall("x:none", sand, "minecraft:sand", down, false).is_err());
            assert!(world::make_block_fall(dim, sand, "minecraft:gravel", down, false).is_err());
            assert!(world::make_block_fall(dim, sand, "minecraft:air", down, false).is_err());
            let floor = BlockChangePos { x: 0, y: 63, z: 0 };
            assert!(world::make_block_fall(dim, floor, "minecraft:bedrock", down, false).is_err());
            let stairs = BlockChangePos { x: 1, y: 64, z: 0 };
            let state =
                "minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=true]";
            assert!(world::make_block_fall(dim, stairs, state, down, false).is_err());
            assert!(
                world::make_block_fall(dim, sand, "minecraft:sand", p(4.0, 0.0, 0.0), false)
                    .is_err()
            );
            let far = BlockChangePos { x: 99, y: 64, z: 0 };
            assert!(world::make_block_fall(dim, far, "minecraft:sand", down, false).is_err());
            assert!(world::fluid_flow("x:none", sand).is_none());
        }
        "queue" => {
            players::set_health(id, 10.0)?;
            players::hurt(id, 2.0, "minecraft:generic")?;
            players::set_food(id, 10, 5.0)?;
            players::give_experience(id, 7)?;
            players::give_levels(id, 2)?;
            players::clear_effects(id)?;
            players::play_sound_to(id, sound, 1.5, 0.75)?;
            assert_eq!(
                players::clear_items(id, &ItemFilter::Item("minecraft:stone".into()), 5)?,
                3
            );
            let mut flame = burst("minecraft:flame", 12);
            flame.speed = 0.5;
            world::spawn_particles(dim, &flame)?;
            world::explode(dim, p(0.0, 64.0, 0.0), 3.0, true, BlastKind::Tnt)?;
            world::make_block_fall(dim, sand, "minecraft:sand", p(0.0, 0.4, 0.0), true)?;
        }
        "caps" => {
            for _ in 0..64 {
                players::set_health(id, 10.0)?;
            }
            assert!(players::set_health(id, 10.0).is_err());
            assert!(
                players::hurt(id, 1.0, "minecraft:generic").is_err(),
                "one shared budget"
            );
            for _ in 0..4 {
                world::explode(dim, p(0.0, 64.0, 0.0), 1.0, false, BlastKind::Keep)?;
            }
            assert!(world::explode(dim, p(0.0, 64.0, 0.0), 1.0, false, BlastKind::Keep).is_err());
            for _ in 0..16 {
                world::make_block_fall(dim, sand, "minecraft:sand", p(0.0, 0.4, 0.0), false)?;
            }
            assert!(
                world::make_block_fall(dim, sand, "minecraft:sand", p(0.0, 0.4, 0.0), false)
                    .is_err()
            );
            for _ in 0..16 {
                world::spawn_particles(dim, &burst("minecraft:flame", 256))?;
            }
            assert!(world::spawn_particles(dim, &burst("minecraft:flame", 1)).is_err());
            let stone = ItemFilter::Item("minecraft:stone".into());
            for _ in 0..64 {
                players::clear_items(id, &stone, 1)?;
            }
            assert!(players::clear_items(id, &stone, 1).is_err());
        }
        "reads" => {
            let deaths: Vec<String> = players::deaths()
                .iter()
                .map(|d| {
                    format!(
                        "{}@{}({},{},{}){}",
                        d.player,
                        d.dimension,
                        d.at.x,
                        d.at.y,
                        d.at.z,
                        d.cause.as_deref().unwrap_or("-")
                    )
                })
                .collect();
            let respawns: Vec<String> = players::respawns()
                .iter()
                .map(|r| {
                    format!(
                        "{}@{}({},{},{})",
                        r.player, r.dimension, r.at.x, r.at.y, r.at.z
                    )
                })
                .collect();
            let vitals = players::vitals(id)
                .map(|v| {
                    format!(
                        "{}/{}/{}/{}/{}",
                        v.food, v.saturation, v.xp_level, v.xp_progress, v.xp_total
                    )
                })
                .unwrap_or_default();
            let effects: Vec<String> = players::effects(id)
                .unwrap_or_default()
                .iter()
                .map(|e| format!("{}/{}/{}", e.id, e.ticks, e.amplifier))
                .collect();
            let flow = world::fluid_flow(dim, BlockChangePos { x: 2, y: 64, z: 0 })
                .map(|f| format!("{},{},{}", f.x, f.y, f.z))
                .unwrap_or_else(|| "none".into());
            players::tell(
                id,
                &format!(
                    "deaths={deaths:?} respawns={respawns:?} vitals={vitals} effects={effects:?} flow={flow}"
                ),
            );
        }
        _ => return Err("unknown pw31 mode".into()),
    }
    Ok(Some("pw31 checked".into()))
}
