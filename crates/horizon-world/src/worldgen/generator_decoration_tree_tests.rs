use super::*;
use crate::worldgen::feature_graph::FeatureDefinition;
use crate::{
    DATA_VERSION,
    nbt::{self, Tag},
};
use serde_json::json;
use std::{convert::Infallible, fs, path::PathBuf};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("horizon-decoration-tree-{}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, path: &str, value: Value) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    }
    fn nbt(&self, name: &str, tag: Tag) {
        let path = self.0.join(format!("worldgen/blocks/{name}.nbt"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            nbt::write_named(
                &nbt::NamedTag {
                    name: "".into(),
                    tag,
                },
                nbt::Limits::default(),
            )
            .unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

struct GrassWorld {
    air: BlockStateId,
    grass: BlockStateId,
    biome: BiomeId,
    writes: BTreeMap<[i32; 3], BlockStateId>,
}
impl GrassWorld {
    fn read(&self, p: [i32; 3]) -> BlockStateId {
        self.writes
            .get(&p)
            .copied()
            .unwrap_or(if p[1] < 64 { self.grass } else { self.air })
    }
}
impl FeatureWorld for GrassWorld {
    type Error = Infallible;
    fn block(&mut self, p: [i32; 3]) -> Result<BlockStateId, Infallible> {
        Ok(self.read(p))
    }
    fn set_block(&mut self, p: [i32; 3], state: BlockStateId, _: i32) -> Result<bool, Infallible> {
        self.writes.insert(p, state);
        Ok(true)
    }
}
impl PlacementContext for GrassWorld {
    fn generation(&self) -> WorldGenerationContext {
        WorldGenerationContext::new(-64, 384, 63)
    }
    fn min_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(-64)
    }
    fn max_y(&mut self) -> Result<i32, PlacementQueryError> {
        Ok(319)
    }
    fn height(&mut self, _: HeightmapType, _: i32, _: i32) -> Result<i32, PlacementQueryError> {
        Ok(64)
    }
    fn block_state(&mut self, p: [i32; 3]) -> Result<BlockStateId, PlacementQueryError> {
        Ok(self.read(p))
    }
    fn set_section_block(
        &mut self,
        p: [i32; 3],
        state: BlockStateId,
    ) -> Result<(), PlacementQueryError> {
        self.writes.insert(p, state);
        Ok(())
    }
    fn block_survives(
        &mut self,
        _: BlockStateId,
        p: [i32; 3],
    ) -> Result<bool, PlacementQueryError> {
        Ok(self.read([p[0], p[1] - 1, p[2]]) == self.grass)
    }
    fn biome_allows_feature(
        &mut self,
        _: [i32; 3],
        id: FeatureIdentity,
    ) -> Result<bool, PlacementQueryError> {
        Ok(id.0 < 2)
    }
}
impl DecorationWorld for GrassWorld {
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
fn content_tree_and_flower_decoration_steps() {
    let f = Fixture::new();
    // Original synthetic Cartesian state tables, including placement's required moss states.
    let mut states: Vec<Value> = [
        "air",
        "cave_air",
        "void_air",
        "stone",
        "cobblestone",
        "end_stone",
        "obsidian",
        "grass_block",
        "mycelium",
        "dirt",
    ]
    .map(|n| json!({"id":format!("minecraft:{n}")}))
    .to_vec();
    for axis in ["x", "y", "z"] {
        states.push(json!({"id":"wgtest:glass_log","properties":{"axis":axis}}));
    }
    for distance in 1..=7 {
        for persistent in ["false", "true"] {
            for waterlogged in ["false", "true"] {
                states.push(json!({"id":"wgtest:glass_leaves","properties":{"distance":distance.to_string(),"persistent":persistent,"waterlogged":waterlogged}}));
            }
        }
    }
    states.push(json!({"id":"wgtest:glass_flower"}));
    for bottom in ["false", "true"] {
        for n in ["none", "low", "tall"] {
            for e in ["none", "low", "tall"] {
                for s in ["none", "low", "tall"] {
                    for w in ["none", "low", "tall"] {
                        states.push(json!({"id":"minecraft:pale_moss_carpet","properties":{"bottom":bottom,"north":n,"east":e,"south":s,"west":w}}));
                    }
                }
            }
        }
    }
    let mut defaults = BTreeMap::new();
    for (id, state) in states.iter().enumerate() {
        defaults.entry(state["id"].as_str().unwrap()).or_insert(id);
    }
    f.write("facts.json", json!({"states":states.iter().map(|s| json!([s["id"],s["id"].as_str().unwrap().ends_with("air"),false,s["id"],s])).collect::<Vec<_>>(),"defaults":defaults}));
    let registry = Box::leak(Box::new(crate::level::data::load_registry(&f.0).unwrap()));
    let identities = Tag::List(
        (0..registry.len())
            .map(|i| registry.state(BlockStateId(i as u32)).unwrap().to_nbt())
            .collect(),
    );
    let empty = Tag::compound([
        ("x", Tag::LongArray(vec![0])),
        ("y", Tag::LongArray(vec![0])),
        ("z", Tag::LongArray(vec![0])),
        ("occupied", Tag::ByteArray(vec![])),
        ("cube", Tag::Byte(0)),
        ("cube_axes", Tag::Byte(0)),
        ("discrete_min", Tag::IntArray(vec![0; 3])),
        ("discrete_max", Tag::IntArray(vec![0; 3])),
    ]);
    let shapes = Tag::compound([
        ("DataVersion", Tag::Int(DATA_VERSION)),
        ("states", identities.clone()),
        ("shapes", Tag::List(vec![empty])),
        ("cached_shapes", Tag::IntArray(vec![0; registry.len()])),
    ]);
    f.nbt("cached-collision", shapes.clone());
    let mut support = shapes.clone();
    if let Tag::Compound(entries) = &mut support {
        entries.insert("Kind".into(), Tag::String("block_support".into()));
    }
    f.nbt("cached-support", support);
    f.nbt(
        "block-properties",
        Tag::compound([
            ("DataVersion", Tag::Int(DATA_VERSION)),
            ("states", identities),
            ("occlusion", shapes),
            (
                "blocks",
                Tag::Compound(
                    defaults
                        .keys()
                        .map(|n| {
                            (
                                (*n).into(),
                                Tag::Byte(if n.ends_with("air") { 6 } else { 2 }),
                            )
                        })
                        .collect(),
                ),
            ),
        ]),
    );
    for (name, value) in [
        ("survival-rules", json!({})),
        (
            "survival-extra-definitions",
            json!({"ignition_odds":{},"fence_gates":[]}),
        ),
        (
            "placement-definitions",
            json!({"double_plants":[],"mossy_carpets":[]}),
        ),
    ] {
        f.write(&format!("worldgen/blocks/{name}.json"), value);
    }
    f.write(
        "worldgen/block_flags.json",
        json!(
            states
                .iter()
                .map(|s| (s["id"].as_str().unwrap().ends_with("air"), false))
                .collect::<Vec<_>>()
        ),
    );
    let mut tags = serde_json::Map::new();
    for name in [
        "blocks_motion_in_heightmap",
        "blocks_motion_in_heightmap_no_leaves",
        "uncarvable",
    ] {
        tags.insert(
            format!("minecraft:{name}"),
            json!(json!({"values":["minecraft:stone"]}).to_string()),
        );
    }
    for (name, block) in [
        ("logs", "wgtest:glass_log"),
        ("prevents_nearby_leaf_decay", "wgtest:glass_log"),
        ("leaves", "wgtest:glass_leaves"),
    ] {
        tags.insert(
            format!("minecraft:{name}"),
            json!(json!({"values":[block]}).to_string()),
        );
    }
    f.write("worldgen/block_tags.json", json!(tags));
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
    let tree = json!({"type":"minecraft:tree",
        "trunk_provider":{"id":"wgtest:glass_log","properties":{"axis":"y"}},
        "trunk_placer":{"type":"minecraft:straight_trunk_placer","base_height":5,"height_rand_a":0,"height_rand_b":0},
        "foliage_provider":{"id":"wgtest:glass_leaves","properties":{"distance":"7","persistent":"false","waterlogged":"false"}},
        "foliage_placer":{"type":"minecraft:blob_foliage_placer","radius":2,"offset":0,"height":3},
        "minimum_size":{"type":"minecraft:two_layers_feature_size","limit":1,"lower_size":0,"upper_size":1},
        "decorators":[],"ignore_vines":true,"below_trunk_provider":{"id":"minecraft:dirt"}});
    let flower = json!({"type":"minecraft:simple_block","to_place":{"id":"wgtest:glass_flower"}});
    f.write(
        "worldgen/features.json",
        json!({"wgtest:glass_tree":tree.to_string(),"wgtest:glass_flower":flower.to_string()}),
    );
    let placed = |name: &str, count, flower: bool| {
        let mut placement = vec![
            json!({"type":"minecraft:count","count":count}),
            json!({"type":"minecraft:in_square"}),
            json!({"type":"minecraft:heightmap","heightmap":"MOTION_BLOCKING"}),
        ];
        if flower {
            placement.push(json!({"type":"minecraft:block_predicate_filter","predicate":{"type":"minecraft:all_of","predicates":[
            {"type":"minecraft:matching_blocks","blocks":"minecraft:air"},
            {"type":"minecraft:matching_blocks","offset":[0,-1,0],"blocks":"minecraft:grass_block"}]}}));
        }
        placement.push(json!({"type":"minecraft:biome"}));
        json!({"feature":name,"placement":placement}).to_string()
    };
    f.write("worldgen/placed_features.json", json!({"wgtest:glass_tree":placed("wgtest:glass_tree",2,false),"wgtest:glass_flower":placed("wgtest:glass_flower",32,true)}));
    // Exercise the requested fixture index and the checkout's vegetal_decoration index.
    for decoration_step in [8, 9] {
        let mut steps = vec![json!([]); 11];
        steps[decoration_step] = json!(["wgtest:glass_tree", "wgtest:glass_flower"]);
        f.write("worldgen/biomes.json", json!({"wgtest:glass_flats":json!({"temperature":0.5,"downfall":0.0,"has_precipitation":false,"carvers":[],"features":steps}).to_string()}));
        let data = GeneratorData::load(&f.0.join("worldgen"), registry).unwrap();
        let source = crate::worldgen::biome_source::BiomeSourceDefinition::Fixed(
            "wgtest:glass_flats".into(),
        )
        .resolve(&BTreeMap::new())
        .unwrap();
        let decoration = Decoration::new(&data, &source).unwrap().unwrap();
        assert_eq!(
            decoration.names,
            ["wgtest:glass_tree", "wgtest:glass_flower"]
        );
        assert!(
            decoration.unsupported().is_empty(),
            "{:?}",
            decoration.unsupported()
        );
        assert!(matches!(
            *decoration.features[0].as_ref().unwrap().feature,
            FeatureDefinition::Tree(_)
        ));
        assert!(matches!(
            *decoration.features[1].as_ref().unwrap().feature,
            FeatureDefinition::SimpleBlock(_)
        ));
        let biome = data.biome_id("wgtest:glass_flats").unwrap();
        assert_eq!(
            decoration.biome_features[biome.0 as usize],
            BTreeSet::from([FeatureIdentity(0), FeatureIdentity(1)])
        );
        let mut world = GrassWorld {
            air: registry.default_state("minecraft:air").unwrap(),
            grass: registry.default_state("minecraft:grass_block").unwrap(),
            biome,
            writes: BTreeMap::new(),
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
                    let feature = decoration.features[id.0 as usize].as_ref().unwrap();
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
        assert_eq!(
            scheduled,
            [
                (decoration_step, FeatureIdentity(0)),
                (decoration_step, FeatureIdentity(1))
            ]
        );
        let written = |name| {
            world
                .writes
                .values()
                .filter_map(|s| registry.state(*s))
                .filter(|s| s.name == name)
                .collect::<Vec<_>>()
        };
        let logs = written("wgtest:glass_log");
        assert!(!logs.is_empty());
        assert!(logs.iter().all(|s| s.properties["axis"] == "y"));
        let leaves = written("wgtest:glass_leaves");
        assert!(!leaves.is_empty());
        assert!(leaves.iter().all(
            |s| s.properties["persistent"] == "false" && s.properties["waterlogged"] == "false"
        ));
        assert!(
            leaves
                .iter()
                .any(|s| s.properties["distance"].parse::<u8>().unwrap() < 7)
        );
        assert!(!written("wgtest:glass_flower").is_empty());
    }
}
