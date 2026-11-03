//! What the configuration phase sends: the synchronized registries (names in
//! network order, with their data for clients that do not know the vanilla
//! pack) and the tags of every registry, loaded from the prepared resources,
//! the content overlay and the registries report.
use horizon_protocol::{
    packets::configuration::{RegistryData, RegistryEntry, RegistryTags, TagEntry, UpdateTags},
    registry::{Builtin, Dynamic},
};
use horizon_world::nbt::{JavaString, Tag};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::{Path, PathBuf},
};

/// RegistryDataLoader.SYNCHRONIZED_REGISTRIES, in vanilla's order.
pub const SYNCHRONIZED: &[&str] = &[
    "minecraft:worldgen/biome",
    "minecraft:chat_type",
    "minecraft:trim_pattern",
    "minecraft:trim_material",
    "minecraft:wolf_variant",
    "minecraft:wolf_sound_variant",
    "minecraft:pig_variant",
    "minecraft:pig_sound_variant",
    "minecraft:frog_variant",
    "minecraft:cat_variant",
    "minecraft:cat_sound_variant",
    "minecraft:cow_sound_variant",
    "minecraft:cow_variant",
    "minecraft:chicken_sound_variant",
    "minecraft:chicken_variant",
    "minecraft:zombie_nautilus_variant",
    "minecraft:painting_variant",
    "minecraft:sulfur_cube_archetype",
    "minecraft:dimension_type",
    "minecraft:damage_type",
    "minecraft:banner_pattern",
    "minecraft:enchantment",
    "minecraft:jukebox_song",
    "minecraft:instrument",
    "minecraft:test_environment",
    "minecraft:test_instance",
    "minecraft:dialog",
    "minecraft:world_clock",
    "minecraft:timeline",
    "minecraft:decorated_pot_pattern",
    "minecraft:block_transformer",
    "minecraft:worldgen/block_state_provider",
];
/// KnownPack.vanilla("core") for this version.
pub const CORE_PACK: (&str, &str, &str) = ("minecraft", "core", horizon_protocol::GAME_VERSION);

/// A data-driven registry: entry names in network order and their JSON.
struct Entries {
    names: Vec<String>,
    json: Vec<Value>,
}

pub struct SyncData {
    pub builtin: Builtin,
    /// Entry names per synchronized registry (for id lookups).
    pub dynamic: Dynamic,
    registries: Vec<(String, Entries)>,
    pub tags: UpdateTags,
    pub features: Vec<String>,
    /// Vanilla's command tree (the commands report), when found.
    pub commands: Option<super::commands::CommandTree>,
    /// ClientboundUpdateRecipesPacket's payload.
    pub recipes: Vec<u8>,
    pub advancements: super::advancements::ServerAdvancements,
}

/// Every `*.json` under `dir`, as (path without extension relative to dir, value), sorted.
fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Value>) {
    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            walk(&path, &format!("{prefix}{name}/"), out);
        } else if let Some(stem) = name.strip_suffix(".json")
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(value) = serde_json::from_str::<Value>(&text)
        {
            out.insert(format!("{prefix}{stem}"), value);
        }
    }
}

/// A JSON value as the NBT that vanilla's NbtOps would produce from the codec
/// (integers are ints, other numbers doubles, booleans bytes).
pub fn json_to_nbt(v: &Value) -> Tag {
    match v {
        Value::Null => Tag::compound(Vec::<(&str, Tag)>::new()),
        Value::Bool(b) => Tag::Byte(i8::from(*b)),
        Value::Number(n) => match n.as_i64() {
            Some(i) => match i32::try_from(i) {
                Ok(i) => Tag::Int(i),
                Err(_) => Tag::Long(i),
            },
            None => Tag::Double(n.as_f64().unwrap_or(0.0).to_bits()),
        },
        Value::String(s) => Tag::String(JavaString::from(s.as_str())),
        Value::Array(a) => Tag::List(a.iter().map(json_to_nbt).collect()),
        Value::Object(o) => Tag::compound(o.iter().map(|(k, v)| (k.as_str(), json_to_nbt(v)))),
    }
}

/// A component's JSON as NbtOps writes it. A list whose elements differ in type
/// (translation arguments mixing numbers and components) stays one list: the NBT
/// writer wraps each element that isn't a compound under the empty key, as
/// `ListTag.wrapIfNeeded` does, so wrapping it here too would leave the client a
/// `{"": 1}` argument it cannot read.
pub fn component_nbt(v: &Value) -> Tag {
    match v {
        Value::Array(a) => Tag::List(a.iter().map(component_nbt).collect()),
        Value::Object(o) => Tag::compound(o.iter().map(|(k, v)| (k.as_str(), component_nbt(v)))),
        other => json_to_nbt(other),
    }
}

impl SyncData {
    pub fn load(resources: &Path) -> Result<Self, String> {
        let builtin = Builtin::find(resources)
            .and_then(|p| Builtin::load(&p).ok())
            .ok_or("registries.json not found next to the resources")?;
        let root: PathBuf = resources.join("data/minecraft");
        let mut biome_namespaces = horizon_items::content_namespaces::namespaces(resources);
        biome_namespaces.retain(|ns| ns != "minecraft");
        biome_namespaces.sort();
        biome_namespaces.dedup();
        let mut registries = Vec::new();
        let mut dynamic = Dynamic::default();
        for key in SYNCHRONIZED {
            let path = key.trim_start_matches("minecraft:");
            let mut found = BTreeMap::new();
            walk(&root.join(path), "", &mut found);
            let (mut names, mut json): (Vec<_>, Vec<_>) = found
                .into_iter()
                .map(|(n, v)| (format!("minecraft:{n}"), v))
                .unzip();
            // Preserve vanilla's network IDs, then append owned content biomes
            // in resource-name order, just as the local biome loader does.
            if *key == "minecraft:worldgen/biome" {
                for ns in &biome_namespaces {
                    let mut found = BTreeMap::new();
                    walk(&resources.join("data").join(ns).join(path), "", &mut found);
                    for (name, value) in found {
                        names.push(format!("{ns}:{name}"));
                        json.push(value);
                    }
                }
            }
            dynamic.registries.insert(
                (*key).to_owned(),
                names.iter().map(|n| (n.clone(), None)).collect(),
            );
            registries.push(((*key).to_owned(), Entries { names, json }));
        }
        let tags = Self::load_tags(resources, &builtin, &dynamic);
        let mut sync = Self {
            builtin,
            dynamic,
            registries,
            tags,
            features: vec!["minecraft:vanilla".to_owned()],
            commands: super::commands::CommandTree::load(resources),
            recipes: Vec::new(),
            advancements: super::advancements::ServerAdvancements::load(resources),
        };
        sync.recipes = super::recipes::update_recipes(resources, &sync);
        Ok(sync)
    }

    /// The id of a name in a built-in or synchronized registry.
    pub fn id(&self, registry: &str, name: &str) -> Option<i32> {
        if let Some(ids) = self.builtin.get(registry) {
            return ids.id(name);
        }
        self.dynamic
            .registries
            .get(registry)?
            .iter()
            .position(|(n, _)| n == name)
            .map(|i| i as i32)
    }

    /// ClientboundRegistryDataPacket per registry. With `known_pack` the
    /// client rebuilds vanilla entries from its own data. Content entries are
    /// outside the known core pack and always carry their data.
    pub fn registry_packets(&self, known_pack: bool) -> Vec<RegistryData> {
        self.registries
            .iter()
            .map(|(key, e)| RegistryData {
                registry: key.clone(),
                entries: e
                    .names
                    .iter()
                    .zip(&e.json)
                    .map(|(name, json)| RegistryEntry {
                        id: name.clone(),
                        data: if known_pack && name.starts_with("minecraft:") {
                            None
                        } else {
                            Some(json_to_nbt(json))
                        },
                    })
                    .collect(),
            })
            .collect()
    }

    fn registry_names<'a>(
        builtin: &'a Builtin,
        dynamic: &'a Dynamic,
        key: &str,
    ) -> Option<HashMap<&'a str, i32>> {
        if let Some(ids) = builtin.get(key) {
            return Some(
                ids.names
                    .iter()
                    .enumerate()
                    .map(|(i, n)| (n.as_str(), i as i32))
                    .collect(),
            );
        }
        dynamic.registries.get(key).map(|v| {
            v.iter()
                .enumerate()
                .map(|(i, (n, _))| (n.as_str(), i as i32))
                .collect()
        })
    }

    /// TagNetworkSerialization.serializeTagsToNetwork from the tag files.
    fn load_tags(resources: &Path, builtin: &Builtin, dynamic: &Dynamic) -> UpdateTags {
        let mut namespaces = horizon_items::content_namespaces::tag_namespaces(resources);
        namespaces.sort();
        namespaces.dedup();
        let mut registries = Vec::new();
        let mut keys: Vec<String> = builtin
            .registries
            .keys()
            .chain(dynamic.registries.keys())
            .cloned()
            .collect();
        keys.sort();
        keys.dedup();
        for key in keys {
            let mut files = BTreeMap::new();
            for ns in &namespaces {
                let dir = resources
                    .join("data")
                    .join(ns)
                    .join("tags")
                    .join(key.trim_start_matches("minecraft:"));
                walk(&dir, &format!("{ns}:"), &mut files);
            }
            if files.is_empty() {
                continue;
            }
            let Some(names) = Self::registry_names(builtin, dynamic, &key) else {
                continue;
            };
            let mut tags = Vec::new();
            for name in files.keys() {
                let mut ids = Vec::new();
                let mut seen = HashSet::new();
                Self::expand(&files, name, &names, &mut ids, &mut seen);
                tags.push(TagEntry {
                    name: name.clone(),
                    ids,
                });
            }
            registries.push(RegistryTags {
                registry: key,
                tags,
            });
        }
        UpdateTags { registries }
    }

    fn expand(
        files: &BTreeMap<String, Value>,
        tag: &str,
        names: &HashMap<&str, i32>,
        out: &mut Vec<i32>,
        seen: &mut HashSet<String>,
    ) {
        if !seen.insert(tag.to_owned()) {
            return;
        }
        let Some(values) = files.get(tag).and_then(|v| v["values"].as_array()) else {
            return;
        };
        for value in values {
            let id = value
                .as_str()
                .or_else(|| value["id"].as_str())
                .unwrap_or_default();
            if let Some(reference) = id.strip_prefix('#') {
                let reference = if reference.contains(':') {
                    reference.to_owned()
                } else {
                    format!("minecraft:{reference}")
                };
                Self::expand(files, &reference, names, out, seen);
            } else if let Some(&n) = names.get(id)
                && !out.contains(&n)
            {
                out.push(n);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_becomes_nbt() {
        let v: Value = serde_json::from_str(r#"{"a":1,"b":[true,2.5],"c":"x"}"#).unwrap();
        let t = json_to_nbt(&v);
        assert!(matches!(t.get("a"), Some(Tag::Int(1))));
        assert!(matches!(t.get("c"), Some(Tag::String(_))));
        assert!(matches!(t.get("b"), Some(Tag::List(l)) if l.len() == 2));
    }

    #[test]
    fn content_biomes_append_in_resource_order_and_require_ownership() {
        let root = std::env::temp_dir().join(format!(
            "horizon-sync-content-biomes-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("registries.json", "{}");
        for name in [
            "minecraft:plains",
            "minecraft:desert",
            "aaa:zebra",
            "aaa:groves/frost",
            "zzz:flats",
            "unowned:ignored",
        ] {
            let (ns, path) = name.split_once(':').unwrap();
            write(&format!("data/{ns}/worldgen/biome/{path}.json"), "{}");
        }
        write(
            "data/minecraft/tags/worldgen/biome/test.json",
            r##"{"values":["minecraft:plains", "#nested"]}"##,
        );
        write(
            "data/minecraft/tags/worldgen/biome/nested.json",
            r#"{"values":["minecraft:desert"]}"#,
        );
        let registry = "minecraft:worldgen/biome";
        let vanilla = SyncData::load(&root).unwrap();
        assert_eq!(vanilla.dynamic.len(registry), 2);
        assert_eq!(vanilla.id(registry, "minecraft:desert"), Some(0));
        assert_eq!(vanilla.id(registry, "minecraft:plains"), Some(1));
        let tags = &vanilla.tags.registries[0].tags;
        assert_eq!(
            tags.iter()
                .find(|t| t.name == "minecraft:test")
                .unwrap()
                .ids,
            [1, 0]
        );
        assert!(
            vanilla
                .registry_packets(true)
                .iter()
                .all(|p| p.entries.iter().all(|e| e.data.is_none()))
        );
        write(
            horizon_items::content_namespaces::MARKER,
            r#"["zzz","aaa","minecraft","aaa"]"#,
        );
        let content = SyncData::load(&root).unwrap();
        let names: Vec<_> = content.dynamic.registries[registry]
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "minecraft:desert",
                "minecraft:plains",
                "aaa:groves/frost",
                "aaa:zebra",
                "zzz:flats"
            ]
        );
        assert_eq!(content.id(registry, "unowned:ignored"), None);
        // An empty owned namespace preserves the complete vanilla packet list.
        write(horizon_items::content_namespaces::MARKER, r#"["empty"]"#);
        let empty = SyncData::load(&root).unwrap();
        for known_pack in [false, true] {
            assert_eq!(
                empty.registry_packets(known_pack),
                vanilla.registry_packets(known_pack)
            );
        }
        assert_eq!(empty.tags, vanilla.tags);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use horizon_world::nbt::{Limits, read_network, write_network};

    /// `/give`'s reply: a count, then components. On the wire each element is wrapped
    /// once, and read back (ListTag.addAndUnwrap) it is the number again.
    #[test]
    fn mixed_translation_arguments_are_wrapped_once() {
        let json = serde_json::json!({
            "translate": "commands.give.success.single",
            "with": [1, {"text": "Raw Iron"}, {"text": "Tester"}],
        });
        let tag = component_nbt(&json);
        let Some(Tag::List(args)) = tag.get("with") else {
            panic!("no arguments: {tag:?}");
        };
        assert_eq!(args[0], Tag::Int(1));
        // Read back as ListTag.addAndUnwrap does: a wrapper wrapped twice stays a compound.
        let bytes = write_network(&tag, Limits::default()).unwrap();
        assert_eq!(read_network(&bytes, Limits::default()).unwrap().0, tag);
    }
}
