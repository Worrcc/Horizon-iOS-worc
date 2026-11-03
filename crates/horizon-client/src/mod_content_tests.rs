use super::*;
use crate::{assets::Assets, blocks::BlockFacts, items::Items};

#[test]
fn renames_ids_keys_and_states() {
    let mut r = Rename::new("minecraft:iron_block", "ruby:ruby_block");
    r.states = 10..12;
    r.first = 100;
    assert_eq!(
        r.string("minecraft:iron_block").as_deref(),
        Some("ruby:ruby_block")
    );
    assert_eq!(
        r.string("Block{minecraft:iron_block}").as_deref(),
        Some("Block{ruby:ruby_block}")
    );
    assert_eq!(
        r.string("block.minecraft.iron_block").as_deref(),
        Some("block.ruby.ruby_block")
    );
    assert_eq!(r.string("minecraft:iron_blocks"), None);
    assert_eq!(r.string("block.minecraft.iron_bars"), None);
    assert_eq!(
        (r.id(9), r.id(10), r.id(11), r.id(12)),
        (None, Some(100), Some(101), None)
    );
}

#[test]
fn json_keeps_key_order_and_numbers() {
    let text = r#"{"z":1,"a":[0.6,5.0,-1,{"m":null,"b":true}],"k":"v"}"#;
    let doc: J = serde_json::from_str(text).unwrap();
    assert_eq!(serde_json::to_string(&doc).unwrap(), text);
}

#[test]
fn tags_merge_without_repeats() {
    let vanilla = r#"{"values": ["minecraft:stone"]}"#;
    let more = [
        J::Str("ruby:ruby_block".into()),
        J::Str("minecraft:stone".into()),
    ];
    let merged: serde_json::Value =
        serde_json::from_str(&merge_tag(Some(vanilla), &more, "t").unwrap()).unwrap();
    assert_eq!(
        merged["values"],
        serde_json::json!(["minecraft:stone", "ruby:ruby_block"])
    );
}

fn prepared() -> Option<PathBuf> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    [
        manifest.join("../../.local/horizon-client/26.3"),
        manifest.join("../../../../.local/horizon-client/26.3"),
    ]
    .into_iter()
    .find(|root| root.join("gameplay-facts.json").exists())
}

/// A content mod folder: a full block, a door and an item-less glass from
/// templates, an item, a vanilla tag addition and a language file.
fn ruby_mod(dir: &Path) -> ContentMod {
    let files: [(&str, &str); 8] = [
        (
            "horizon-mod.toml",
            "id = \"ruby\"\nname = \"Ruby\"\nversion = \"0.1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n",
        ),
        (
            "data/ruby/horizon/block/ruby_block.json",
            r#"{"template": "minecraft:iron_block", "properties": {"hardness": 4.0, "light": 3,
                "sounds": "minecraft:amethyst_block", "map_color": 28, "friction": 0.8}}"#,
        ),
        (
            "data/ruby/horizon/block/ruby_door.json",
            r#"{"template": "minecraft:oak_door"}"#,
        ),
        (
            "data/ruby/horizon/block/ruby_glass.json",
            r#"{"template": "minecraft:glass", "item": false}"#,
        ),
        (
            "data/ruby/horizon/item/ruby.json",
            r#"{"template": "minecraft:diamond", "max_stack_size": 16, "rarity": "rare"}"#,
        ),
        // Carried-over tags name blocks left out and other mods' ids too.
        (
            "data/minecraft/tags/block/mineable/pickaxe.json",
            r##"{"values": ["ruby:ruby_block", "ruby:left_out", "othermod:thing",
                {"id": "othermod:optional", "required": false}, "#c:storage_blocks/ruby"]}"##,
        ),
        (
            "data/c/tags/item/gems/ruby.json",
            r#"{"values": ["ruby:ruby", "othermod:ruby"]}"#,
        ),
        (
            "assets/ruby/lang/en_us.json",
            r#"{"block.ruby.ruby_block": "Block of Ruby"}"#,
        ),
    ];
    for (name, text) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let package = horizon_mods::Package::read(dir).unwrap();
    ContentMod {
        id: "ruby".into(),
        content: Content::read(dir, &package.manifest).unwrap(),
    }
}

/// The overlay over the real prepared resources: every loader reads it, the
/// new blocks behave like their templates with the overrides, and vanilla's
/// states are where they were.
#[test]
#[ignore = "full content root: CI and pr_ready run it"]
fn content_root_loads_with_every_table() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-content-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let m = ruby_mod(&scratch.join("ruby"));
    // The base as a relative path (cargo runs tests in the crate's folder):
    // the overlay's links still resolve.
    let relative = Path::new("../../.local/horizon-client/26.3");
    let root = resource_root(relative, std::slice::from_ref(&m), &scratch.join("cache")).unwrap();
    assert_ne!(root, base);

    let vanilla = Assets::load(&base).unwrap();
    let assets = Assets::load(&root).unwrap();
    let (old, new) = (vanilla.registry, assets.registry);
    let n = old.len() as u32;
    let iron = old.default_state("minecraft:iron_block").unwrap();
    let door = old.default_state("minecraft:oak_door").unwrap();
    let door_first = (0..n)
        .map(BlockStateId)
        .find(|&s| old.state(s).unwrap().name == "minecraft:oak_door")
        .unwrap();
    // Vanilla keeps its ids; blocks come after, sorted: ruby_block, ruby_door, ruby_glass.
    assert_eq!(new.len(), old.len() + 1 + 64 + 1);
    assert_eq!(new.default_state("minecraft:iron_block"), Some(iron));
    let ruby = new.default_state("ruby:ruby_block").unwrap();
    assert_eq!(ruby, BlockStateId(n));
    let ruby_door = new.default_state("ruby:ruby_door").unwrap();
    assert_eq!(ruby_door, BlockStateId(n + 1 + (door.0 - door_first.0)));
    assert_eq!(
        new.default_state("ruby:ruby_glass"),
        Some(BlockStateId(n + 65))
    );
    // Network ids: the new items and blocks follow vanilla's in the registries report, which
    // the game and its server both read (a missing one travels as air).
    let report = |root: &Path| {
        horizon_protocol::registry::Builtin::load(&root.join("registries.json")).unwrap()
    };
    let (vanilla_ids, ids) = (report(&base), report(&root));
    let item = |b: &horizon_protocol::registry::Builtin, name: &str| {
        b.get("minecraft:item").and_then(|r| r.id(name))
    };
    let vanilla_items = (0..)
        .take_while(|&i| vanilla_ids.get("minecraft:item").unwrap().name(i).is_some())
        .count() as i32;
    assert_eq!(
        item(&ids, "minecraft:stone"),
        item(&vanilla_ids, "minecraft:stone")
    );
    let ruby_item = item(&ids, "ruby:ruby_block").unwrap();
    assert!(ruby_item >= vanilla_items, "{ruby_item} < {vanilla_items}");
    assert_eq!(
        ids.get("minecraft:item").unwrap().name(ruby_item),
        Some("ruby:ruby_block")
    );
    assert!(item(&ids, "ruby:ruby_glass").is_none(), "no item declared");
    assert!(
        ids.get("minecraft:block")
            .unwrap()
            .id("ruby:ruby_glass")
            .is_some()
    );

    let old_facts = BlockFacts::load(&base, old).unwrap();
    let facts = BlockFacts::load(&root, new).unwrap();
    let shared_json: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(
        fs::File::open(root.join("block-behaviours.json")).unwrap(),
    ))
    .unwrap();
    let (shared_facts, shared_behaviours) =
        BlockFacts::load_with_behaviours(&root, new, &shared_json).unwrap();
    let independent_behaviours = horizon_world::level::Behaviours::load(new, &root).unwrap();
    let (deferred_facts, gameplay) =
        BlockFacts::load_with_projection(&root, new, &shared_json).unwrap();
    let deferred_behaviours =
        horizon_world::level::Behaviours::load_with_gameplay(new, &root, &shared_json, &gameplay)
            .unwrap();
    assert_eq!(shared_facts.states.len(), facts.states.len());
    for state in [ruby, ruby_door, BlockStateId(n + 65)] {
        assert_eq!(shared_facts.get(state).hardness, facts.get(state).hardness);
        assert_eq!(
            deferred_facts.get(state).hardness,
            facts.get(state).hardness
        );
        assert_eq!(deferred_facts.get(state).classes, facts.get(state).classes);
        assert_eq!(
            deferred_behaviours.facts(state).flags,
            independent_behaviours.facts(state).flags
        );
        assert_eq!(
            deferred_behaviours.data(state).sounds.place,
            independent_behaviours.data(state).sounds.place
        );
        assert_eq!(shared_facts.get(state).emission, facts.get(state).emission);
        assert_eq!(shared_facts.get(state).classes, facts.get(state).classes);
        assert_eq!(
            shared_behaviours.facts(state).flags,
            independent_behaviours.facts(state).flags,
        );
        assert_eq!(
            shared_behaviours.data(state).sounds.place,
            independent_behaviours.data(state).sounds.place,
        );
    }

    let (a, b) = (old_facts.get(iron), facts.get(ruby));
    assert_eq!((b.hardness, b.emission, b.friction), (4.0, 3, 0.8));
    assert_eq!(b.explosion_resistance, a.explosion_resistance);
    assert_eq!(b.outline, a.outline);
    assert_eq!(b.classes, a.classes);
    assert_eq!(&*b.item, "ruby:ruby_block");
    assert_eq!(b.map_color, 28);
    let amethyst = old_facts.get(old.default_state("minecraft:amethyst_block").unwrap());
    assert_eq!(b.sounds.break_sound, amethyst.sounds.break_sound);
    for i in 0..64 {
        let (v, m) = (
            old_facts.get(BlockStateId(door_first.0 + i)),
            facts.get(BlockStateId(n + 1 + i)),
        );
        assert_eq!(
            (v.outline.clone(), v.hardness, v.classes.clone()),
            (m.outline.clone(), m.hardness, m.classes.clone())
        );
        assert_eq!(&*m.name, "ruby:ruby_door");
    }
    let glass = facts.get(BlockStateId(n + 65));
    assert_eq!(&*glass.item, "minecraft:air");
    // Vanilla's own rows are untouched.
    assert_eq!(facts.get(iron).hardness, a.hardness);
    assert_eq!(&*facts.get(iron).name, "minecraft:iron_block");

    let items = Items::load(&root, new, &facts).unwrap();
    let ruby_block = &items.items["ruby:ruby_block"];
    assert_eq!(ruby_block.block, Some(ruby));
    let gem = &items.items["ruby:ruby"];
    assert_eq!(gem.max_stack, 16);
    assert!(!items.items.contains_key("ruby:ruby_glass"));
    let pickaxe = &items.block_tags["minecraft:mineable/pickaxe"];
    assert!(pickaxe.contains("ruby:ruby_block"));
    assert!(!pickaxe.contains("ruby:left_out") && !pickaxe.contains("othermod:thing"));
    assert!(pickaxe.contains("minecraft:stone"));
    // Convention tags load, with the ids nobody registers dropped.
    assert_eq!(items.item_tag_order["c:gems/ruby"], ["ruby:ruby"]);

    crate::collisions::load(&root, new).unwrap();
    crate::models::load_visibility(&root, new).unwrap();
    crate::light::LightTables::load(&root, new, &facts).unwrap();
    crate::motion::MotionResources::load(&root, &assets).unwrap();
    horizon_world::worldgen::generator::GeneratorData::load(&root.join("worldgen"), new).unwrap();
    assert!(root.join("assets/ruby/lang/en_us.json").is_file());
    // Blocks without a blockstate of their own take their template's.
    assert_eq!(
        fs::read(root.join("assets/ruby/blockstates/ruby_door.json")).unwrap(),
        fs::read(base.join("assets/minecraft/blockstates/oak_door.json")).unwrap()
    );

    // The base root was only read.
    assert!(
        fs::symlink_metadata(base.join("facts.json"))
            .unwrap()
            .is_file()
    );
    fs::remove_dir_all(&scratch).unwrap();
}

#[test]
fn refuses_unknown_templates_and_vanilla_ids() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-refuse-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let mut m = ruby_mod(&scratch.join("ruby"));
    m.content.blocks[0].template = "minecraft:not_a_block".into();
    let e = build(&base, std::slice::from_ref(&m), &scratch.join("out")).unwrap_err();
    assert!(e.to_string().contains("is not a block"), "{e}");
    fs::remove_dir_all(&scratch).unwrap();
}

#[test]
fn refuses_more_states_than_a_world_holds() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-states-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let mut m = ruby_mod(&scratch.join("ruby"));
    // Redstone wire has 1296 states: 810 copies pass 2^20 with any game.
    let mut wire = m.content.blocks[0].clone();
    wire.template = "minecraft:redstone_wire".into();
    for i in 0..810 {
        let mut b = wire.clone();
        b.id = format!("ruby:wire_{i}");
        m.content.blocks.push(b);
    }
    let e = build(&base, std::slice::from_ref(&m), &scratch.join("out")).unwrap_err();
    assert!(
        e.to_string()
            .contains("more than the 1048576 a world can hold"),
        "{e}"
    );
    assert!(e.to_string().contains("the mods: ruby "), "{e}");
    fs::remove_dir_all(&scratch).unwrap();
}

#[test]
fn builds_past_sixteen_bit_states() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-wide-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let mut m = ruby_mod(&scratch.join("ruby"));
    // 40 redstone wires: 51,840 states, past 65,536 with vanilla's 35,723.
    let mut wire = m.content.blocks[0].clone();
    wire.template = "minecraft:redstone_wire".into();
    for i in 0..40 {
        let mut b = wire.clone();
        b.id = format!("ruby:wire_{i}");
        m.content.blocks.push(b);
    }
    build(&base, std::slice::from_ref(&m), &scratch.join("out")).unwrap();
    fs::remove_dir_all(&scratch).unwrap();
}

#[test]
fn no_content_keeps_the_base_root() {
    let base = Path::new("/nonexistent/base");
    assert_eq!(
        resource_root(base, &[], Path::new("/nonexistent/cache")).unwrap(),
        base
    );
}

/// Original fixtures: a packaged biome reaches both resource consumers without
/// needing prepared vanilla resources, code mods, a window or a network.
#[test]
fn content_biome_registry_and_climate_source() {
    use horizon_core::density::{ConstantSampler, SamplerContext};
    use horizon_world::worldgen::{
        biome_source::{BiomeSourceCatalog, ResolvedBiomeSource},
        climate::{ClimateSampler, TargetPoint},
    };
    use serde_json::{Value, json};
    use std::{collections::HashMap, sync::Arc};

    let scratch =
        std::env::temp_dir().join(format!("horizon-content-biome-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let base = scratch.join("base");
    let write = |name: &str, value: Value| {
        let path = base.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, value.to_string()).unwrap();
    };
    write(
        "facts.json",
        json!({"states":[["minecraft:air"]],"defaults":{"minecraft:air":0}}),
    );
    write(
        "gameplay-facts.json",
        json!({"states":[{"name":"minecraft:air","item":"minecraft:air"}]}),
    );
    write("items.json", json!({}));
    write("horizon-resources.json", json!({}));
    write(
        "registries.json",
        json!({
            "minecraft:item":{"entries":{"minecraft:air":{"protocol_id":0}}},
            "minecraft:block":{"entries":{"minecraft:air":{"protocol_id":0}}}
        }),
    );
    let vanilla = json!({"temperature":0.5,"downfall":0.5,"has_precipitation":true,
        "effects":{"water_color":4159204},"carvers":[],"features":[]});
    write("data/minecraft/worldgen/biome/plains.json", vanilla.clone());
    write(
        "worldgen/biomes.json",
        json!({"minecraft:plains":vanilla.to_string()}),
    );
    write("worldgen/biome_tags.json", json!({}));
    for name in ["grass", "foliage", "dry_foliage"] {
        let path = base.join(format!("assets/minecraft/textures/colormap/{name}.png"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut encoder = png::Encoder::new(fs::File::create(path).unwrap(), 256, 256);
        encoder.set_color(png::ColorType::Rgb);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&vec![128; 256 * 256 * 3])
            .unwrap();
    }
    let before = crate::biome::Biomes::load(&base).unwrap();
    let biome = json!({"temperature":-0.7,"downfall":0.4,"has_precipitation":true,
        "effects":{"grass_color":1193046,"foliage_color":2311527,"water_color":3430008},
        "attributes":{
            "minecraft:visual/sky_color":"#456789","minecraft:visual/fog_color":"#56789a",
            "minecraft:visual/water_fog_color":"#6789ab",
            "minecraft:gameplay/natural_mob_spawns":{"spawns_by_category":{
                "creature":[{"type":"minecraft:sheep","weight":7,"count":2}]},
                "spawn_costs":{"minecraft:sheep":{"energy_budget":1.0,"charge":0.2}}}},
        "carvers":[],"features":[]});
    let addition = json!({"type":"minecraft:multi_noise","horizon:preset":"minecraft:overworld",
        "biomes":[{"biome":"aaa:groves/frost","parameters":{
            "temperature":-0.7,"humidity":0.4,"continentalness":0.3,
            "erosion":0.1,"depth":0,"weirdness":0.2,"offset":0}}]});
    let m = folder_mod(
        &scratch.join("mod"),
        &[
            ("horizon-mod.toml", &manifest("aaa", "")),
            (
                "data/aaa/worldgen/biome/groves/frost.json",
                &biome.to_string(),
            ),
            (
                "data/aaa/worldgen/biome_source_addition/groves.json",
                &addition.to_string(),
            ),
            (
                "data/aaa/tags/worldgen/biome/frost.json",
                r#"{"values":["aaa:groves/frost","aaa:missing"]}"#,
            ),
            (
                "data/c/tags/worldgen/biome/cold.json",
                r##"{"values":["#aaa:frost"]}"##,
            ),
            (
                "data/minecraft/tags/worldgen/biome/cold.json",
                r##"{"values":["#c:cold","minecraft:plains"]}"##,
            ),
        ],
    );
    let root = resource_root(&base, &[m], &scratch.join("cache")).unwrap();
    let bundled: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(root.join("worldgen/biomes.json")).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&bundled["aaa:groves/frost"]).unwrap(),
        biome
    );
    assert_eq!(
        serde_json::from_str::<Value>(&bundled["minecraft:plains"]).unwrap(),
        vanilla
    );
    let tags: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(root.join(BIOME_TAGS)).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&tags["aaa:frost"]).unwrap()["values"],
        json!(["aaa:groves/frost"])
    );
    let biomes = crate::biome::Biomes::load(&root).unwrap();
    assert_eq!(biomes.id("minecraft:plains"), before.id("minecraft:plains"));
    let frost = biomes.get(biomes.id("aaa:groves/frost"));
    assert_eq!(frost.name, "aaa:groves/frost");
    assert_eq!(
        (frost.temperature, frost.downfall, frost.precipitation),
        (-0.7, 0.4, true)
    );
    assert_eq!(
        (frost.grass, frost.foliage, frost.water),
        (0x123456, 0x234567, 0x345678)
    );
    assert_eq!(
        (frost.sky, frost.fog, frost.water_fog),
        (Some(0x456789), Some(0x56789a), 0x6789ab)
    );
    let spawn = &frost.spawns["creature"][0];
    assert_eq!(
        (
            &*spawn.entity,
            spawn.weight,
            spawn.min,
            spawn.max,
            spawn.constant
        ),
        ("minecraft:sheep", 7, 2, 2, true)
    );
    assert_eq!(frost.spawn_costs["minecraft:sheep"].charge, 0.2);
    // Configuration uses the same overlay for integrated, LAN and dedicated
    // servers. Decode actual packet bytes as the client does in net.rs.
    use crate::lan::{chunks::ChunkEncoder, data::SyncData};
    use horizon_protocol::{
        buf::Reader,
        packets::configuration::{Clientbound, UpdateTags},
        registry::{Dynamic, ceil_log2},
    };
    let sync = SyncData::load(&root).unwrap();
    let registry = "minecraft:worldgen/biome";
    let before_sync = SyncData::load(&base).unwrap();
    let frost_id = sync.id(registry, &frost.name).unwrap();
    assert_eq!(frost_id, 1);
    assert_eq!(sync.id(registry, "minecraft:plains"), Some(0));
    assert_eq!(before_sync.id(registry, "minecraft:plains"), Some(0));
    let names: Vec<_> = biomes.list.iter().map(|b| b.name.clone()).collect();
    let encoder = ChunkEncoder::new(
        [(Arc::<str>::from("minecraft:air"), false)],
        &sync.builtin,
        // Deliberately use a different internal ordering to test name mapping.
        &[frost.name.clone(), "minecraft:plains".into()],
        &names,
    );
    let mut chunk = crate::chunk::Chunk::empty(0, 0);
    Arc::make_mut(&mut chunk.sections[0]).biomes.set(1, 1);
    for known_pack in [false, true] {
        let mut received = Dynamic::default();
        for packet in sync.registry_packets(known_pack) {
            let bytes = Clientbound::RegistryData(packet).encode();
            let mut reader = Reader::new(&bytes);
            let id = reader.varint().unwrap();
            let Clientbound::RegistryData(packet) = Clientbound::decode(id, reader.rest()).unwrap()
            else {
                panic!("expected registry data")
            };
            received.add(packet);
        }
        assert_eq!(received.id(registry, &frost.name), Some(frost_id));
        assert_eq!(received.name(registry, frost_id), Some(frost.name.as_str()));
        assert_eq!(
            received.data(registry, frost_id),
            Some(&crate::lan::data::json_to_nbt(&biome))
        );
        assert_eq!(received.data(registry, 0).is_none(), known_pack);
        let bytes = Clientbound::UpdateTags(sync.tags.clone()).encode();
        let mut reader = Reader::new(&bytes);
        let id = reader.varint().unwrap();
        let Clientbound::UpdateTags(UpdateTags { registries }) =
            Clientbound::decode(id, reader.rest()).unwrap()
        else {
            panic!("expected tags")
        };
        let tags = &registries
            .iter()
            .find(|r| r.registry == registry)
            .unwrap()
            .tags;
        for name in ["aaa:frost", "c:cold", "minecraft:cold"] {
            let tag = tags.iter().find(|t| t.name == name).unwrap();
            assert_eq!(tag.ids[0], frost_id);
            assert_eq!(
                received.name(registry, tag.ids[0]),
                Some(frost.name.as_str())
            );
            if name == "minecraft:cold" {
                assert_eq!(tag.ids, [frost_id, 0]);
            }
        }
        let packet = encoder.encode(&chunk, &HashMap::new());
        let mut reader = Reader::new(&packet.chunk.buffer);
        let section = horizon_protocol::chunk::Section::read(
            &mut reader,
            16,
            ceil_log2(received.len(registry)),
        )
        .unwrap();
        for cell in 0..64 {
            let id = section.biomes.get(cell);
            let name = received.name(registry, id).unwrap();
            assert_eq!(
                name,
                if cell == 1 {
                    "minecraft:plains"
                } else {
                    &frost.name
                }
            );
            assert_eq!(biomes.get(biomes.id(name)).name, name);
        }
    }
    let catalog = BiomeSourceCatalog::new(
        &bundled.keys().cloned().collect(),
        &BTreeMap::new(),
        &["minecraft:overworld".into()].into(),
    )
    .unwrap();
    let presets = [(
        "minecraft:overworld".into(),
        json!([{
            "coordinates":vec![[10000,10000];6],"offset":0,"biome":"minecraft:plains"
        }])
        .to_string(),
    )]
    .into();
    let additions = serde_json::from_slice(
        &fs::read(root.join("worldgen/biome_source_additions.json")).unwrap(),
    )
    .unwrap();
    let presets = catalog.with_preset_additions(presets, &additions).unwrap();
    let source = catalog
        .decode(r#"{"type":"minecraft:multi_noise","preset":"minecraft:overworld"}"#)
        .unwrap()
        .resolve(&presets)
        .unwrap();
    let ResolvedBiomeSource::MultiNoise(source) = source else {
        panic!("expected multi-noise")
    };
    let climate = ClimateSampler::new(
        [-0.7, 0.4, 0.3, 0.1, 0., 0.2].map(|v| Arc::new(ConstantSampler(v)) as _),
    );
    assert_eq!(
        source
            .resolver(&climate)
            .sample(&mut SamplerContext::cached(), [4, -2, 8])
            .unwrap(),
        frost.name
    );
    assert_eq!(
        source
            .parameters()
            .searcher()
            .find(TargetPoint::from_values([1.; 6]))
            .unwrap(),
        "minecraft:plains"
    );
    // A content mod without biome declarations keeps the vanilla tables and IDs.
    let plain = folder_mod(
        &scratch.join("plain"),
        &[
            ("horizon-mod.toml", &manifest("plain", "")),
            ("assets/plain/lang/en_us.json", "{}"),
        ],
    );
    let plain_root = resource_root(&base, &[plain], &scratch.join("plain-cache")).unwrap();
    assert!(
        !plain_root
            .join("worldgen/biome_source_additions.json")
            .exists()
    );
    assert_eq!(
        fs::read(plain_root.join("worldgen/biomes.json")).unwrap(),
        fs::read(base.join("worldgen/biomes.json")).unwrap()
    );
    assert_eq!(
        crate::biome::Biomes::load(&plain_root).unwrap().index,
        before.index
    );
    assert!(!base.join("worldgen/biome_source_additions.json").exists());
    fs::remove_dir_all(scratch).unwrap();
}

use horizon_world::registry::BlockStateId;

#[test]
#[ignore = "full content root: CI and pr_ready run it"]
fn shared_behaviour_document_matches_independent_loaders() {
    let Some(root) = prepared() else { return };
    let assets = Assets::load(&root).expect("assets");
    let doc: serde_json::Value = serde_json::from_reader(std::io::BufReader::new(
        std::fs::File::open(root.join("block-behaviours.json")).unwrap(),
    ))
    .unwrap();
    let original = BlockFacts::load(&root, assets.registry).unwrap();
    let (shared, b) = BlockFacts::load_with_behaviours(&root, assets.registry, &doc).unwrap();
    let (deferred_facts, projection) =
        BlockFacts::load_with_projection(&root, assets.registry, &doc).unwrap();
    let deferred = horizon_world::level::Behaviours::load_with_gameplay(
        assets.registry,
        &root,
        &doc,
        &projection,
    )
    .unwrap();
    assert_eq!(deferred_facts.states.len(), shared.states.len());
    assert_eq!(original.states.len(), shared.states.len());
    for (a, b) in original.states.iter().zip(&shared.states) {
        assert_eq!(a.classes, b.classes);
        assert_eq!(a.legacy_solid, b.legacy_solid);
        assert_eq!(a.sturdy_full, b.sturdy_full);
        assert_eq!(a.emission, b.emission);
        assert_eq!(a.dampening, b.dampening);
        assert_eq!(a.hardness, b.hardness);
        assert_eq!(a.sounds.volume, b.sounds.volume);
        assert_eq!(a.sounds.pitch, b.sounds.pitch);
    }
    let a = horizon_world::level::Behaviours::load(assets.registry, &root).unwrap();
    for id in 0..assets.registry.len() {
        let state = horizon_world::registry::BlockStateId(id as u32);
        let (a_state, b_state) = (a.facts(state), b.facts(state));
        let deferred_state = deferred.facts(state);
        assert_eq!(b_state.flags, deferred_state.flags);
        assert_eq!(b_state.emission, deferred_state.emission);
        assert_eq!(b_state.dampening, deferred_state.dampening);
        assert_eq!(b_state.hardness, deferred_state.hardness);
        assert_eq!(b_state.push, deferred_state.push);
        assert_eq!(a_state.flags, b_state.flags);
        assert_eq!(a_state.sturdy, b_state.sturdy);
        assert_eq!(a_state.weak, b_state.weak);
        assert_eq!(a_state.strong, b_state.strong);
        assert_eq!(a_state.fluid, b_state.fluid);
        assert_eq!(a_state.emission, b_state.emission);
        assert_eq!(a_state.dampening, b_state.dampening);
        assert_eq!(a_state.hardness, b_state.hardness);
        assert_eq!(a_state.push, b_state.push);
        assert_eq!(a_state.instrument, b_state.instrument);
        assert_eq!(a_state.shape, b_state.shape);
        let (a_sound, b_sound) = (&a.data(state).sounds, &b.data(state).sounds);
        assert_eq!(a_sound.volume, b_sound.volume);
        assert_eq!(a_sound.pitch, b_sound.pitch);
        assert_eq!(a_sound.place, b_sound.place);
        assert_eq!(a_sound.break_, b_sound.break_);
        assert_eq!(a_sound.fall, b_sound.fall);
    }
}

#[cfg(unix)]
#[test]
#[ignore = "full content root: CI and pr_ready run it"]
fn independent_reloads_read_changed_behaviour_json_and_report_invalid_files() {
    let Some(base) = prepared() else { return };
    let scratch = std::env::temp_dir().join(format!("horizon-json-reloads-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    fs::create_dir_all(&scratch).unwrap();
    // Keep the remaining resource tree intact without copying a huge prepared client.
    for entry in fs::read_dir(&base).unwrap() {
        let entry = entry.unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(entry.path(), scratch.join(entry.file_name())).unwrap();
    }
    let assets = Assets::load(&base).unwrap();
    let registry = assets.registry;
    let original = fs::read(base.join("block-behaviours.json")).unwrap();
    fs::remove_file(scratch.join("block-behaviours.json")).unwrap();
    fs::write(scratch.join("block-behaviours.json"), &original).unwrap();
    // Independent facts and behaviours can load first, followed by paired data.
    let independent_first = BlockFacts::load(&scratch, registry).unwrap();
    let independent_world_first =
        horizon_world::level::Behaviours::load(registry, &scratch).unwrap();
    let (facts, paired_world) = BlockFacts::load_pair(&scratch, registry).unwrap();
    assert_eq!(facts.states.len(), registry.len());
    for state in [0, 1, registry.len() - 1] {
        let state = horizon_world::registry::BlockStateId(state as u32);
        assert_eq!(
            facts.get(state).classes,
            independent_first.get(state).classes
        );
        assert_eq!(
            facts.get(state).sounds.place,
            independent_first.get(state).sounds.place
        );
        assert_eq!(
            paired_world.facts(state).flags,
            independent_world_first.facts(state).flags
        );
    }

    // Override one exported state bit at the same path and verify that a later
    // independent and paired read sees it while the original resource root does not.
    let mut changed: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let original_bits = changed["states"][0][1].as_u64().unwrap();
    changed["states"][0][1] = (original_bits ^ 1).into();
    fs::write(
        scratch.join("block-behaviours.json"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    let changed_pair = BlockFacts::load_pair(&scratch, registry).unwrap();
    let changed_independent = BlockFacts::load(&scratch, registry).unwrap();
    let base_facts = BlockFacts::load(&base, registry).unwrap();
    let state = horizon_world::registry::BlockStateId(0);
    assert_ne!(
        changed_pair.0.get(state).legacy_solid,
        base_facts.get(state).legacy_solid
    );
    assert_eq!(
        changed_pair.0.get(state).legacy_solid,
        changed_independent.get(state).legacy_solid
    );
    // The world loader sees the same overridden legacy solidity flag.
    let changed_world = horizon_world::level::Behaviours::load(registry, &scratch).unwrap();
    assert_eq!(
        changed_world.facts(state).flags & 1 != 0,
        changed_pair.0.get(state).legacy_solid
    );

    fs::write(scratch.join("block-behaviours.json"), b"{").unwrap();
    let paired_error = BlockFacts::load_pair(&scratch, registry)
        .err()
        .unwrap()
        .to_string();
    let independent_error = BlockFacts::load(&scratch, registry)
        .err()
        .unwrap()
        .to_string();
    assert_eq!(paired_error, independent_error);
    assert!(horizon_world::level::Behaviours::load(registry, &scratch).is_err());

    fs::write(scratch.join("block-behaviours.json"), b"{}").unwrap();
    assert!(BlockFacts::load_pair(&scratch, registry).is_err());
    assert!(BlockFacts::load(&scratch, registry).is_err());
    assert!(horizon_world::level::Behaviours::load(registry, &scratch).is_err());

    fs::write(scratch.join("block-behaviours.json"), original).unwrap();
    assert_eq!(
        BlockFacts::load_pair(&scratch, registry)
            .unwrap()
            .0
            .states
            .len(),
        registry.len()
    );
    fs::remove_file(scratch.join("gameplay-facts.json")).unwrap();
    fs::write(scratch.join("gameplay-facts.json"), b"{\"states\":[]}").unwrap();
    assert!(BlockFacts::load_pair(&scratch, registry).is_err());
    assert!(BlockFacts::load(&scratch, registry).is_err());
    assert!(horizon_world::level::Behaviours::load(registry, &scratch).is_err());
    fs::write(scratch.join("gameplay-facts.json"), b"{").unwrap();
    assert!(BlockFacts::load_pair(&scratch, registry).is_err());
    assert!(BlockFacts::load(&scratch, registry).is_err());
    assert!(horizon_world::level::Behaviours::load(registry, &scratch).is_err());
    fs::write(
        scratch.join("gameplay-facts.json"),
        fs::read(base.join("gameplay-facts.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        BlockFacts::load_pair(&scratch, registry)
            .unwrap()
            .0
            .states
            .len(),
        registry.len()
    );
    fs::remove_dir_all(&scratch).unwrap();
}

/// A built root is kept for later starts, and built again when a mod's
/// files change.
#[test]
#[ignore = "full content root: CI and pr_ready run it"]
fn content_root_is_reused_until_a_mod_changes() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-reuse-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let m = ruby_mod(&scratch.join("ruby"));
    let cache = scratch.join("cache");
    let root = resource_root(&base, std::slice::from_ref(&m), &cache).unwrap();
    // A rebuild starts from an empty folder: this file tells them apart.
    let sentinel = root.join("sentinel");
    fs::write(&sentinel, b"").unwrap();
    assert_eq!(
        resource_root(&base, std::slice::from_ref(&m), &cache).unwrap(),
        root
    );
    assert!(sentinel.exists(), "an unchanged mod rebuilt the root");
    let lang = scratch.join("ruby/assets/ruby/lang/en_us.json");
    fs::write(&lang, r#"{"block.ruby.ruby_block": "Ruby Block"}"#).unwrap();
    let file = fs::File::options().write(true).open(&lang).unwrap();
    file.set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(5))
        .unwrap();
    resource_root(&base, std::slice::from_ref(&m), &cache).unwrap();
    assert!(!sentinel.exists(), "a changed mod kept the old root");
    assert!(
        fs::read_to_string(root.join("assets/ruby/lang/en_us.json"))
            .unwrap()
            .contains("Ruby Block")
    );
    let _ = fs::remove_dir_all(&scratch);
}

/// A structure mod's worldgen reaches the worldgen loader: its structure,
/// set, pool and templates join the bundles, and its biome tag keeps only
/// what the loader knows.
#[test]
#[ignore = "full content root: CI and pr_ready run it"]
fn content_root_carries_a_structure_mod() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-worldgen-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let dir = scratch.join("huts");
    let files = [
        (
            "horizon-mod.toml",
            "id = \"huts\"\nname = \"Huts\"\nversion = \"1.0\"\napi = \"0.2\"\nside = \"both\"\ncapabilities = [\"registries\"]\n",
        ),
        (
            "data/huts/worldgen/structure/hut.json",
            r##"{"type": "minecraft:jigsaw", "biomes": "#huts:has_hut", "step": "surface_structures",
                "spawn_overrides": {}, "terrain_adaptation": "beard_thin", "start_pool": "huts:hut/start",
                "size": 1, "start_height": {"absolute": 0}, "project_start_to_heightmap": "WORLD_SURFACE_WG",
                "max_distance_from_center": 80}"##,
        ),
        (
            "data/huts/worldgen/template_pool/hut/start.json",
            r#"{"fallback": "minecraft:empty", "elements": [{"weight": 1, "element": {
                "element_type": "minecraft:single_pool_element", "location": "huts:hut",
                "projection": "rigid", "processors": "minecraft:empty"}}]}"#,
        ),
        (
            "data/huts/worldgen/structure_set/huts.json",
            r#"{"structures": [{"structure": "huts:hut", "weight": 1}],
                "placement": {"type": "minecraft:random_spread", "salt": 7, "spacing": 4, "separation": 2}}"#,
        ),
        (
            "data/huts/tags/worldgen/biome/has_hut.json",
            r##"{"values": ["#minecraft:is_overworld", "#c:is_plains", "othermod:glade", "minecraft:plains"]}"##,
        ),
        // Additions to vanilla's set and pool; a target nobody defines is skipped.
        (
            "data/huts/worldgen/structure_set/added/villages.json",
            r#"{"horizon:add_to": "minecraft:villages",
                "structures": [{"structure": "huts:hut", "weight": 3}]}"#,
        ),
        (
            "data/huts/worldgen/template_pool/added/houses.json",
            r#"{"horizon:add_to": ["othermod:houses", "minecraft:village/plains/houses"],
                "elements": [{"weight": 2, "element": {"element_type": "minecraft:single_pool_element",
                "location": "huts:hut", "projection": "rigid", "processors": "minecraft:empty"}}]}"#,
        ),
    ];
    for (name, text) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    // The template: vanilla's smallest plains house, as the mod's own file.
    let house =
        base.join("worldgen/structure/minecraft/village/plains/houses/plains_small_house_1.nbt");
    fs::create_dir_all(dir.join("data/huts/structure")).unwrap();
    fs::copy(&house, dir.join("data/huts/structure/hut.nbt")).unwrap();
    let package = horizon_mods::Package::read(&dir).unwrap();
    let m = ContentMod {
        id: "huts".into(),
        content: Content::read(&dir, &package.manifest).unwrap(),
    };
    let root = resource_root(&base, &[m], &scratch.join("cache")).unwrap();
    assert!(root.join("worldgen/structure/huts/hut.nbt").exists());
    let tags: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(root.join("worldgen/biome_tags.json")).unwrap()).unwrap();
    let hut: serde_json::Value = serde_json::from_str(&tags["huts:has_hut"]).unwrap();
    assert_eq!(
        hut["values"],
        serde_json::json!(["#minecraft:is_overworld", "minecraft:plains"])
    );
    // The additions join vanilla's entries at the end and are no set or pool
    // of their own.
    let bundle = |name: &str, base: &Path| -> BTreeMap<String, String> {
        serde_json::from_slice(&fs::read(base.join("worldgen").join(name)).unwrap()).unwrap()
    };
    for (name, id, list, added) in [
        (
            "structure_sets.json",
            "minecraft:villages",
            "structures",
            serde_json::json!({"structure": "huts:hut", "weight": 3}),
        ),
        (
            "template_pools.json",
            "minecraft:village/plains/houses",
            "elements",
            serde_json::json!({"weight": 2, "element": {"element_type": "minecraft:single_pool_element",
                "location": "huts:hut", "projection": "rigid", "processors": "minecraft:empty"}}),
        ),
    ] {
        let (ours, vanilla) = (bundle(name, &root), bundle(name, &base));
        let entries = |text: &str| -> Vec<serde_json::Value> {
            let v: serde_json::Value = serde_json::from_str(text).unwrap();
            v[list].as_array().unwrap().clone()
        };
        let mut expected = entries(&vanilla[id]);
        expected.push(added);
        assert_eq!(entries(&ours[id]), expected, "{id}");
        assert!(!ours.keys().any(|k| k.starts_with("huts:added/")), "{name}");
        assert!(!ours.contains_key("othermod:houses"));
    }

    let assets = Assets::load(&root).unwrap();
    let biomes = crate::biome::Biomes::load(&root).unwrap();
    let terrain = crate::vanillagen::VanillaTerrain::new(
        &root.join("worldgen"),
        assets.registry,
        &biomes,
        1234,
    )
    .unwrap();
    let (name, at) = terrain
        .locate(&["huts:hut".to_owned()], [0, 64, 0], 16)
        .expect("the mod's structure generates");
    assert_eq!(name, "huts:hut");
    assert!(at[0].abs() < 1024 && at[2].abs() < 1024, "{at:?}");
    let _ = fs::remove_dir_all(&scratch);
}

/// A content mod folder from `(path, text)` pairs, read as the game reads it.
fn folder_mod(dir: &Path, files: &[(&str, &str)]) -> ContentMod {
    for (name, text) in files {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    let package = horizon_mods::Package::read(dir).unwrap();
    ContentMod {
        id: package.manifest.id.clone(),
        content: Content::read(dir, &package.manifest).unwrap(),
    }
}

fn manifest(id: &str, namespaces: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0\"\napi = \"0.2\"\ncapabilities = [\"registries\"]\nnamespaces = [{namespaces}]\n"
    )
}

/// A mod whose files sit under namespaces besides its id (Towns and Towers'
/// `kaisyn`): they join the root, and loaders read those namespaces.
#[test]
fn listed_namespaces_join_the_root() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-ns-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let toml = manifest("towers", "\"kaisyn\"");
    let m = folder_mod(
        &scratch.join("towers"),
        &[
            ("horizon-mod.toml", &toml),
            (
                "data/kaisyn/worldgen/processor_list/mossy.json",
                r#"{"processors": []}"#,
            ),
            (
                "data/kaisyn/tags/worldgen/biome/has_tower.json",
                r#"{"values": ["minecraft:plains"]}"#,
            ),
            (
                "assets/kaisyn/lang/en_us.json",
                r#"{"block.kaisyn.x": "X"}"#,
            ),
        ],
    );
    let root = scratch.join("root");
    build(&base, &[m], &root).unwrap();
    assert_eq!(namespaces(&root), ["kaisyn", "towers"]);
    assert!(tag_namespaces(&root).contains(&"kaisyn".to_owned()));
    let lists: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(root.join("worldgen/processor_lists.json")).unwrap())
            .unwrap();
    assert!(lists.contains_key("kaisyn:mossy"));
    let tags: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(root.join("worldgen/biome_tags.json")).unwrap()).unwrap();
    assert!(tags.contains_key("kaisyn:has_tower"));
    let _ = fs::remove_dir_all(&scratch);
}

/// Two mods owning one namespace (one's listed namespace is the other's id,
/// or both list it) are refused together, naming both.
#[test]
fn mods_sharing_a_namespace_are_refused() {
    let scratch = std::env::temp_dir().join(format!("horizon-mod-clash-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let towers = folder_mod(
        &scratch.join("towers"),
        &[("horizon-mod.toml", &manifest("towers", "\"kaisyn\""))],
    );
    let kaisyn = folder_mod(
        &scratch.join("kaisyn"),
        &[("horizon-mod.toml", &manifest("kaisyn", ""))],
    );
    let other = folder_mod(
        &scratch.join("other"),
        &[("horizon-mod.toml", &manifest("other", "\"kaisyn\""))],
    );
    // The check runs before the base is read.
    let base = scratch.join("no-base");
    let out = scratch.join("root");
    let e = build(&base, &[towers, kaisyn], &out).unwrap_err();
    assert_eq!(
        e.to_string(),
        "mods towers and kaisyn both use the namespace kaisyn; turn one of them off"
    );
    let towers = folder_mod(
        &scratch.join("towers"),
        &[("horizon-mod.toml", &manifest("towers", "\"kaisyn\""))],
    );
    let e = build(&base, &[towers, other], &out).unwrap_err();
    assert!(e.to_string().starts_with("mods towers and other"), "{e}");
    let _ = fs::remove_dir_all(&scratch);
}

/// Mods merge into a tag in load order, as data packs do: each adds its
/// values, and one that replaces drops what came before it, with a warning
/// naming what it dropped.
#[test]
fn tags_merge_in_load_order_and_replace_warns() {
    let v = |ids: &[&str]| -> Vec<J> { ids.iter().map(|i| J::Str((*i).into())).collect() };
    let file = "data/minecraft/tags/worldgen/biome/has_structure/village_plains.json";
    let mut merge = TagMerge::default();
    assert_eq!(merge.add("a", file, false, true, &v(&["a:x"])), None);
    let warning = merge.add("b", file, true, true, &v(&["minecraft:plains"]));
    assert_eq!(
        warning.as_deref(),
        Some(
            "mod b replaces data/minecraft/tags/worldgen/biome/has_structure/village_plains.json: \
             vanilla's entries and those of a are left out, as a data pack's replace does; \
             the mods after it in load order still add to it"
        )
    );
    assert_eq!(merge.add("c", file, false, true, &v(&["c:y"])), None);
    assert!(merge.replaced);
    assert_eq!(merge.values, v(&["minecraft:plains", "c:y"]));
    // A second replace drops only the mods' entries.
    let warning = merge.add("d", file, true, true, &v(&["d:z"])).unwrap();
    assert!(
        warning.contains(": those of b, c are left out"),
        "{warning}"
    );
    assert_eq!(merge.values, v(&["d:z"]));
    // A mod replacing a tag of its own that nobody added to drops nothing.
    let own = "data/a/tags/item/gems.json";
    assert_eq!(
        TagMerge::default().add("a", own, true, false, &v(&["a:x"])),
        None
    );
}

/// A replacing mod's tag leaves vanilla's entries out of the tag file and of
/// the worldgen loader's biome tags, and the root keeps the warning for the
/// starts that reuse it.
#[test]
fn a_replacing_mod_tag_reaches_the_root() {
    let Some(base) = prepared() else {
        return;
    };
    let scratch = std::env::temp_dir().join(format!("horizon-mod-replace-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let file = "data/minecraft/tags/worldgen/biome/has_structure/village_plains.json";
    let adds = folder_mod(
        &scratch.join("adds"),
        &[
            ("horizon-mod.toml", &manifest("adds", "")),
            (file, r#"{"values": ["minecraft:forest"]}"#),
        ],
    );
    let towns = folder_mod(
        &scratch.join("towns"),
        &[
            ("horizon-mod.toml", &manifest("towns", "")),
            (file, r#"{"replace": true, "values": ["minecraft:plains"]}"#),
        ],
    );
    let late = folder_mod(
        &scratch.join("late"),
        &[
            ("horizon-mod.toml", &manifest("late", "")),
            (file, r#"{"values": ["minecraft:savanna"]}"#),
        ],
    );
    let vanilla: serde_json::Value =
        serde_json::from_slice(&fs::read(base.join(file)).unwrap()).unwrap();
    assert!(vanilla["values"].as_array().unwrap().len() > 1, "{vanilla}");
    let root = resource_root(&base, &[adds, towns, late], &scratch.join("cache")).unwrap();
    let expected = serde_json::json!(["minecraft:plains", "minecraft:savanna"]);
    let tag: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(file)).unwrap()).unwrap();
    assert_eq!(tag["values"], expected);
    let tags: BTreeMap<String, String> =
        serde_json::from_slice(&fs::read(root.join("worldgen/biome_tags.json")).unwrap()).unwrap();
    let bundled: serde_json::Value =
        serde_json::from_str(&tags["minecraft:has_structure/village_plains"]).unwrap();
    assert_eq!(bundled["values"], expected);
    let warnings = fs::read_to_string(root.join(WARNINGS)).unwrap();
    assert!(
        warnings.starts_with("mod towns replaces data/minecraft/tags/worldgen/biome/has_structure/village_plains.json: vanilla's entries and those of adds are left out"),
        "{warnings}"
    );
    let _ = fs::remove_dir_all(&scratch);
}

#[test]
fn mod_entities_copy_their_template_row() {
    let golem = Entity {
        id: "ruby:ruby_golem".into(),
        template: "minecraft:zombie".into(),
        width: Some(1.5),
        height: None,
        eye_height: None,
        attributes: [("max_health".to_owned(), 60.0)].into_iter().collect(),
        fire_immune: Some(true),
        persistent: None,
        category: None,
        texture: Some("ruby:entity/golem".into()),
        model: Some("ruby:golem".into()),
        animations: [("walk".to_owned(), "animation.golem.walk".to_owned())]
            .into_iter()
            .collect(),
    };
    let plan = Plan {
        blocks: Vec::new(),
        items: Vec::new(),
        entities: vec![&golem],
        states: 0,
        defaults: BTreeMap::new(),
        sounds: BTreeMap::new(),
    };
    let mut doc: J = serde_json::from_str(
        r#"{"minecraft:zombie": {"width": 0.6, "height": 1.95, "category": "monster",
            "fire_immune": false, "attributes": {"minecraft:max_health": 20.0}}}"#,
    )
    .unwrap();
    plan.json("entity-types.json", &mut doc).unwrap();
    let row = doc.get("ruby:ruby_golem").unwrap();
    assert_eq!(
        row.get("width"),
        Some(&J::Num(serde_json::Number::from_f64(1.5).unwrap()))
    );
    assert_eq!(
        row.get("height"),
        doc.get("minecraft:zombie").unwrap().get("height")
    );
    assert_eq!(row.get("fire_immune"), Some(&J::Bool(true)));
    assert_eq!(
        row.get("attributes")
            .and_then(|a| a.get("minecraft:max_health")),
        Some(&J::Num(serde_json::Number::from_f64(60.0).unwrap()))
    );
    let mut attributes: J = serde_json::from_str(
        r#"{"data_version":1,"attributes":[],"entities":{"minecraft:zombie":{"minecraft:armor":"4030000000000000","minecraft:max_health":"4034000000000000"}}}"#,
    )
    .unwrap();
    plan.json("attributes.json", &mut attributes).unwrap();
    assert_eq!(
        attributes
            .get("entities")
            .and_then(|e| e.get("ruby:ruby_golem"))
            .and_then(|e| e.get("minecraft:armor")),
        Some(&J::Str("4030000000000000".into()))
    );
    assert_eq!(
        attributes
            .get("entities")
            .and_then(|e| e.get("ruby:ruby_golem"))
            .and_then(|e| e.get("minecraft:max_health")),
        Some(&J::Str(format!("{:016x}", 60.0f64.to_bits())))
    );
    let horizon = row.get("horizon").unwrap();
    assert_eq!(
        horizon.get("template"),
        Some(&J::Str("minecraft:zombie".into()))
    );
    assert_eq!(
        horizon.get("texture"),
        Some(&J::Str("ruby:entity/golem".into()))
    );
    assert_eq!(horizon.get("model"), Some(&J::Str("ruby:golem".into())));
    assert_eq!(
        horizon.get("animations").and_then(|a| a.get("walk")),
        Some(&J::Str("animation.golem.walk".into()))
    );
    // The template is untouched, and a second run refuses the id it now holds.
    assert_eq!(
        doc.get("minecraft:zombie").unwrap().get("fire_immune"),
        Some(&J::Bool(false))
    );
    assert!(plan.json("entity-types.json", &mut doc).is_err());
}

/// A small exported-table fixture exercises the complete content root and the
/// real component/tag loaders without requiring the player's prepared game.
#[test]
fn declared_equipment_and_charged_templates_load_into_the_catalog() {
    use serde_json::{Value, json};
    let scratch =
        std::env::temp_dir().join(format!("horizon-item-components-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let base = scratch.join("base");
    let write = |name: &str, value: Value| {
        let path = base.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    };
    write(
        "facts.json",
        json!({"states":[["minecraft:air"]],"defaults":{"minecraft:air":0}}),
    );
    write(
        "gameplay-facts.json",
        json!({"states":[{"name":"minecraft:air","item":"minecraft:air"}]}),
    );
    fs::create_dir_all(base.join("data/minecraft/loot_table")).unwrap();
    let mut vanilla = json!({"minecraft:stick":{"minecraft:max_stack_size":64}});
    let mut declarations: Vec<(String, String)> =
        vec![("horizon-mod.toml".into(), manifest("gear", ""))];
    for (slot, suffix) in [
        ("head", "helmet"),
        ("chest", "chestplate"),
        ("legs", "leggings"),
        ("feet", "boots"),
    ] {
        vanilla[format!("minecraft:iron_{suffix}")] = json!({
            "minecraft:item_model":format!("minecraft:iron_{suffix}"),
            "minecraft:max_damage":165,"minecraft:max_stack_size":1,
            "minecraft:equippable":{"slot":slot,"asset_id":"minecraft:iron","equip_sound":"minecraft:item.armor.equip_iron"}
        });
        declarations.push((format!("data/gear/horizon/item/{suffix}.json"), json!({
            "template":"minecraft:stick",
            "armor":{"slot":slot,"defense":7,"toughness":6,"knockback_resistance":0.1,"durability":1760,"texture":"gear:metal"},
            "properties":{"enchantability":30,"repair_item":"gear:ingot","fire_resistant":true}
        }).to_string()));
    }
    for kind in ["sword", "pickaxe", "axe", "shovel", "hoe"] {
        let id = format!("minecraft:iron_{kind}");
        vanilla[&id] = json!({
            "minecraft:item_model":id,"minecraft:max_damage":250,"minecraft:max_stack_size":1,
            "minecraft:tool":{"rules":[{"blocks":"minecraft:cobweb","speed":15,"correct_for_drops":true}],"damage_per_block":2,"can_destroy_blocks_in_creative":false},
            "minecraft:weapon":{"item_damage_per_attack":1},
            "minecraft:block_transformer":format!("minecraft:{kind}")
        });
        declarations.push((format!("data/gear/horizon/item/{kind}.json"), json!({
            "template":"minecraft:stick",
            "tool":{"kind":kind,"attack_damage":27,"attack_speed":-2.4,"mining_speed":10,"durability":8000,
                    "incorrect_blocks_for_drops":"minecraft:incorrect_for_wooden_tool","enchantability":21,"repair_item":"gear:ingot"}
        }).to_string()));
        write(
            &format!("data/minecraft/tags/item/enchantable/{kind}.json"),
            json!({"values":[id]}),
        );
        write(
            &format!("data/minecraft/tags/block/mineable/{kind}.json"),
            json!({"values":["minecraft:stone","minecraft:obsidian"]}),
        );
    }
    write(
        "data/minecraft/tags/block/incorrect_for_wooden_tool.json",
        json!({"values":["minecraft:obsidian"]}),
    );
    for kind in ["bow", "crossbow", "trident"] {
        let id = format!("minecraft:{kind}");
        vanilla[&id] = json!({"minecraft:item_model":id,"minecraft:max_damage":384,"minecraft:max_stack_size":1,
                             "minecraft:use_effects":{"speed_multiplier":0.2}});
        declarations.push((
            format!("data/gear/horizon/item/{kind}.json"),
            json!({
                "template":id,"properties":{"max_damage":584,"attack_damage":12,"attack_speed":-2.9}
            })
            .to_string(),
        ));
        write(
            &format!("data/minecraft/tags/item/enchantable/{kind}.json"),
            json!({"values":[id]}),
        );
    }
    vanilla["minecraft:netherite_helmet"] = json!({
        "minecraft:equippable":{"slot":"head","asset_id":"minecraft:netherite"},
        "minecraft:max_damage":407,"minecraft:max_stack_size":1,
        "minecraft:enchantable":{"value":15},
        "minecraft:damage_resistant":{"types":"#minecraft:is_fire"}
    });
    declarations.push(("data/gear/horizon/item/plain_helmet.json".into(), json!({
        "template":"minecraft:netherite_helmet",
        "properties":{"max_damage":123,"enchantability":0,"fire_resistant":false,"rarity":"epic"}
    }).to_string()));
    // Root-level legacy overrides still load and retain their meaning.
    declarations.push((
        "data/gear/horizon/item/ingot.json".into(),
        json!({"template":"minecraft:stick","max_stack_size":16,"rarity":"rare"}).to_string(),
    ));
    declarations.push((
        "assets/gear/equipment/metal.json".into(),
        json!({"layers":{
            "humanoid":[{"texture":"gear:metal","model":"gear:armor/horn"}],
            "humanoid_leggings":[{"texture":"gear:metal","model":"gear:armor/horn_inner"}]
        }})
        .to_string(),
    ));
    let geometry =
        include_str!("../tests/fixtures/armor-geo/assets/armor_fixture/geo/armor/horn.geo.json");
    for name in ["horn", "horn_inner"] {
        declarations.push((
            format!("assets/gear/geo/armor/{name}.geo.json"),
            geometry.into(),
        ));
    }
    write("items.json", vanilla.clone());
    let borrowed: Vec<_> = declarations
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let m = folder_mod(&scratch.join("gear"), &borrowed);
    let out = scratch.join("root");
    build(&base, &[m], &out).unwrap();
    let items = Items::catalog(&out).unwrap();
    let equipment: Value =
        serde_json::from_slice(&fs::read(out.join("assets/gear/equipment/metal.json")).unwrap())
            .unwrap();
    assert_eq!(
        equipment["layers"]["humanoid"][0]["model"],
        "gear:armor/horn"
    );
    assert_eq!(
        equipment["layers"]["humanoid_leggings"][0]["model"],
        "gear:armor/horn_inner"
    );
    assert_eq!(
        fs::read_to_string(out.join("assets/gear/geo/armor/horn.geo.json")).unwrap(),
        geometry
    );

    for (slot, suffix) in [
        ("head", "helmet"),
        ("chest", "chestplate"),
        ("legs", "leggings"),
        ("feet", "boots"),
    ] {
        let id = format!("gear:{suffix}");
        let i = items.get(&id).unwrap();
        assert_eq!(i.equip_slot.as_deref(), Some(slot));
        assert_eq!(i.equip_asset.as_deref(), Some("gear:metal"));
        assert_eq!(
            (i.max_stack, i.max_damage, i.fire_resistant),
            (1, 1760, true)
        );
        assert_eq!(
            i.equip_sound.as_deref(),
            Some("minecraft:item.armor.equip_iron")
        );
        for (attribute, amount) in [
            ("armor", 7.0),
            ("armor_toughness", 6.0),
            ("knockback_resistance", 0.1),
        ] {
            assert!(i.modifiers.iter().any(|(a, _, n, op, s)| a
                == &format!("minecraft:{attribute}")
                && *n == amount
                && op == "add_value"
                && s == slot));
        }
        assert_eq!(items.enchantments.enchantable(&id), 30);
        assert_eq!(
            items.enchantments.props(&id).unwrap().repairable,
            ["gear:ingot"]
        );
    }
    for kind in ["sword", "pickaxe", "axe", "shovel", "hoe"] {
        let id = format!("gear:{kind}");
        let i = items.get(&id).unwrap();
        assert_eq!(
            (i.attack_damage, i.attack_speed, i.max_damage),
            (27.0, -2.4, 8000)
        );
        assert_eq!(i.weapon_damage, Some(1));
        assert_eq!(
            i.block_transformer.as_deref(),
            Some(format!("minecraft:{kind}").as_str())
        );
        assert!(items.item_tags[&format!("minecraft:enchantable/{kind}")].contains(&id));
        let held = crate::items::ItemStack::new(&id, 1);
        if kind == "sword" {
            assert_eq!(items.mining_speed(Some(&held), "minecraft:cobweb"), 15.0);
            assert!(!items.can_destroy_block(Some(&held), true));
        } else {
            assert_eq!(items.mining_speed(Some(&held), "minecraft:stone"), 10.0);
            assert!(items.correct_for_drops(Some(&held), "minecraft:stone"));
            assert!(!items.correct_for_drops(Some(&held), "minecraft:obsidian"));
        }
    }
    for kind in ["bow", "crossbow", "trident"] {
        let id = format!("gear:{kind}");
        let i = items.get(&id).unwrap();
        assert_eq!(items.behavior(&id), format!("minecraft:{kind}"));
        assert_eq!(i.id.as_ref(), id);
        assert_eq!(i.model, id);
        assert_eq!(i.max_damage, 584);
        assert_eq!((i.attack_damage, i.attack_speed), (12.0, -2.9));
        assert!(items.item_tags[&format!("minecraft:enchantable/{kind}")].contains(&id));
    }
    let plain = items.get("gear:plain_helmet").unwrap();
    assert_eq!(plain.max_damage, 123);
    assert_eq!(plain.equip_slot.as_deref(), Some("head"));
    assert!(!plain.fire_resistant);
    assert_eq!(items.enchantments.enchantable("gear:plain_helmet"), 0);
    assert_eq!(items.behavior("gear:unknown"), "gear:unknown");
    assert_eq!(items.get("gear:ingot").unwrap().max_stack, 16);
    // Source tables and their vanilla rows are never changed.
    let source: Value =
        serde_json::from_slice(&fs::read(base.join("items.json")).unwrap()).unwrap();
    let overlay: Value =
        serde_json::from_slice(&fs::read(out.join("items.json")).unwrap()).unwrap();
    assert_eq!(source, vanilla);
    for (id, components) in vanilla.as_object().unwrap() {
        assert_eq!(&overlay[id], components);
    }
    fs::remove_dir_all(scratch).unwrap();
}
