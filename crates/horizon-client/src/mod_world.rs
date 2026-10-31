//! Read-only server-mod world queries: vanilla daylight, weather and sky visibility.
use crate::{
    chunk::MIN_Y,
    chunk_map::ChunkMap,
    environment::{self, Environment},
    world::World,
};
use horizon_mods::WorldTime;

/// All dimensions share the Overworld's clock and weather data. `isBrightOutside`
/// additionally excludes fixed-time dimensions; rain and thunder use Level's thresholds.
pub fn time(world: &World, overworld: &Environment, dimension: &str) -> WorldTime {
    let (rain, thunder) = world.weather.levels(1.0);
    let mut sky = overworld.sample(world.day_time, None, None);
    environment::apply_weather(&mut sky, rain, thunder);
    let darken = (15.0 - 15.0 * sky.sky_light_level).round() as i32;
    WorldTime {
        day_time: world.day_time as u64,
        game_time: world.game_time as u64,
        is_day: dimension == "minecraft:overworld" && darken < 4,
        raining: rain > 0.2,
        thundering: dimension != "minecraft:the_nether" && thunder > 0.9,
    }
}

/// `Level.canSeeSky`: the raw sky light layer, never the sun-adjusted brightness
/// or block light. Reads loaded chunks only and respects each dimension's build height.
pub fn can_see_sky(chunks: &ChunkMap, dimension: &str, x: i32, y: i32, z: i32) -> bool {
    let (min, max) = match dimension {
        "minecraft:overworld" => (MIN_Y, 320),
        "minecraft:the_end" => (0, 256),
        _ => return false,
    };
    let Some(chunk) = chunks.get(&(x >> 4, z >> 4)) else {
        return false;
    };
    y >= min && (y >= max || chunk.light(x & 15, y, z & 15) >> 4 == 15)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunk::{Chunk, Light};
    use horizon_world::registry::BlockStateId;
    use std::sync::Arc;

    #[test]
    fn sky_visibility_reads_sky_light_and_dimension_bounds_in_loaded_chunks() {
        let mut chunks = ChunkMap::default();
        let mut chunk = Chunk::empty(-1, 0);
        for section in &mut chunk.sections {
            Arc::make_mut(section).light = Light::Uniform(0xF0);
        }
        chunks.insert((-1, 0), Arc::new(chunk));
        let sees = |dim, y| can_see_sky(&chunks, dim, -1, y, 0);
        assert!(sees("minecraft:overworld", 64));
        assert!(sees("minecraft:the_end", 64));
        assert!(!sees("minecraft:the_nether", 64));
        assert!(!sees("missing", 64));
        assert!(!sees("minecraft:overworld", -65));
        assert!(sees("minecraft:overworld", -64));
        assert!(!sees("minecraft:the_end", -1));
        assert!(sees("minecraft:overworld", 320));
        assert!(sees("minecraft:the_end", 256));
        assert!(!can_see_sky(&chunks, "minecraft:overworld", 16, 400, 0));
        let chunk = Arc::make_mut(chunks.get_mut(&(-1, 0)).unwrap());
        for section in &mut chunk.sections {
            Arc::make_mut(section).light = Light::Uniform(0xEF);
        }
        // Block light 15 cannot make sky light 14 into canSeeSky.
        assert!(!can_see_sky(&chunks, "minecraft:overworld", -1, 64, 0));
    }

    #[test]
    fn world_time_uses_brightness_weather_thresholds_and_fixed_time_dimensions() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.local/horizon-client/26.3");
        if !root.join("data/minecraft/timeline/day.json").exists() {
            return;
        }
        let environment = Environment::load(&root).unwrap();
        let mut world = World::new(BlockStateId(0), 1);
        world.game_time = 90000;
        for (ticks, daylight) in [(1000, true), (6000, true), (18000, false), (25000, true)] {
            world.day_time = ticks;
            let t = time(&world, &environment, "minecraft:overworld");
            assert_eq!(
                (t.day_time, t.game_time, t.is_day),
                (ticks as u64, 90000, daylight)
            );
            for dim in ["minecraft:the_nether", "minecraft:the_end"] {
                let t = time(&world, &environment, dim);
                assert_eq!(
                    (t.day_time, t.game_time, t.is_day),
                    (ticks as u64, 90000, false)
                );
            }
        }
        world.day_time = 6000;
        for (rain, thunder, raining, thundering, day) in [
            (0.2, 1.0, false, false, true),
            (0.21, 1.0, true, false, true),
            (1.0, 0.0, true, false, true),
            (1.0, 0.9, true, false, false),
            (1.0, 1.0, true, true, false),
        ] {
            world.weather.rain_level = (rain, rain);
            world.weather.thunder_level = (thunder, thunder);
            let t = time(&world, &environment, "minecraft:overworld");
            assert_eq!(
                (t.raining, t.thundering, t.is_day),
                (raining, thundering, day)
            );
            assert!(!time(&world, &environment, "minecraft:the_nether").thundering);
        }
        // The pinned 26.3 day timeline's last/first bright ticks at dusk and dawn.
        world.weather = Default::default();
        for (ticks, day) in [(12440, true), (12441, false), (23559, false), (23560, true)] {
            world.day_time = ticks;
            assert_eq!(
                time(&world, &environment, "minecraft:overworld").is_day,
                day,
                "tick {ticks}"
            );
        }
    }
}
