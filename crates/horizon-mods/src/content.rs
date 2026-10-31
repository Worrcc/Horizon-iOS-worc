//! A content mod's `assets/` and `data/` (API 0.2, docs/modpacks.md): the
//! files it adds to the game's resource tree, and the blocks, items and entities it
//! declares in `data/<id>/horizon/block/`, `data/<id>/horizon/item/` and
//! `data/<id>/horizon/entity/`.
//!
//! Content is data, not code: nothing here runs in the sandbox. What a mod
//! may add is fenced instead. Its own namespaces (its id and the ones its
//! manifest lists), plus additions to vanilla tags and to the convention
//! tags (`c`, which NeoForge and Fabric mods share). Paths are plain, files
//! and totals are capped, vanilla ids cannot be redefined, and no two mods
//! share a namespace ([`clash`]).
use crate::manifest::{Capability, MANIFEST, Manifest};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    io::Read,
    path::{Path, PathBuf},
};

/// Content files per mod, bytes per file and bytes per mod.
pub const MAX_FILES: usize = 65_536;
pub const MAX_FILE: u64 = 16 << 20;
pub const MAX_TOTAL: u64 = 512 << 20;

/// Checks a content path: `assets/` or `data/`, then `/`-separated parts of
/// `[a-z0-9_.-]`, none of them `.` or `..` (vanilla's resource-location
/// characters, so the path means the same on every OS).
pub fn check_path(name: &str) -> Result<(), String> {
    let bad = || format!("{name:?} is not a plain assets/ or data/ path");
    let mut parts = name.split('/');
    if !matches!(parts.next(), Some("assets" | "data")) {
        return Err(bad());
    }
    let mut count = 0;
    for part in parts {
        count += 1;
        let chars_ok = part
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
        if part.is_empty() || part == "." || part == ".." || !chars_ok {
            return Err(bad());
        }
    }
    // assets/<namespace>/<file> at least.
    if count < 2 {
        return Err(bad());
    }
    Ok(())
}

/// A vanilla identifier, `minecraft:path`.
fn vanilla_id(id: &str) -> bool {
    id.strip_prefix("minecraft:").is_some_and(|path| {
        !path.is_empty()
            && path
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
    })
}

/// What a declared block changes from its template; unset keeps the
/// template's value.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockProperties {
    /// Destroy time in seconds of hand mining (-1: unbreakable).
    pub hardness: Option<f32>,
    pub explosion_resistance: Option<f32>,
    /// Light emitted, 0 to 15.
    pub light: Option<u8>,
    pub requires_tool: Option<bool>,
    pub friction: Option<f32>,
    /// A `MapColor` id, 0 to 61.
    pub map_color: Option<u8>,
    /// A vanilla block whose sounds this block uses.
    pub sounds: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockFile {
    template: String,
    #[serde(default)]
    properties: BlockProperties,
    #[serde(default = "yes")]
    item: bool,
}
fn yes() -> bool {
    true
}

/// A block the mod adds: a vanilla template's behaviour, states and shapes
/// under a new id, with `properties` applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    /// `<mod id>:<path>`.
    pub id: String,
    /// The vanilla block it copies, `minecraft:<path>`.
    pub template: String,
    pub properties: BlockProperties,
    /// Also add a block item with the same id.
    pub item: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ItemFile {
    template: String,
    max_stack_size: Option<u8>,
    rarity: Option<Rarity>,
}

/// An item the mod adds: a vanilla template item's components under a new
/// id, with these changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub id: String,
    /// The vanilla item it copies, `minecraft:<path>`.
    pub template: String,
    pub max_stack_size: Option<u8>,
    pub rarity: Option<Rarity>,
}

/// The attributes an entity may set, vanilla's names without `minecraft:`.
pub const ENTITY_ATTRIBUTES: [&str; 11] = [
    "max_health",
    "movement_speed",
    "flying_speed",
    "attack_damage",
    "attack_knockback",
    "armor",
    "armor_toughness",
    "knockback_resistance",
    "follow_range",
    "step_height",
    "scale",
];
/// Vanilla's mob categories (`MobCategory`).
const CATEGORIES: [&str; 8] = [
    "monster",
    "creature",
    "ambient",
    "axolotls",
    "underground_water_creature",
    "water_creature",
    "water_ambient",
    "misc",
];

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntityFile {
    template: String,
    width: Option<f32>,
    height: Option<f32>,
    eye_height: Option<f32>,
    #[serde(default)]
    attributes: std::collections::BTreeMap<String, f64>,
    fire_immune: Option<bool>,
    category: Option<String>,
    texture: Option<String>,
    model: Option<String>,
    #[serde(default)]
    animations: std::collections::BTreeMap<String, String>,
}

/// The states a mod entity's own animations play in (docs/modpacks.md, entities).
pub const ANIMATION_STATES: &[&str] = &["idle", "walk", "attack"];

/// An entity the mod adds (API 0.5, first part): a vanilla template mob's behaviour (its AI,
/// model and sounds) under a new id, with its size, attributes and texture changed.
#[derive(Clone, Debug, PartialEq)]
pub struct Entity {
    /// `<mod id>:<path>`.
    pub id: String,
    /// The vanilla mob it copies, `minecraft:<path>`.
    pub template: String,
    pub width: Option<f32>,
    pub height: Option<f32>,
    pub eye_height: Option<f32>,
    /// Attribute values by name (`max_health`, ...), from [`ENTITY_ATTRIBUTES`].
    pub attributes: std::collections::BTreeMap<String, f64>,
    pub fire_immune: Option<bool>,
    pub category: Option<String>,
    /// The model's texture, `<namespace>:entity/<path>` for
    /// `assets/<namespace>/textures/entity/<path>.png` in one of the mod's namespaces.
    pub texture: Option<String>,
    /// Its own model, `<namespace>:<path>` for `assets/<namespace>/geo/<path>.geo.json`
    /// (Bedrock geometry, as GeckoLib reads it), in place of the template's.
    pub model: Option<String>,
    /// The animation each state plays (from [`ANIMATION_STATES`]), by name in the model's
    /// `assets/<namespace>/animations/<path>.animation.json`.
    pub animations: std::collections::BTreeMap<String, String>,
}

impl Entity {
    /// The model's geometry file and animation file in the mod.
    pub fn model_files(&self) -> Option<(String, String)> {
        model_files(self.model.as_deref()?)
    }
}
fn model_files(model: &str) -> Option<(String, String)> {
    let (ns, path) = model.split_once(':')?;
    Some((
        format!("assets/{ns}/geo/{path}.geo.json"),
        format!("assets/{ns}/animations/{path}.animation.json"),
    ))
}

/// The same safe resource identifier for entity JSON and per-mob texture orders.
pub fn entity_texture(name: &str) -> bool {
    name.split_once(':').is_some_and(|(ns, path)| {
        name.len() <= 32767
            && !ns.is_empty()
            && ns != "minecraft"
            && path.starts_with("entity/")
            && path.len() > "entity/".len()
            && check_path(&format!("assets/{ns}/textures/{path}.png")).is_ok()
    })
}

fn check_entity(file: &EntityFile, namespaces: &[String]) -> Result<(), String> {
    if !vanilla_id(&file.template) {
        return Err(format!(
            "template {:?} is not a vanilla entity (minecraft:...)",
            file.template
        ));
    }
    for (name, v) in [
        ("width", file.width),
        ("height", file.height),
        ("eye_height", file.eye_height),
    ] {
        if v.is_some_and(|v| !(v.is_finite() && v > 0.0 && v <= 64.0)) {
            return Err(format!("{name} is more than 0 and at most 64 blocks"));
        }
    }
    for (name, v) in &file.attributes {
        if !ENTITY_ATTRIBUTES.contains(&name.as_str()) {
            return Err(format!(
                "attribute {name:?} is not one of {}",
                ENTITY_ATTRIBUTES.join(", ")
            ));
        }
        if !v.is_finite() || *v < 0.0 || *v > 1024.0 {
            return Err(format!("attribute {name} is 0 to 1024"));
        }
    }
    if let Some(c) = &file.category
        && !CATEGORIES.contains(&c.as_str())
    {
        return Err(format!(
            "category {c:?} is not one of {}",
            CATEGORIES.join(", ")
        ));
    }
    if let Some(t) = &file.texture {
        let ok = t
            .split_once(':')
            .is_some_and(|(ns, _)| namespaces.iter().any(|n| n == ns) && entity_texture(t));
        if !ok {
            return Err(format!(
                "texture {t:?} is <namespace>:entity/<path>, in one of the mod's namespaces"
            ));
        }
    }
    if let Some(m) = &file.model {
        let ok = m.split_once(':').is_some_and(|(ns, _)| {
            namespaces.iter().any(|n| n == ns)
                && model_files(m).is_some_and(|(geo, _)| check_path(&geo).is_ok())
        });
        if !ok {
            return Err(format!(
                "model {m:?} is <namespace>:<path>, in one of the mod's namespaces"
            ));
        }
    }
    if !file.animations.is_empty() && file.model.is_none() {
        return Err("animations need the entity's own model".into());
    }
    for (state, name) in &file.animations {
        if !ANIMATION_STATES.contains(&state.as_str()) {
            return Err(format!(
                "animation state {state:?} is not one of {}",
                ANIMATION_STATES.join(", ")
            ));
        }
        if name.is_empty() {
            return Err(format!("animation state {state} names no animation"));
        }
    }
    Ok(())
}

#[derive(Debug)]
enum Source {
    Dir(PathBuf),
    Zip(PathBuf),
}

/// A mod's content: its files and declarations.
#[derive(Debug)]
pub struct Content {
    source: Source,
    /// Every content file (`assets/...`, `data/...`), sorted.
    pub files: Vec<String>,
    pub blocks: Vec<Block>,
    pub items: Vec<Item>,
    pub entities: Vec<Entity>,
    /// The namespaces it owns: the mod id, then the manifest's `namespaces`.
    pub namespaces: Vec<String>,
}

/// Reads a mod's content files ([`Content::reader`]).
pub enum FileReader<'a> {
    Dir(&'a Path),
    Zip(Box<zip::ZipArchive<std::fs::File>>),
}
impl FileReader<'_> {
    /// One content file's bytes.
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>, String> {
        check_path(name)?;
        let reader: Box<dyn Read + '_> = match self {
            Self::Dir(root) => {
                Box::new(std::fs::File::open(root.join(name)).map_err(|e| format!("{name}: {e}"))?)
            }
            Self::Zip(zip) => Box::new(zip.by_name(name).map_err(|e| format!("{name}: {e}"))?),
        };
        let mut out = Vec::new();
        reader
            .take(MAX_FILE + 1)
            .read_to_end(&mut out)
            .map_err(|e| format!("{name}: {e}"))?;
        if out.len() as u64 > MAX_FILE {
            return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
        }
        Ok(out)
    }
}

/// Why two content mods cannot be on together: a namespace both own (one's
/// id or listed namespace is the other's), which would mix their files.
/// `a` and `b` are (mod id, owned namespaces).
pub fn clash(a: (&str, &[String]), b: (&str, &[String])) -> Option<String> {
    let shared = a.1.iter().find(|ns| b.1.contains(ns))?;
    Some(format!(
        "mods {} and {} both use the namespace {shared}; turn one of them off",
        a.0, b.0
    ))
}

impl Content {
    /// The content of the mod at `path` (a `.hmod` or a mod folder).
    pub fn read(path: &Path, manifest: &Manifest) -> Result<Content, String> {
        let (source, sized) = if path.is_dir() {
            let mut out = Vec::new();
            for top in ["assets", "data"] {
                walk(path, &path.join(top), &mut out)?;
            }
            (Source::Dir(path.to_owned()), out)
        } else {
            let mut zip = open_zip(path)?;
            let mut out = Vec::new();
            for i in 0..zip.len() {
                let entry = zip.by_index_raw(i).map_err(|e| e.to_string())?;
                let name = entry.name();
                if (name.starts_with("assets/") || name.starts_with("data/")) && !entry.is_dir() {
                    out.push((name.to_owned(), entry.size()));
                }
            }
            (Source::Zip(path.to_owned()), out)
        };
        let mut files = Vec::with_capacity(sized.len());
        let mut total = 0u64;
        for (name, size) in sized {
            check_path(&name)?;
            if size > MAX_FILE {
                return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
            }
            total += size;
            files.push(name);
        }
        if files.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} content files"));
        }
        if total > MAX_TOTAL {
            return Err(format!("content is over {} MiB", MAX_TOTAL >> 20));
        }
        files.sort();
        files.dedup();
        let mut content = Content {
            source,
            files,
            blocks: Vec::new(),
            items: Vec::new(),
            entities: Vec::new(),
            namespaces: manifest.owned_namespaces().map(str::to_owned).collect(),
        };
        content.check(manifest)?;
        Ok(content)
    }

    /// Where the files are read from: the mod's folder or its `.hmod`.
    pub fn source(&self) -> &Path {
        match &self.source {
            Source::Dir(path) | Source::Zip(path) => path,
        }
    }

    /// No files: a mod with code only.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// One content file's bytes.
    pub fn file(&self, name: &str) -> Result<Vec<u8>, String> {
        self.reader()?.read(name)
    }

    /// A reader of its content files. A packaged mod's archive is opened
    /// once for all of them (opening it reads its whole index).
    pub fn reader(&self) -> Result<FileReader<'_>, String> {
        Ok(match &self.source {
            Source::Dir(root) => FileReader::Dir(root),
            Source::Zip(path) => FileReader::Zip(Box::new(open_zip(path)?)),
        })
    }

    /// Where content may go, and the declarations.
    fn check(&mut self, manifest: &Manifest) -> Result<(), String> {
        let id = manifest.id.as_str();
        if !self.files.is_empty() && !manifest.capabilities.contains(&Capability::Registries) {
            return Err(format!(
                "has assets/ or data/ but not capability registries in {MANIFEST}"
            ));
        }
        let mut blocks = Vec::new();
        let mut items = Vec::new();
        let mut entities = Vec::new();
        for name in &self.files {
            let mut parts = name.splitn(3, '/');
            let (top, namespace, rest) = (
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
                parts.next().unwrap_or(""),
            );
            let own = namespace == id;
            let owned = own || manifest.namespaces.iter().any(|n| n == namespace);
            // Vanilla's own files stay vanilla's; a mod may only add to its
            // tags, and to the convention tags mods share.
            let shared = matches!(namespace, "minecraft" | "c");
            let allowed = owned || (top == "data" && shared && rest.starts_with("tags/"));
            if !allowed {
                return Err(format!(
                    "{name}: a mod adds files in its own namespaces ({top}/{id}/ and those listed in namespaces) or to vanilla and convention (c) tags"
                ));
            }
            if !own && top == "data" && rest.starts_with("horizon/") {
                return Err(format!(
                    "{name}: blocks, items and entities are declared in data/{id}/horizon/"
                ));
            }
            let Some(decl) = (own && top == "data")
                .then(|| rest.strip_prefix("horizon/"))
                .flatten()
            else {
                continue;
            };
            let parse = |kind: &str| -> Option<String> {
                let path = decl
                    .strip_prefix(kind)?
                    .strip_prefix('/')?
                    .strip_suffix(".json")?;
                Some(format!("{id}:{path}"))
            };
            if let Some(block) = parse("block") {
                blocks.push((block, name.clone()));
            } else if let Some(item) = parse("item") {
                items.push((item, name.clone()));
            } else if let Some(entity) = parse("entity") {
                entities.push((entity, name.clone()));
            } else {
                return Err(format!(
                    "{name}: data/{id}/horizon/ holds block/*.json, item/*.json and entity/*.json only"
                ));
            }
        }
        let mut ids = BTreeSet::new();
        for (block, name) in blocks {
            let file: BlockFile =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            check_block(&file).map_err(|e| format!("{name}: {e}"))?;
            if file.item {
                ids.insert(block.clone());
            }
            self.blocks.push(Block {
                id: block,
                template: file.template,
                properties: file.properties,
                item: file.item,
            });
        }
        for (item, name) in items {
            let file: ItemFile =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if !vanilla_id(&file.template) {
                return Err(format!(
                    "{name}: template {:?} is not a vanilla item (minecraft:...)",
                    file.template
                ));
            }
            if file.max_stack_size.is_some_and(|n| !(1..=99).contains(&n)) {
                return Err(format!("{name}: max_stack_size is 1 to 99"));
            }
            if !ids.insert(item.clone()) {
                return Err(format!("{name}: {item} is also a block's item"));
            }
            self.items.push(Item {
                id: item,
                template: file.template,
                max_stack_size: file.max_stack_size,
                rarity: file.rarity,
            });
        }
        for (entity, name) in entities {
            let file: EntityFile =
                serde_json::from_slice(&self.file(&name)?).map_err(|e| format!("{name}: {e}"))?;
            check_entity(&file, &self.namespaces).map_err(|e| format!("{name}: {e}"))?;
            if let Some(t) = &file.texture {
                let (ns, path) = t.split_once(':').unwrap_or_default();
                let png = format!("assets/{ns}/textures/{path}.png");
                if self.files.binary_search(&png).is_err() {
                    return Err(format!("{name}: texture {t} has no {png}"));
                }
            }
            if let Some((geo, animations)) = file.model.as_deref().and_then(model_files) {
                if self.files.binary_search(&geo).is_err() {
                    return Err(format!("{name}: model has no {geo}"));
                }
                if !file.animations.is_empty() && self.files.binary_search(&animations).is_err() {
                    return Err(format!("{name}: animations need {animations}"));
                }
            }
            self.entities.push(Entity {
                id: entity,
                template: file.template,
                width: file.width,
                height: file.height,
                eye_height: file.eye_height,
                attributes: file.attributes,
                fire_immune: file.fire_immune,
                category: file.category,
                texture: file.texture,
                model: file.model,
                animations: file.animations,
            });
        }
        Ok(())
    }
}

fn check_block(file: &BlockFile) -> Result<(), String> {
    if !vanilla_id(&file.template) {
        return Err(format!(
            "template {:?} is not a vanilla block (minecraft:...)",
            file.template
        ));
    }
    let p = &file.properties;
    let finite = |v: Option<f32>| v.is_none_or(f32::is_finite);
    if !finite(p.hardness) || !finite(p.explosion_resistance) || !finite(p.friction) {
        return Err("hardness, explosion_resistance and friction are numbers".into());
    }
    if p.hardness.is_some_and(|h| h < 0.0 && h != -1.0) {
        return Err("hardness is 0 or more, or -1 (unbreakable)".into());
    }
    if p.explosion_resistance.is_some_and(|r| r < 0.0) {
        return Err("explosion_resistance is 0 or more".into());
    }
    if p.friction.is_some_and(|f| !(0.0..=2.0).contains(&f)) {
        return Err("friction is 0 to 2".into());
    }
    if p.light.is_some_and(|l| l > 15) {
        return Err("light is 0 to 15".into());
    }
    if p.map_color.is_some_and(|c| c > 61) {
        return Err("map_color is a MapColor id, 0 to 61".into());
    }
    if p.sounds.as_deref().is_some_and(|s| !vanilla_id(s)) {
        return Err("sounds names a vanilla block (minecraft:...)".into());
    }
    Ok(())
}

fn open_zip(path: &Path) -> Result<zip::ZipArchive<std::fs::File>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    zip::ZipArchive::new(file).map_err(|e| e.to_string())
}

/// Files under `dir`, relative to `root` with `/` separators. Symbolic links
/// are not followed: a mod folder holds its own files.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> Result<(), String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in read.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
            let name = parts.ok_or_else(|| format!("{} is not UTF-8", rel.display()))?;
            out.push((name.join("/"), meta.len()));
        }
        if out.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} content files"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Package;

    const MANIFEST_TEXT: &str = r#"
id = "ruby"
name = "Ruby"
version = "0.1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"#;
    const BLOCK: &[u8] = br#"{"template": "minecraft:iron_block",
        "properties": {"hardness": 4.0, "light": 3, "sounds": "minecraft:amethyst_block"}}"#;
    const ITEM: &[u8] = br#"{"template": "minecraft:diamond", "rarity": "uncommon"}"#;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("horizon-content-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn hmod(name: &str, manifest: &str, files: &[(&str, &[u8])]) -> Result<Content, String> {
        let dir = scratch(name);
        let path = dir.join("m.hmod");
        std::fs::write(&path, Package::write_files(manifest, None, files)?).unwrap();
        let package = Package::read(&path);
        let content = package.and_then(|p| Content::read(&path, &p.manifest));
        std::fs::remove_dir_all(&dir).unwrap();
        content
    }

    #[test]
    fn content_only_mod_round_trips() {
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/block/ruby_block.json", BLOCK),
            ("data/ruby/horizon/item/ruby.json", ITEM),
            ("data/ruby/recipe/ruby_block.json", b"{}"),
            ("data/minecraft/tags/block/mineable/pickaxe.json", b"{}"),
            ("data/c/tags/item/gems/ruby.json", b"{}"),
            ("assets/ruby/textures/block/ruby_block.png", b"png"),
        ];
        let content = hmod("ok", MANIFEST_TEXT, files).unwrap();
        assert_eq!(content.files.len(), 6);
        assert_eq!(
            content.blocks,
            [Block {
                id: "ruby:ruby_block".into(),
                template: "minecraft:iron_block".into(),
                properties: BlockProperties {
                    hardness: Some(4.0),
                    light: Some(3),
                    sounds: Some("minecraft:amethyst_block".into()),
                    ..BlockProperties::default()
                },
                item: true,
            }]
        );
        assert_eq!(content.items[0].id, "ruby:ruby");
        assert_eq!(content.items[0].rarity, Some(Rarity::Uncommon));
    }

    const ENTITY: &[u8] = br#"{"template": "minecraft:zombie", "width": 1.2, "height": 2.4,
        "attributes": {"max_health": 60, "attack_damage": 7}, "fire_immune": true,
        "category": "monster", "texture": "ruby:entity/golem"}"#;

    #[test]
    fn entities_copy_a_vanilla_mob() {
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/entity/ruby_golem.json", ENTITY),
            ("assets/ruby/textures/entity/golem.png", b"png"),
        ];
        let content = hmod("entity", MANIFEST_TEXT, files).unwrap();
        let golem = &content.entities[0];
        assert_eq!(golem.id, "ruby:ruby_golem");
        assert_eq!(golem.template, "minecraft:zombie");
        assert_eq!((golem.width, golem.height), (Some(1.2), Some(2.4)));
        assert_eq!(golem.attributes["max_health"], 60.0);
        assert_eq!(golem.fire_immune, Some(true));
        assert_eq!(golem.texture.as_deref(), Some("ruby:entity/golem"));
        assert_eq!(golem.model, None);
    }

    #[test]
    fn entities_bring_their_own_model() {
        let decl = br#"{"template": "minecraft:zombie", "model": "ruby:golem",
            "animations": {"idle": "animation.golem.idle", "walk": "animation.golem.walk"}}"#;
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/entity/ruby_golem.json", decl),
            ("assets/ruby/geo/golem.geo.json", b"{}"),
            ("assets/ruby/animations/golem.animation.json", b"{}"),
        ];
        let content = hmod("entity-model", MANIFEST_TEXT, files).unwrap();
        let golem = &content.entities[0];
        assert_eq!(golem.model.as_deref(), Some("ruby:golem"));
        assert_eq!(golem.animations["walk"], "animation.golem.walk");
        assert_eq!(
            golem.model_files(),
            Some((
                "assets/ruby/geo/golem.geo.json".into(),
                "assets/ruby/animations/golem.animation.json".into()
            ))
        );
        let refused = |decl: &str, files: &[(&str, &[u8])]| {
            let mut all: Vec<(&str, &[u8])> =
                vec![("data/ruby/horizon/entity/a.json", decl.as_bytes())];
            all.extend_from_slice(files);
            hmod("bad-model", MANIFEST_TEXT, &all).unwrap_err()
        };
        let geo: &[(&str, &[u8])] = &[("assets/ruby/geo/golem.geo.json", b"{}")];
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "model": "ruby:golem"}"#,
                &[]
            )
            .contains("has no assets/ruby/geo/golem.geo.json")
        );
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "model": "minecraft:golem"}"#,
                geo
            )
            .contains("mod's namespaces")
        );
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "animations": {"idle": "a"}}"#,
                &[]
            )
            .contains("own model")
        );
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "model": "ruby:golem", "animations": {"fly": "a"}}"#,
                geo
            )
            .contains("animation state \"fly\"")
        );
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "model": "ruby:golem", "animations": {"idle": "a"}}"#,
                geo
            )
            .contains("animations need assets/ruby/animations/golem.animation.json")
        );
    }

    #[test]
    fn refuses_entities_it_cannot_load() {
        let refused = |decl: &str, texture: bool| {
            let mut files: Vec<(&str, &[u8])> =
                vec![("data/ruby/horizon/entity/a.json", decl.as_bytes())];
            if texture {
                files.push(("assets/ruby/textures/entity/golem.png", b"png"));
            }
            hmod("bad-entity", MANIFEST_TEXT, &files).unwrap_err()
        };
        assert!(refused(r#"{"template": "ruby:other"}"#, false).contains("not a vanilla entity"));
        assert!(
            refused(r#"{"template": "minecraft:zombie", "height": 0}"#, false).contains("height")
        );
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "attributes": {"luck": 1}}"#,
                false
            )
            .contains("attribute \"luck\"")
        );
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "category": "boss"}"#,
                false
            )
            .contains("category")
        );
        // Another mod's texture, and one the mod doesn't carry.
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "texture": "minecraft:entity/zombie/zombie"}"#,
                true
            )
            .contains("mod's namespaces")
        );
        assert!(
            refused(
                r#"{"template": "minecraft:zombie", "texture": "ruby:entity/missing"}"#,
                true
            )
            .contains("has no assets/ruby/textures/entity/missing.png")
        );
        assert!(
            refused(r#"{"template": "minecraft:zombie", "ai": "x"}"#, false)
                .contains("unknown field")
        );
    }

    #[test]
    fn folder_mod_reads_the_same() {
        let dir = scratch("folder");
        std::fs::write(dir.join(MANIFEST), MANIFEST_TEXT).unwrap();
        let block = dir.join("data/ruby/horizon/block");
        std::fs::create_dir_all(&block).unwrap();
        std::fs::write(block.join("ruby_block.json"), BLOCK).unwrap();
        let package = Package::read(&dir).unwrap();
        assert!(package.wasm.is_none());
        let content = Content::read(&dir, &package.manifest).unwrap();
        assert_eq!(content.files, ["data/ruby/horizon/block/ruby_block.json"]);
        assert_eq!(content.file(&content.files[0]).unwrap(), BLOCK);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_content_outside_its_fence() {
        let refused = |name: &str, files: &[(&str, &[u8])]| {
            let e = hmod(name, MANIFEST_TEXT, files).unwrap_err();
            assert!(!e.is_empty());
            e
        };
        let e = refused("other-ns", &[("data/other/horizon/block/a.json", BLOCK)]);
        assert!(e.contains("own namespaces"), "{e}");
        refused(
            "vanilla-asset",
            &[("assets/minecraft/textures/block/stone.png", b"x")],
        );
        refused(
            "vanilla-recipe",
            &[("data/minecraft/recipe/stone.json", b"{}")],
        );
        refused("convention-recipe", &[("data/c/recipe/stone.json", b"{}")]);
        refused("convention-asset", &[("assets/c/lang/en_us.json", b"{}")]);
        let e = refused(
            "modded-template",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template": "ruby:other"}"#,
            )],
        );
        assert!(e.contains("not a vanilla block"), "{e}");
        refused(
            "light",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template": "minecraft:stone", "properties": {"light": 16}}"#,
            )],
        );
        refused(
            "unknown-key",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template": "minecraft:stone", "glow": true}"#,
            )],
        );
        let e = refused(
            "clash",
            &[
                ("data/ruby/horizon/block/a.json", BLOCK),
                ("data/ruby/horizon/item/a.json", ITEM),
            ],
        );
        assert!(e.contains("also a block's item"), "{e}");
        refused("other-kind", &[("data/ruby/horizon/entity/a.json", b"{}")]);
        refused(
            "stack",
            &[(
                "data/ruby/horizon/item/a.json",
                br#"{"template": "minecraft:stick", "max_stack_size": 100}"#,
            )],
        );
    }

    #[test]
    fn listed_namespaces_hold_content() {
        let manifest = format!("{MANIFEST_TEXT}namespaces = [\"kaisyn\"]\n");
        let files: &[(&str, &[u8])] = &[
            ("data/kaisyn/worldgen/template_pool/a.json", b"{}"),
            ("data/kaisyn/structure/a.nbt", b"nbt"),
            ("assets/kaisyn/lang/en_us.json", b"{}"),
        ];
        let content = hmod("listed", &manifest, files).unwrap();
        assert_eq!(content.files.len(), 3);
        assert_eq!(content.namespaces, ["ruby", "kaisyn"]);
        // Declarations stay in the id's namespace.
        let decl: &[(&str, &[u8])] = &[("data/kaisyn/horizon/block/a.json", BLOCK)];
        let e = hmod("listed-decl", &manifest, decl).unwrap_err();
        assert!(e.contains("declared in data/ruby/horizon/"), "{e}");
        // A namespace it does not list is still outside.
        let other: &[(&str, &[u8])] = &[("data/other/structure/a.nbt", b"nbt")];
        assert!(hmod("listed-other", &manifest, other).is_err());
    }

    #[test]
    fn mods_sharing_a_namespace_clash() {
        let t_and_t = ["t_and_t".to_owned(), "kaisyn".to_owned()];
        let kaisyn = ["kaisyn".to_owned()];
        let ruby = ["ruby".to_owned(), "gems".to_owned()];
        assert_eq!(
            clash(("t_and_t", &t_and_t), ("kaisyn", &kaisyn)).as_deref(),
            Some("mods t_and_t and kaisyn both use the namespace kaisyn; turn one of them off")
        );
        assert_eq!(clash(("t_and_t", &t_and_t), ("ruby", &ruby)), None);
    }

    #[test]
    fn content_needs_the_registries_capability() {
        let code_only = MANIFEST_TEXT
            .replace("api = \"0.2\"", "api = \"0.1\"")
            .replace("capabilities = [\"registries\"]", "capabilities = []");
        // No code and no registries: nothing to load.
        let e = hmod("empty", &code_only, &[]).unwrap_err();
        assert!(e.contains("mod.wasm: missing"), "{e}");
        // Registries on API 0.1.
        let old = MANIFEST_TEXT.replace("api = \"0.2\"", "api = \"0.1\"");
        assert!(Manifest::parse(&old).unwrap_err().contains("needs api"));
        // Content without the capability, in a mod with code.
        let dir = scratch("no-cap");
        let path = dir.join("m.hmod");
        let files: &[(&str, &[u8])] = &[("assets/ruby/lang/en_us.json", b"{}")];
        std::fs::write(
            &path,
            Package::write_files(&code_only, Some(b"\0asm"), files).unwrap(),
        )
        .unwrap();
        let package = Package::read(&path).unwrap();
        let e = Content::read(&path, &package.manifest).err().unwrap();
        assert!(e.contains("not capability registries"), "{e}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn paths_are_plain() {
        assert!(check_path("assets/ruby/textures/block/ruby_block.png").is_ok());
        assert!(check_path("data/ruby/loot_table/blocks/a.json").is_ok());
        for bad in [
            "assets/ruby",
            "assets/../x/y",
            "assets/ruby//a",
            "assets/Ruby/a",
            "assets/ruby/a b",
            "/assets/ruby/a",
            "other/ruby/a",
            "assets\\ruby\\a",
        ] {
            assert!(check_path(bad).is_err(), "{bad}");
        }
    }
}
