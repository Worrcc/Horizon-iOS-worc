use super::*;
use crate::mod_content::{ContentMod, resource_root};
use horizon_mods::{Package, content::Content};
use serde_json::json;
use std::collections::BTreeMap;

// Original fixtures only: no prepared data or files under .local are read.
struct Fixture(PathBuf);

impl Fixture {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("horizon-biome-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let fixture = Self(dir);
        for (path, value) in [
            (
                "facts.json",
                json!({"states":[["minecraft:air"]],"defaults":{"minecraft:air":0}}),
            ),
            (
                "gameplay-facts.json",
                json!({"states":[{"name":"minecraft:air","item":"minecraft:air"}]}),
            ),
            ("items.json", json!({})),
            ("horizon-resources.json", json!({})),
            (
                "registries.json",
                json!({
                    "minecraft:item":{"entries":{"minecraft:air":{"protocol_id":0}}},
                    "minecraft:block":{"entries":{"minecraft:air":{"protocol_id":0}}}
                }),
            ),
            ("data/minecraft/worldgen/biome/plains.json", glass_flats()),
            (
                "worldgen/biomes.json",
                json!({"minecraft:plains":glass_flats().to_string()}),
            ),
            (
                "worldgen/presets.json",
                json!({"minecraft:overworld":json!([{
                    "coordinates":vec![[10000,10000];6],"offset":0,"biome":"minecraft:plains"
                }]).to_string()}),
            ),
        ] {
            fixture.write(&format!("base/{path}"), &value.to_string());
        }
        for name in ["grass", "foliage", "dry_foliage"] {
            let path = fixture.0.join(format!(
                "base/assets/minecraft/textures/colormap/{name}.png"
            ));
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), 256, 256);
            encoder.set_color(png::ColorType::Rgb);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&vec![128; 256 * 256 * 3])
                .unwrap();
        }
        fixture
    }

    fn write(&self, path: &str, text: &str) {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn overlay(&self, folder: &str, biome: Option<&Value>) -> PathBuf {
        self.write(&format!("{folder}/horizon-mod.toml"),
            "id = \"wgtest\"\nname = \"Worldgen Test\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n");
        // Keep this a nonempty content mod even without biome declarations.
        self.write(&format!("{folder}/assets/wgtest/lang/en_us.json"), "{}");
        if let Some(biome) = biome {
            self.write(
                &format!("{folder}/data/wgtest/worldgen/biome/glass_flats.json"),
                &biome.to_string(),
            );
            let parameters = |temperature, humidity, depth| {
                json!({
                    "temperature":temperature,"humidity":humidity,"continentalness":[-2,2],
                    "erosion":[-2,2],"depth":depth,"weirdness":[-2,2],"offset":0
                })
            };
            self.write(&format!("{folder}/data/wgtest/worldgen/biome_source_addition/glass_flats.json"),
                &json!({"type":"minecraft:multi_noise","horizon:preset":"minecraft:overworld","biomes":[
                    {"biome":"wgtest:glass_flats","parameters":parameters(json!([-2,2]),json!([-2,2]),json!(0.1))},
                    {"biome":"wgtest:glass_flats","parameters":parameters(json!([-0.45,0.55]),json!([-1,0.3]),json!([-0.3,0.05]))}
                ]}).to_string());
        }
        let dir = self.0.join(folder);
        let package = Package::read(&dir).unwrap();
        let content = Content::read(&dir, &package.manifest).unwrap();
        resource_root(
            &self.0.join("base"),
            &[ContentMod {
                id: package.manifest.id,
                content,
            }],
            &self.0.join(format!("cache-{folder}")),
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// Self-contained snapshot of the local test mod's original glass_flats data.
fn glass_flats() -> Value {
    json!({"temperature":0.5,"downfall":0.0,"has_precipitation":false,
        "effects":{"water_color":65535,"grass_color":16711935,"foliage_color":16711935,
            "dry_foliage_color":16711935,"fog_color":65535,"water_fog_color":65535,"sky_color":16711935},
        "attributes":{"minecraft:visual/fog_color":"#00ffff",
            "minecraft:visual/water_fog_color":"#00ffff","minecraft:visual/sky_color":"#ff00ff",
            "minecraft:gameplay/natural_mob_spawns":{"spawns_by_category":{},"spawn_costs":{}},
            "minecraft:gameplay/creature_world_gen_spawn_probability":0.0},
        "spawners":{},"spawn_costs":{},"carvers":[],"features":[]})
}

#[test]
fn content_biome_spawn_attributes_colours_and_climate() {
    use crate::mobs::spawn_selection::{Category, SpawnCost, weighted_pick};
    use horizon_core::{
        density::{ConstantSampler, SamplerContext},
        random::RandomSource,
    };
    use horizon_world::worldgen::{
        biome_source::{BiomeSourceCatalog, ResolvedBiomeSource},
        climate::ClimateSampler,
    };
    use std::sync::Arc;

    let fixture = Fixture::new("attributes");
    let mut biome = glass_flats();
    let root = fixture.overlay("empty", Some(&biome));
    let biomes = Biomes::load(&root).unwrap();
    let glass = biomes.get(biomes.id("wgtest:glass_flats"));
    assert_eq!(glass.name, "wgtest:glass_flats");
    assert!(glass.spawns.is_empty());
    assert!(glass.spawn_costs.is_empty());
    assert_eq!(glass.creature_probability, 0.0);

    // The shipped fixture is intentionally empty. Populate only this test's
    // copy to exercise both supported 26.3 natural-spawn attribute forms.
    let natural = json!({"spawns_by_category":{
        "creature":[{"type":"minecraft:sheep","weight":7,"count":2},
            {"type":"minecraft:pig","weight":3,"count":{"type":"minecraft:uniform","min_inclusive":1,"max_inclusive":4}}],
        "monster":[{"type":"minecraft:zombie","weight":11,"count":{"type":"minecraft:constant","value":3}}]},
        "spawn_costs":{"minecraft:zombie":{"energy_budget":1.25,"charge":0.5}}});
    for (folder, attribute) in [
        ("plain", natural.clone()),
        ("overlay", json!({"modifier":"overlay","argument":natural})),
    ] {
        biome["attributes"]["minecraft:gameplay/natural_mob_spawns"] = attribute;
        biome["attributes"]["minecraft:gameplay/creature_world_gen_spawn_probability"] =
            json!(0.25);
        // A vanilla-named biome with identical data takes the same loader path.
        fixture.write(
            "base/data/minecraft/worldgen/biome/plains.json",
            &biome.to_string(),
        );
        fixture.write(
            "base/worldgen/biomes.json",
            &json!({"minecraft:plains":biome.to_string()}).to_string(),
        );
        let root = fixture.overlay(folder, Some(&biome));
        let biomes = Biomes::load(&root).unwrap();
        assert_eq!(biomes.id("minecraft:plains"), 0);
        assert_eq!(biomes.id("wgtest:glass_flats"), 1);
        let glass = biomes.get(biomes.id("wgtest:glass_flats"));
        assert_eq!(glass.spawns.len(), 2);
        let entries = |category: Category| {
            glass.spawns[category.name()]
                .iter()
                .map(|e| (e.entity.as_str(), e.weight, e.min, e.max, e.constant))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            entries(Category::Creature),
            vec![
                ("minecraft:sheep", 7, 2, 2, true),
                ("minecraft:pig", 3, 1, 4, false)
            ]
        );
        assert_eq!(
            entries(Category::Monster),
            vec![("minecraft:zombie", 11, 3, 3, true)]
        );
        assert_eq!(glass.spawn_costs.len(), 1);
        assert_eq!(
            glass.spawn_costs["minecraft:zombie"],
            SpawnCost {
                energy_budget: 1.25,
                charge: 0.5
            }
        );
        assert_eq!(glass.creature_probability, 0.25);
        assert_eq!(
            (glass.temperature, glass.downfall, glass.precipitation),
            (0.5, 0.0, false)
        );
        assert_eq!(
            (
                biomes.grass(1, 0.0, 0.0),
                glass.foliage,
                glass.dry_foliage,
                glass.water
            ),
            (0xff00ff, 0xff00ff, 0xff00ff, 0x00ffff)
        );
        assert_eq!(
            (glass.sky, glass.fog, glass.water_fog),
            (Some(0xff00ff), Some(0x00ffff), 0x00ffff)
        );
        let weights = gaussian_weights([10.3, 64.0, -7.9], |_, _, _| {
            biomes.id("wgtest:glass_flats")
        });
        assert_eq!(
            blend_rgb(&weights, |id| rgb(biomes.get(id).fog.unwrap())),
            [0.0, 1.0, 1.0]
        );
        // Use the ordinary weighted selector, pinning its draw against the
        // equivalent vanilla list and the explicitly declared 7:3 weights.
        let vanilla = biomes.get(0);
        let mut random = LegacyRandom::new(42);
        let mut reference = LegacyRandom::new(42);
        let mut vanilla_random = LegacyRandom::new(42);
        for _ in 0..32 {
            let selected = weighted_pick(&glass.spawns["creature"], &mut random).unwrap();
            let expected = if reference.next_int_bound(10).unwrap() < 7 {
                "minecraft:sheep"
            } else {
                "minecraft:pig"
            };
            assert_eq!(selected.entity, expected);
            assert_eq!(
                selected.entity,
                weighted_pick(&vanilla.spawns["creature"], &mut vanilla_random)
                    .unwrap()
                    .entity
            );
        }
        assert_eq!(glass.spawn_costs, vanilla.spawn_costs);

        let bundled: BTreeMap<String, String> =
            serde_json::from_slice(&fs::read(root.join("worldgen/biomes.json")).unwrap()).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&bundled["wgtest:glass_flats"]).unwrap(),
            biome
        );
        let catalog = BiomeSourceCatalog::new(
            &bundled.keys().cloned().collect(),
            &BTreeMap::new(),
            &["minecraft:overworld".into()].into(),
        )
        .unwrap();
        let presets =
            serde_json::from_slice(&fs::read(root.join("worldgen/presets.json")).unwrap()).unwrap();
        let additions = serde_json::from_slice(
            &fs::read(root.join("worldgen/biome_source_additions.json")).unwrap(),
        )
        .unwrap();
        let presets = catalog.with_preset_additions(presets, &additions).unwrap();
        let ResolvedBiomeSource::MultiNoise(source) = catalog
            .decode(r#"{"type":"minecraft:multi_noise","preset":"minecraft:overworld"}"#)
            .unwrap()
            .resolve(&presets)
            .unwrap()
        else {
            panic!("expected multi-noise")
        };
        // Strict winners, avoiding any dependence on climate tie ordering.
        for (point, expected) in [
            ([0.0, 0.0, 0.0, 0.0, 0.1, 0.0], "wgtest:glass_flats"),
            ([1.0; 6], "minecraft:plains"),
        ] {
            let climate = ClimateSampler::new(point.map(|v| Arc::new(ConstantSampler(v)) as _));
            assert_eq!(
                source
                    .resolver(&climate)
                    .sample(&mut SamplerContext::cached(), [4, -2, 8])
                    .unwrap(),
                expected
            );
        }
    }
}

#[test]
fn content_mod_without_biomes_preserves_biome_bytes() {
    let fixture = Fixture::new("no-declarations");
    let base = fixture.0.join("base");
    let before = Biomes::load(&base).unwrap();
    assert_eq!(
        resource_root(&base, &[], &fixture.0.join("unused-cache")).unwrap(),
        base
    );
    let root = fixture.overlay("ordinary", None);
    assert_ne!(root, base);
    for path in [
        "worldgen/biomes.json",
        "worldgen/presets.json",
        "data/minecraft/worldgen/biome/plains.json",
        "assets/minecraft/textures/colormap/grass.png",
        "assets/minecraft/textures/colormap/foliage.png",
        "assets/minecraft/textures/colormap/dry_foliage.png",
    ] {
        assert_eq!(
            fs::read(root.join(path)).unwrap(),
            fs::read(base.join(path)).unwrap(),
            "{path}"
        );
    }
    assert!(!root.join("worldgen/biome_source_additions.json").exists());
    assert!(!base.join("worldgen/biome_source_additions.json").exists());
    let after = Biomes::load(&root).unwrap();
    assert_eq!(after.index, before.index);
    assert_eq!(after.list.len(), before.list.len());
}
