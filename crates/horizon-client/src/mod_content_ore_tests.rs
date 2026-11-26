use super::*;
use horizon_mods::Package;
use horizon_world::{
    level::data::load_registry,
    registry::BlockStateJsonCodec,
    worldgen::{
        block_feature::FeatureBlockBindings, feature_catalog::FeatureCatalog,
        feature_graph::FeatureDefinition, tags::TagDefinitions,
    },
};
use serde_json::{Value, json};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!("horizon-content-ore-{}", std::process::id()));
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

#[test]
fn content_ore_package_overlay_and_generator_lookup() {
    let f = Fixture::new();
    let names = [
        "minecraft:air",
        "minecraft:cave_air",
        "minecraft:void_air",
        "minecraft:emerald_ore",
    ];
    let defaults: BTreeMap<_, _> = names.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    f.write("base/facts.json", json!({"states":names.map(|n| json!([n,n.ends_with("air"),false,n,n])),"defaults":defaults}));
    f.write(
        "base/gameplay-facts.json",
        json!({"states":names.map(|n| json!({"name":n,"item":n}))}),
    );
    f.write(
        "base/items.json",
        json!({"minecraft:emerald_ore":{"components":{"minecraft:max_stack_size":64}}}),
    );
    f.write("base/horizon-resources.json", json!({}));
    f.write(
        "base/registries.json",
        json!({
            "minecraft:block":{"entries":{"minecraft:emerald_ore":{"protocol_id":3}}},
            "minecraft:item":{"entries":{"minecraft:emerald_ore":{"protocol_id":0}}}
        }),
    );
    for bundle in ["features", "placed_features", "biomes"] {
        f.write(&format!("base/worldgen/{bundle}.json"), json!({}));
    }
    f.write(
        "base/worldgen/block_flags.json",
        json!([[true, false], [true, false], [true, false], [false, false]]),
    );
    let root = f.0.join("mod");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("horizon-mod.toml"), "id = \"wgtest\"\nname = \"Ore Test\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n").unwrap();
    f.write(
        "mod/data/wgtest/horizon/block/glass_ore.json",
        json!({"template":"minecraft:emerald_ore","item":true}),
    );
    // An original synthetic model binding avoids any prepared asset dependency.
    f.write(
        "mod/assets/wgtest/blockstates/glass_ore.json",
        json!({"variants":{"":{"model":"wgtest:block/glass_ore"}}}),
    );
    let ore = json!({"type":"minecraft:ore","targets":[{"target":{"predicate_type":"minecraft:block_match","block":"minecraft:emerald_ore"},"state":"wgtest:glass_ore"}],"size":9,"discard_chance_on_air_exposure":0});
    let placed = json!({"feature":"wgtest:glass_ore","placement":[
        {"type":"minecraft:count","count":20},
        {"type":"minecraft:in_square"},
        {"type":"minecraft:height_range","height":{"type":"minecraft:uniform","min_inclusive":{"absolute":-64},"max_inclusive":{"absolute":64}}},
        {"type":"minecraft:biome"}
    ]});
    let mut steps = vec![json!([]); 11];
    steps[6] = json!(["wgtest:glass_ore"]);
    f.write(
        "mod/data/wgtest/worldgen/configured_feature/glass_ore.json",
        ore.clone(),
    );
    f.write(
        "mod/data/wgtest/worldgen/placed_feature/glass_ore.json",
        placed.clone(),
    );
    f.write(
        "mod/data/wgtest/worldgen/biome/glass_flats.json",
        json!({"features":steps}),
    );
    let package = Package::read(&root).unwrap();
    let content = Content::read(&root, &package.manifest).unwrap();
    assert!(content.blocks[0].item);
    let overlay = resource_root(
        &f.0.join("base"),
        &[ContentMod {
            id: package.manifest.id,
            content,
        }],
        &f.0.join("cache"),
    )
    .unwrap();
    let registry = load_registry(&overlay).unwrap();
    let state = registry.default_state("wgtest:glass_ore").unwrap();
    assert_eq!(state.0, 4);
    assert_eq!(
        registry.default_state("minecraft:emerald_ore").unwrap().0,
        3
    );
    let codec = BlockStateJsonCodec::new(&registry, Default::default()).unwrap();
    assert_eq!(codec.decode(r#""wgtest:glass_ore""#, false).unwrap(), state);
    let items: Value =
        serde_json::from_slice(&fs::read(overlay.join("items.json")).unwrap()).unwrap();
    assert_eq!(items["wgtest:glass_ore"], items["minecraft:emerald_ore"]);
    let flags: Vec<(bool, bool)> =
        serde_json::from_slice(&fs::read(overlay.join("worldgen/block_flags.json")).unwrap())
            .unwrap();
    assert_eq!(flags.len(), registry.len());
    assert_eq!(flags[4], flags[3]);
    let bundle = |name: &str| -> BTreeMap<String, String> {
        serde_json::from_slice(&fs::read(overlay.join(format!("worldgen/{name}.json"))).unwrap())
            .unwrap()
    };
    let features = bundle("features");
    let placed_features = bundle("placed_features");
    assert_eq!(
        serde_json::from_str::<Value>(&features["wgtest:glass_ore"]).unwrap(),
        ore
    );
    assert_eq!(
        serde_json::from_str::<Value>(&placed_features["wgtest:glass_ore"]).unwrap(),
        placed
    );
    let biome: Value = serde_json::from_str(&bundle("biomes")["wgtest:glass_flats"]).unwrap();
    assert_eq!(biome["features"][6], json!(["wgtest:glass_ore"]));
    let catalog =
        FeatureCatalog::from_resources(&features, &placed_features, &TagDefinitions::default())
            .unwrap();
    let bindings = FeatureBlockBindings::new(&registry, Default::default()).unwrap();
    let decoded = catalog
        .placed_feature_with_blocks(r#""wgtest:glass_ore""#, &bindings)
        .unwrap();
    assert!(matches!(*decoded.feature, FeatureDefinition::Ore(_)));
}
