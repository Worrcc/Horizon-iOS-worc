//! Read-only server-mod world queries: vanilla daylight, weather, sky visibility and heights.
use crate::{
    chunk::MIN_Y,
    chunk_map::ChunkMap,
    environment::{self, Environment},
    world::World,
};
use horizon_mods::WorldTime;
use horizon_world::{
    heightmap::{HeightmapState, HeightmapType},
    registry::BlockStateId,
    worldgen::terrain::TerrainBlockStates,
};

/// OCEAN_FLOOR first-available heights in local z * 16 + x order, for chunk coordinates.
/// Loaded chunks retain WORLD_SURFACE rather than OCEAN_FLOOR: scan down from
/// that cached map using the level's facts, as the game's `column_height` does.
pub fn heightmap(
    chunks: &ChunkMap,
    x: i32,
    z: i32,
    properties: &impl TerrainBlockStates,
) -> Option<Vec<i32>> {
    let chunk = chunks.get(&(x, z))?;
    let mut heights = Vec::with_capacity(256);
    for z in 0..16 {
        for x in 0..16 {
            let surface = chunk.world_surface(x, z, |s| {
                properties.heightmap_state(BlockStateId(s)).is_air()
            });
            let height = (chunk.min_y()..surface)
                .rev()
                .find(|&y| {
                    HeightmapType::OceanFloor
                        .is_opaque(properties.heightmap_state(chunk.block(x, y, z)))
                })
                .map_or(chunk.min_y(), |y| y + 1);
            heights.push(height);
        }
    }
    Some(heights)
}

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
    fn heightmap_packs_ocean_floor_columns_and_follows_edits() {
        let properties = crate::assets::Properties::for_tests(&[
            (true, false, false, false),  // air
            (false, false, true, true),   // stone
            (false, true, false, false),  // water
            (false, false, true, false),  // leaves
            (false, false, false, false), // grass
        ]);
        let mut chunk = Chunk::empty(-2, 3);
        for z in 0..16 {
            for x in 0..16 {
                let y = MIN_Y + z * 16 + x;
                chunk.set(x, y, z, 1);
                chunk.set(x, y + 1, z, 2);
                chunk.set(x, y + 2, z, 4);
            }
        }
        // Leaves count, water and plants do not. Empty columns use min Y.
        chunk.set(1, 300, 2, 3);
        for y in MIN_Y..MIN_Y + 3 {
            chunk.set(0, y, 0, 0);
        }
        let mut chunks = ChunkMap::default();
        chunks.insert((-2, 3), Arc::new(chunk));
        let heights = heightmap(&chunks, -2, 3, &properties).unwrap();
        assert_eq!(heights.len(), 256);
        for (index, &height) in heights.iter().enumerate() {
            let expected = match index {
                0 => MIN_Y,
                33 => 301,
                _ => MIN_Y + index as i32 + 1,
            };
            assert_eq!(height, expected, "column {index}");
        }
        assert!(heightmap(&chunks, -32, 48, &properties).is_none());
        assert!(heightmap(&chunks, 3, -2, &properties).is_none());
        let chunk = Arc::make_mut(chunks.get_mut(&(-2, 3)).unwrap());
        chunk.set(1, 300, 2, 0);
        chunk.set(15, 319, 15, 1);
        let heights = heightmap(&chunks, -2, 3, &properties).unwrap();
        assert_eq!(heights[33], -30);
        assert_eq!(heights[255], 320);
    }

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
