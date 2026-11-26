use super::*;
use crate::worldgen::{feature_graph::FeatureDefinition, placement::PlacementModifier};
use serde_json::json;
use std::{convert::Infallible, fs, path::PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("horizon-decoration-ore-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, value: Value) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct StoneWorld {
    stone: BlockStateId,
    writes: Vec<([i32; 3], BlockStateId)>,
    biome: BiomeId,
}
impl FeatureWorld for StoneWorld {
    type Error = Infallible;
    fn block(&mut self, _: [i32; 3]) -> Result<BlockStateId, Infallible> {
        Ok(self.stone)
    }
    fn set_block(
        &mut self,
        position: [i32; 3],
        state: BlockStateId,
        _: i32,
    ) -> Result<bool, Infallible> {
        self.writes.push((position, state));
        Ok(true)
    }
}
impl PlacementContext for StoneWorld {
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-64, 384, 63)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(-64)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(319)
    }
    fn block_state(&mut self, _: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
        Ok(self.stone)
    }
    fn set_section_block(
        &mut self,
        position: [i32; 3],
        state: BlockStateId,
    ) -> Result<(), PlacementQueryError> {
        self.writes.push((position, state));
        Ok(())
    }
    fn biome_allows_feature(
        &mut self,
        _: [i32; 3],
        feature: FeatureIdentity,
    ) -> Result<bool, PlacementQueryError> {
        Ok(feature == FeatureIdentity(0))
    }
    fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Result<i32, PlacementQueryError> {
        Ok(128)
    }
}
impl DecorationWorld for StoneWorld {
    type Error = Infallible;
    fn min_section_y(&mut self) -> Result<i32, Infallible> {
        Ok(-4)
    }
    fn chunk_biomes(&mut self, _: ChunkPos) -> Result<Vec<BiomeId>, Infallible> {
        Ok(vec![self.biome])
    }
    fn set_currently_generating(&mut self, _: Option<FeatureIdentity>) -> Result<(), Infallible> {
        Ok(())
    }
}

#[test]
fn content_ore_supported_target_and_underground_decoration() {
    let f = Fixture::new();
    // Original synthetic registry and bundles, with no prepared resources.
    let names = [
        "minecraft:air",
        "minecraft:cave_air",
        "minecraft:void_air",
        "minecraft:stone",
        "minecraft:cobblestone",
        "minecraft:end_stone",
        "minecraft:obsidian",
        "minecraft:grass_block",
        "minecraft:mycelium",
        "minecraft:dirt",
        "minecraft:emerald_ore",
        "wgtest:glass_ore",
    ];
    let defaults: BTreeMap<_, _> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    f.write("facts.json", json!({"states":names.map(|n| json!([n,n.ends_with("air"),false,n,n])),"defaults":defaults}));
    f.write(
        "worldgen/block_flags.json",
        json!(names.map(|n| (n.ends_with("air"), false))),
    );
    let mut tags = serde_json::Map::new();
    for name in [
        "minecraft:blocks_motion_in_heightmap",
        "minecraft:blocks_motion_in_heightmap_no_leaves",
        "minecraft:uncarvable",
        "minecraft:stone_ore_replaceables",
    ] {
        tags.insert(
            name.into(),
            json!(json!({"values":["minecraft:stone"]}).to_string()),
        );
    }
    f.write("worldgen/block_tags.json", Value::Object(tags));
    for name in [
        "noises",
        "densities",
        "rules",
        "conditions",
        "settings",
        "carvers",
        "biome_tags",
        "world_presets",
    ] {
        f.write(&format!("worldgen/{name}.json"), json!({}));
    }
    let mut steps = vec![json!([]); 11];
    steps[6] = json!(["wgtest:glass_ore"]);
    let biome = json!({"temperature":0.5,"downfall":0.0,"has_precipitation":false,
        "carvers":[],"features":steps});
    f.write(
        "worldgen/biomes.json",
        json!({"wgtest:glass_flats":biome.to_string()}),
    );
    let ore = json!({"type":"minecraft:ore","targets":[{"target":{"predicate_type":"minecraft:tag_match","tag":"minecraft:stone_ore_replaceables"},"state":"wgtest:glass_ore"}],"size":9,"discard_chance_on_air_exposure":0});
    f.write(
        "worldgen/features.json",
        json!({"wgtest:glass_ore":ore.to_string()}),
    );
    let placed = json!({"feature":"wgtest:glass_ore","placement":[
        {"type":"minecraft:count","count":20},
        {"type":"minecraft:in_square"},
        {"type":"minecraft:height_range","height":{"type":"minecraft:uniform","min_inclusive":{"absolute":-64},"max_inclusive":{"absolute":64}}},
        {"type":"minecraft:biome"}
    ]});
    f.write(
        "worldgen/placed_features.json",
        json!({"wgtest:glass_ore":placed.to_string()}),
    );
    let registry = Box::leak(Box::new(crate::level::data::load_registry(&f.0).unwrap()));
    let data = GeneratorData::load(&f.0.join("worldgen"), registry).unwrap();
    let source =
        crate::worldgen::biome_source::BiomeSourceDefinition::Fixed("wgtest:glass_flats".into())
            .resolve(&BTreeMap::new())
            .unwrap();
    let decoration = Decoration::new(&data, &source).unwrap().unwrap();
    assert_eq!(decoration.names, ["wgtest:glass_ore"]);
    assert!(
        decoration.unsupported().is_empty(),
        "{:?}",
        decoration.unsupported()
    );
    let feature = decoration.features[0].as_ref().unwrap();
    assert!(matches!(*feature.feature, FeatureDefinition::Ore(_)));
    assert!(matches!(feature.placement[0], PlacementModifier::Count(_)));
    let biome = data.biome_id("wgtest:glass_flats").unwrap();
    assert_eq!(
        decoration.biome_features[biome.0 as usize],
        BTreeSet::from([FeatureIdentity(0)])
    );
    let target = registry.default_state("wgtest:glass_ore").unwrap();
    assert_ne!(
        target,
        registry.default_state("minecraft:emerald_ore").unwrap()
    );
    let mut world = StoneWorld {
        stone: registry.default_state("minecraft:stone").unwrap(),
        writes: vec![],
        biome,
    };
    let mut scheduled = Vec::new();
    let step = std::cell::Cell::new(0);
    decoration
        .plan
        .decorate(
            &mut world,
            ChunkPos::new(0, 0),
            42,
            11,
            |_, _, _, current| {
                step.set(current);
                Ok(())
            },
            |world, placer, random, id, origin| {
                scheduled.push((step.get(), id));
                // The production decoration callback uses this same placement path.
                Ok(placer
                    .place_with_biome_check(
                        &feature.placement,
                        world,
                        random,
                        origin,
                        id,
                        |world, random, position| {
                            feature
                                .feature
                                .place(world, random, position, decoration.blocks)
                        },
                    )
                    .unwrap())
            },
        )
        .unwrap();
    assert_eq!(scheduled, [(6, FeatureIdentity(0))]);
    assert!(!world.writes.is_empty());
    assert!(world.writes.iter().all(|(_, state)| *state == target));
}
