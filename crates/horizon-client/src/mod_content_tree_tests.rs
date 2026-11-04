use super::*;
use horizon_mods::Package;
use horizon_world::{
    level::data::load_registry,
    worldgen::state_provider::{BlockStateProvider, StateProviderBindings},
};
use serde_json::{Value, json};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path =
            std::env::temp_dir().join(format!("horizon-content-tree-{}", std::process::id()));
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
fn content_tree_blocks_copy_template_states_and_providers() {
    let f = Fixture::new();
    let mut states: Vec<Value> = ["air", "cave_air", "void_air", "azure_bluet", "dirt"]
        .map(|n| json!({"id":format!("minecraft:{n}")}))
        .to_vec();
    for axis in ["x", "y", "z"] {
        states.push(json!({"id":"minecraft:oak_log","properties":{"axis":axis}}));
    }
    for distance in 1..=7 {
        for persistent in ["false", "true"] {
            for waterlogged in ["false", "true"] {
                states.push(json!({"id":"minecraft:oak_leaves","properties":{"distance":distance.to_string(),"persistent":persistent,"waterlogged":waterlogged}}));
            }
        }
    }
    let mut defaults = BTreeMap::new();
    for (i, s) in states.iter().enumerate() {
        defaults.entry(s["id"].as_str().unwrap()).or_insert(i);
    }
    f.write("base/facts.json", json!({"states":states.iter().map(|s| json!([s["id"],s["id"].as_str().unwrap().ends_with("air"),false,s["id"],s])).collect::<Vec<_>>(),"defaults":defaults}));
    f.write("base/gameplay-facts.json", json!({"states":states.iter().map(|s| json!({"name":s["id"],"item":s["id"]})).collect::<Vec<_>>()}));
    let templates = [
        "minecraft:oak_log",
        "minecraft:oak_leaves",
        "minecraft:azure_bluet",
    ];
    let entries: BTreeMap<_, _> = templates
        .iter()
        .enumerate()
        .map(|(i, n)| (*n, json!({"protocol_id":i})))
        .collect();
    f.write(
        "base/registries.json",
        json!({"minecraft:block":{"entries":entries},"minecraft:item":{"entries":entries}}),
    );
    f.write(
        "base/items.json",
        json!(
            templates
                .iter()
                .map(|n| (*n, json!({"components":{"minecraft:max_stack_size":64}})))
                .collect::<BTreeMap<_, _>>()
        ),
    );
    f.write("base/horizon-resources.json", json!({}));
    for name in ["features", "placed_features", "biomes"] {
        f.write(&format!("base/worldgen/{name}.json"), json!({}));
    }
    f.write("base/worldgen/block_tags.json", json!({}));
    let root = f.0.join("mod");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("horizon-mod.toml"), "id = \"wgtest\"\nname = \"Vegetation Test\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n").unwrap();
    for (name, template) in ["glass_log", "glass_leaves", "glass_flower"]
        .into_iter()
        .zip(templates)
    {
        f.write(
            &format!("mod/data/wgtest/horizon/block/{name}.json"),
            json!({"template":template,"item":true}),
        );
        // Original model bindings keep the test independent of prepared assets.
        f.write(
            &format!("mod/assets/wgtest/blockstates/{name}.json"),
            json!({"variants":{"":{"model":format!("wgtest:block/{name}")}}}),
        );
    }
    for (tag, block) in [
        ("logs", "glass_log"),
        ("leaves", "glass_leaves"),
        ("prevents_nearby_leaf_decay", "glass_log"),
    ] {
        f.write(
            &format!("mod/data/minecraft/tags/block/{tag}.json"),
            json!({"replace":false,"values":[format!("wgtest:{block}")]}),
        );
    }
    let providers = [
        json!({"id":"wgtest:glass_log","properties":{"axis":"y"}}),
        json!({"id":"wgtest:glass_leaves","properties":{"distance":"7","persistent":"false","waterlogged":"false"}}),
        json!({"id":"wgtest:glass_flower"}),
    ];
    f.write("mod/data/wgtest/worldgen/configured_feature/glass_tree.json",json!({"type":"minecraft:tree","trunk_provider":providers[0],"foliage_provider":providers[1],
        "trunk_placer":{"type":"minecraft:straight_trunk_placer","base_height":5,"height_rand_a":0,"height_rand_b":0},
        "foliage_placer":{"type":"minecraft:blob_foliage_placer","radius":2,"offset":0,"height":3},
        "minimum_size":{"type":"minecraft:two_layers_feature_size","limit":1,"lower_size":0,"upper_size":1},
        "decorators":[],"ignore_vines":true,"below_trunk_provider":{"id":"minecraft:dirt"}}));
    f.write(
        "mod/data/wgtest/worldgen/configured_feature/glass_flower.json",
        json!({"type":"minecraft:simple_block","to_place":providers[2]}),
    );
    for name in ["glass_tree", "glass_flower"] {
        f.write(
            &format!("mod/data/wgtest/worldgen/placed_feature/{name}.json"),
            json!({"feature":format!("wgtest:{name}"),"placement":[{"type":"minecraft:biome"}]}),
        );
    }
    let mut steps = vec![json!([]); 11];
    // vegetal_decoration: the ninth of the eleven decoration steps.
    steps[9] = json!(["wgtest:glass_tree", "wgtest:glass_flower"]);
    f.write(
        "mod/data/wgtest/worldgen/biome/glass_flats.json",
        json!({"features":steps}),
    );
    let package = Package::read(&root).unwrap();
    let content = Content::read(&root, &package.manifest).unwrap();
    assert_eq!(content.blocks.len(), 3);
    assert!(content.blocks.iter().all(|b| b.item));
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
    for (name, template) in ["glass_log", "glass_leaves", "glass_flower"]
        .into_iter()
        .zip(templates)
    {
        let template_states = (0..states.len())
            .filter_map(|i| registry.state(horizon_world::registry::BlockStateId(i as u32)))
            .filter(|s| s.name == template)
            .collect::<Vec<_>>();
        let copies = (states.len()..registry.len())
            .filter_map(|i| registry.state(horizon_world::registry::BlockStateId(i as u32)))
            .filter(|s| s.name == format!("wgtest:{name}"))
            .collect::<Vec<_>>();
        assert_eq!(copies.len(), template_states.len());
        for (copy, original) in copies.iter().zip(template_states) {
            assert_eq!(copy.properties, original.properties);
        }
    }
    let bundle = |name| -> BTreeMap<String, String> {
        serde_json::from_slice(&fs::read(overlay.join(format!("worldgen/{name}.json"))).unwrap())
            .unwrap()
    };
    let tags = bundle("block_tags");
    let leaves: Value = serde_json::from_str(&tags["minecraft:leaves"]).unwrap();
    assert_eq!(leaves["values"], json!(["wgtest:glass_leaves"]));
    let features = bundle("features");
    let tree: Value = serde_json::from_str(&features["wgtest:glass_tree"]).unwrap();
    let flower: Value = serde_json::from_str(&features["wgtest:glass_flower"]).unwrap();
    let bindings =
        StateProviderBindings::new(&registry, Default::default(), &BTreeMap::new()).unwrap();
    for provider in [
        &tree["trunk_provider"],
        &tree["foliage_provider"],
        &flower["to_place"],
    ] {
        BlockStateProvider::from_json(&provider.to_string(), &bindings).unwrap();
    }
    let biome: Value = serde_json::from_str(&bundle("biomes")["wgtest:glass_flats"]).unwrap();
    assert_eq!(
        biome["features"][9],
        json!(["wgtest:glass_tree", "wgtest:glass_flower"])
    );
}
