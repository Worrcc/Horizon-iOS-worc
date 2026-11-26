//! API 31: player setters and world effects: budgets and validators.
use super::*;

pub(super) const PLAYER_CHANGES: usize = 64;
pub(super) const CLEARS: usize = 64;
pub(super) const PARTICLE_CALLS: usize = 64;
pub(super) const PARTICLES: usize = 4096;
pub(super) const BLASTS: usize = 4;
pub(super) const FALLS: usize = 16;
pub(super) const BLAST_RADIUS: f32 = 8.0;
pub(super) const FALL_SPEED: f64 = 3.9;
pub(super) const LISTED_EFFECTS: usize = 64;
pub(super) const EVENTS: usize = 256;
pub(super) const BLOCK_PARTICLES: [&str; 5] = [
    "minecraft:block",
    "minecraft:block_marker",
    "minecraft:falling_dust",
    "minecraft:dust_pillar",
    "minecraft:block_crumble",
];

/// Per-tick budgets: (tick, spent).
#[derive(Default)]
pub(super) struct Budgets {
    pub(super) changes: (Option<u64>, usize),
    pub(super) clears: (Option<u64>, usize),
    pub(super) particle_calls: (Option<u64>, usize),
    pub(super) particles: (Option<u64>, usize),
    pub(super) blasts: (Option<u64>, usize),
    pub(super) falls: (Option<u64>, usize),
}

/// Charge `n` against this tick's `limit`; all or nothing.
pub(super) fn spend(budget: &mut (Option<u64>, usize), tick: u64, n: usize, limit: usize) -> bool {
    if budget.0 != Some(tick) {
        *budget = (Some(tick), 0);
    }
    match budget.1.checked_add(n) {
        Some(total) if total <= limit => {
            budget.1 = total;
            true
        }
        _ => false,
    }
}

pub(super) fn check_health(h: f32) -> Result<(), String> {
    if h.is_finite() && h > 0.0 && h <= 1024.0 {
        Ok(())
    } else {
        Err("health must be finite, above 0 and at most 1024".into())
    }
}

pub(super) fn check_hurt(amount: f32, damage_type: &str) -> Result<(), String> {
    if !(amount.is_finite() && amount > 0.0 && amount <= 1024.0) {
        return Err("hurt amount must be finite, above 0 and at most 1024".into());
    }
    if !resource_id(damage_type) {
        return Err("invalid damage type id".into());
    }
    Ok(())
}

pub(super) fn check_food(food: u32, saturation: f32) -> Result<(), String> {
    if food > 20 {
        return Err("food must be 0..20".into());
    }
    if !(saturation.is_finite() && saturation >= 0.0 && saturation <= food as f32) {
        return Err("saturation must be finite and within 0..food".into());
    }
    Ok(())
}

pub(super) fn check_points(points: u32) -> Result<(), String> {
    if points > 100_000 {
        return Err("at most 100000 experience points".into());
    }
    Ok(())
}

pub(super) fn check_levels(levels: i32) -> Result<(), String> {
    if !(-1000..=1000).contains(&levels) {
        return Err("levels must be -1000..1000".into());
    }
    Ok(())
}

pub(super) fn check_sound(volume: f32, pitch: f32) -> Result<(), String> {
    if !(volume.is_finite() && (0.0..=4.0).contains(&volume)) {
        return Err("sound volume must be 0..4".into());
    }
    if !(pitch.is_finite() && (0.5..=2.0).contains(&pitch)) {
        return Err("sound pitch must be 0.5..2".into());
    }
    Ok(())
}

fn point_finite(p: &WorldPoint) -> bool {
    p.x.is_finite() && p.y.is_finite() && p.z.is_finite()
}

pub(super) fn check_particles(
    burst: &ParticleBurst,
    resources: &EntityResources,
    check_state: impl Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    if !point_finite(&burst.at) || !point_finite(&burst.delta) {
        return Err("particle position and spread must be finite".into());
    }
    if [burst.delta.x, burst.delta.y, burst.delta.z]
        .iter()
        .any(|d| d.abs() > 64.0)
    {
        return Err("particle spread must be within 64 blocks".into());
    }
    if !(burst.speed.is_finite() && (0.0..=16.0).contains(&burst.speed)) {
        return Err("particle speed must be 0..16".into());
    }
    if burst.count > 256 {
        return Err("at most 256 particles per burst".into());
    }
    if let Some(block) = &burst.block {
        if !BLOCK_PARTICLES.contains(&burst.name.as_str()) {
            return Err(format!("{} takes no block state", burst.name));
        }
        if burst.dust.is_some() {
            return Err("a block particle takes no dust color".into());
        }
        check_state(block)
    } else if let Some(dust) = &burst.dust {
        if burst.name != "minecraft:dust" {
            return Err(format!("{} takes no dust color", burst.name));
        }
        if dust.rgb > 0xFF_FFFF {
            return Err("dust rgb must be 0xRRGGBB".into());
        }
        if !(dust.scale.is_finite() && (0.01..=4.0).contains(&dust.scale)) {
            return Err("dust scale must be 0.01..4".into());
        }
        Ok(())
    } else if resources.particles.contains_key(&burst.name) {
        Ok(())
    } else if BLOCK_PARTICLES.contains(&burst.name.as_str()) || burst.name == "minecraft:dust" {
        Err(format!("{} needs its options", burst.name))
    } else {
        Err(format!("unknown particle {}", burst.name))
    }
}

pub(super) fn check_blast(center: &WorldPoint, radius: f32) -> Result<(), String> {
    if !point_finite(center)
        || center.x.abs() >= 30_000_000.0
        || center.z.abs() >= 30_000_000.0
        || center.y.abs() >= 20_000_000.0
    {
        return Err("explosion centre must be finite and inside the world".into());
    }
    if !(radius.is_finite() && radius > 0.0 && radius <= BLAST_RADIUS) {
        return Err("explosion radius must be above 0 and at most 8".into());
    }
    Ok(())
}

pub(super) fn check_fall(expected: &str, velocity: &WorldPoint) -> Result<(), String> {
    let (id, props) = match expected.split_once('[') {
        Some((id, rest)) => (id, rest),
        None => (expected, ""),
    };
    if matches!(
        id,
        "minecraft:air"
            | "minecraft:cave_air"
            | "minecraft:void_air"
            | "minecraft:water"
            | "minecraft:lava"
    ) {
        return Err(format!("{id} cannot fall"));
    }
    if props
        .trim_end_matches(']')
        .split(',')
        .any(|p| p.trim() == "waterlogged=true")
    {
        return Err("a waterlogged block cannot fall".into());
    }
    if !point_finite(velocity)
        || [velocity.x, velocity.y, velocity.z]
            .iter()
            .any(|v| v.abs() > FALL_SPEED)
    {
        return Err("fall velocity must be finite, each axis within 3.9".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64, z: f64) -> WorldPoint {
        WorldPoint { x, y, z }
    }
    fn resources() -> EntityResources {
        let mut r = EntityResources::default();
        r.particles.insert("minecraft:flame".into(), 1);
        r
    }
    fn burst(name: &str) -> ParticleBurst {
        ParticleBurst {
            name: name.into(),
            at: p(0.0, 0.0, 0.0),
            delta: p(1.0, 1.0, 1.0),
            speed: 0.0,
            count: 1,
            block: None,
            dust: None,
        }
    }
    fn ok_state(_: &str) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn spend_rolls_over_per_tick_and_charges_n() {
        let mut b = (None, 0);
        assert!(spend(&mut b, 1, 3, 4));
        assert!(!spend(&mut b, 1, 2, 4), "all or nothing");
        assert_eq!(b.1, 3, "a refused charge keeps nothing");
        assert!(spend(&mut b, 1, 1, 4));
        assert!(!spend(&mut b, 1, 1, 4));
        assert!(spend(&mut b, 2, 4, 4), "a new tick resets");
        assert!(!spend(&mut b, 2, usize::MAX, 4));
    }

    #[test]
    fn health_edges() {
        assert!(check_health(0.5).is_ok());
        assert!(check_health(1024.0).is_ok());
        assert!(check_health(0.0).is_err());
        assert!(check_health(1024.001).is_err());
        assert!(check_health(-1.0).is_err());
        assert!(check_health(f32::NAN).is_err());
        assert!(check_health(f32::INFINITY).is_err());
    }

    #[test]
    fn hurt_edges() {
        assert!(check_hurt(1.0, "minecraft:generic").is_ok());
        assert!(check_hurt(1024.0, "minecraft:generic").is_ok());
        assert!(check_hurt(0.0, "minecraft:generic").is_err());
        assert!(check_hurt(1024.01, "minecraft:generic").is_err());
        assert!(check_hurt(f32::NAN, "minecraft:generic").is_err());
        assert!(check_hurt(1.0, "generic").is_err());
        assert!(check_hurt(1.0, "Minecraft:x").is_err());
        let long = format!("minecraft:{}", "a".repeat(250));
        assert!(check_hurt(1.0, &long).is_err());
    }

    #[test]
    fn food_edges() {
        assert!(check_food(0, 0.0).is_ok());
        assert!(check_food(20, 20.0).is_ok());
        assert!(check_food(21, 0.0).is_err());
        assert!(check_food(10, 10.5).is_err());
        assert!(check_food(10, -0.1).is_err());
        assert!(check_food(10, f32::NAN).is_err());
    }

    #[test]
    fn points_and_levels_edges() {
        assert!(check_points(0).is_ok());
        assert!(check_points(100_000).is_ok());
        assert!(check_points(100_001).is_err());
        assert!(check_levels(-1000).is_ok());
        assert!(check_levels(1000).is_ok());
        assert!(check_levels(-1001).is_err());
        assert!(check_levels(1001).is_err());
    }

    #[test]
    fn sound_edges() {
        assert!(check_sound(0.0, 0.5).is_ok());
        assert!(check_sound(4.0, 2.0).is_ok());
        assert!(check_sound(4.01, 1.0).is_err());
        assert!(check_sound(-0.1, 1.0).is_err());
        assert!(check_sound(1.0, 0.49).is_err());
        assert!(check_sound(1.0, 2.01).is_err());
        assert!(check_sound(f32::NAN, 1.0).is_err());
        assert!(check_sound(1.0, f32::NAN).is_err());
    }

    #[test]
    fn particle_edges() {
        let r = resources();
        assert!(check_particles(&burst("minecraft:flame"), &r, ok_state).is_ok());
        assert!(check_particles(&burst("minecraft:nope"), &r, ok_state).is_err());
        let mut b = burst("minecraft:flame");
        b.count = 256;
        assert!(check_particles(&b, &r, ok_state).is_ok());
        b.count = 257;
        assert!(check_particles(&b, &r, ok_state).is_err());
        b.count = 0;
        b.speed = 16.0;
        assert!(check_particles(&b, &r, ok_state).is_ok());
        b.speed = 16.01;
        assert!(check_particles(&b, &r, ok_state).is_err());
        b.speed = f32::NAN;
        assert!(check_particles(&b, &r, ok_state).is_err());
        b.speed = 0.0;
        b.delta = p(64.0, -64.0, 0.0);
        assert!(check_particles(&b, &r, ok_state).is_ok());
        b.delta = p(64.01, 0.0, 0.0);
        assert!(check_particles(&b, &r, ok_state).is_err());
        b.delta = p(0.0, 0.0, 0.0);
        b.at = p(f64::NAN, 0.0, 0.0);
        assert!(check_particles(&b, &r, ok_state).is_err());
        // Dust.
        let mut d = burst("minecraft:dust");
        d.dust = Some(DustColor {
            rgb: 0xFF_FFFF,
            scale: 4.0,
        });
        assert!(check_particles(&d, &r, ok_state).is_ok());
        d.dust = Some(DustColor {
            rgb: 0x100_0000,
            scale: 1.0,
        });
        assert!(check_particles(&d, &r, ok_state).is_err());
        for scale in [0.009, 4.01, f32::NAN] {
            d.dust = Some(DustColor { rgb: 0, scale });
            assert!(check_particles(&d, &r, ok_state).is_err());
        }
        d.dust = Some(DustColor {
            rgb: 0,
            scale: 0.01,
        });
        assert!(check_particles(&d, &r, ok_state).is_ok());
        let mut f = burst("minecraft:flame");
        f.dust = d.dust;
        assert!(check_particles(&f, &r, ok_state).is_err(), "dust on flame");
        // Block particles.
        let mut blk = burst("minecraft:block");
        blk.block = Some("minecraft:stone".into());
        assert!(check_particles(&blk, &r, ok_state).is_ok());
        assert!(check_particles(&blk, &r, |_| Err("bad".into())).is_err());
        blk.dust = d.dust;
        assert!(check_particles(&blk, &r, ok_state).is_err());
        let mut fb = burst("minecraft:flame");
        fb.block = Some("minecraft:stone".into());
        assert!(
            check_particles(&fb, &r, ok_state).is_err(),
            "block on flame"
        );
        // Missing options.
        assert!(check_particles(&burst("minecraft:block"), &r, ok_state).is_err());
        assert!(check_particles(&burst("minecraft:dust"), &r, ok_state).is_err());
    }

    #[test]
    fn blast_edges() {
        let c = p(0.0, 64.0, 0.0);
        assert!(check_blast(&c, 0.1).is_ok());
        assert!(check_blast(&c, 8.0).is_ok());
        assert!(check_blast(&c, 0.0).is_err());
        assert!(check_blast(&c, 8.01).is_err());
        assert!(check_blast(&c, f32::NAN).is_err());
        assert!(check_blast(&p(29_999_999.0, 0.0, -29_999_999.0), 1.0).is_ok());
        assert!(check_blast(&p(30_000_000.0, 0.0, 0.0), 1.0).is_err());
        assert!(check_blast(&p(0.0, 0.0, -30_000_000.0), 1.0).is_err());
        assert!(check_blast(&p(0.0, 20_000_000.0, 0.0), 1.0).is_err());
        assert!(check_blast(&p(f64::NAN, 0.0, 0.0), 1.0).is_err());
    }

    #[test]
    fn fall_edges() {
        let v = p(0.0, 0.4, 0.0);
        assert!(check_fall("minecraft:sand", &v).is_ok());
        assert!(check_fall("minecraft:oak_stairs[facing=north,waterlogged=false]", &v).is_ok());
        for id in ["air", "cave_air", "void_air", "water", "lava"] {
            assert!(check_fall(&format!("minecraft:{id}"), &v).is_err());
        }
        assert!(check_fall("minecraft:water[level=0]", &v).is_err());
        assert!(check_fall("minecraft:oak_stairs[facing=north,waterlogged=true]", &v).is_err());
        assert!(check_fall("minecraft:sand", &p(3.9, -3.9, 3.9)).is_ok());
        assert!(check_fall("minecraft:sand", &p(3.9001, 0.0, 0.0)).is_err());
        assert!(check_fall("minecraft:sand", &p(0.0, 0.0, -4.0)).is_err());
        assert!(check_fall("minecraft:sand", &p(f64::NAN, 0.0, 0.0)).is_err());
    }
}
