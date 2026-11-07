//! A content mod's `assets/` and `data/` (API 0.2, docs/modpacks.md): the
//! files it adds to the game's resource tree, and the blocks, items, entities, mob
//! effects, creative tabs, entity types and menu types it declares in
//! `data/<id>/horizon/{block,item,entity,mob_effect,creative_tab,entity_type,menu}/`.
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
    collections::{BTreeMap, BTreeSet},
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
/// `Identifier.isValidNamespace` and `isValidPath` on an id with both parts
/// given: `[a-z0-9_.-]`, and `/` in the path too.
fn namespaced_id(id: &str) -> bool {
    let ok = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b);
    id.split_once(':').is_some_and(|(namespace, path)| {
        !namespace.is_empty()
            && !path.is_empty()
            && namespace.bytes().all(ok)
            && path.bytes().all(|b| ok(b) || b == b'/')
    })
}

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
    /// Which states tick randomly, whatever the template does (`isRandomlyTicking`).
    pub random_ticks: Option<RandomTicks>,
    /// How the block is drawn, whatever the template does (`BlockBehaviour.getRenderShape`).
    pub render_shape: Option<RenderShape>,
    /// `false`: the block occludes nothing, so its neighbours' faces are drawn and light passes
    /// it (`BlockBehaviour.Properties.noOcclusion`); unset or `true`: as its template.
    pub occludes: Option<bool>,
}

/// Vanilla's `RenderShape`, by its constant's name: `MODEL` puts the block model in the chunk
/// mesh, `ENTITYBLOCK_ANIMATED` leaves it out for a block whose parts are drawn every frame
/// (its block entity's drawing), and `INVISIBLE` draws nothing.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RenderShape {
    Model,
    EntityblockAnimated,
    Invisible,
}

impl RenderShape {
    /// The constant's name, as the block tables write it.
    pub fn name(self) -> &'static str {
        match self {
            RenderShape::Model => "MODEL",
            RenderShape::EntityblockAnimated => "ENTITYBLOCK_ANIMATED",
            RenderShape::Invisible => "INVISIBLE",
        }
    }
}

/// Which of a block's states get random ticks (`BlockBehaviour.isRandomlyTicking(state)`), so a
/// crop, sapling or soil on a template that doesn't tick still grows. The sections' random tick
/// counts follow it, and `native_random_tick` fires for the states it picks.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum RandomTicks {
    /// Every state (`true`), or none (`false`, for a template that ticks).
    All(bool),
    /// The states whose own state property `property` is one of `values`.
    States {
        property: String,
        values: Vec<String>,
    },
}

impl RandomTicks {
    /// Whether a state ticks, `combo` being its values of the block's own state properties.
    pub fn applies(&self, combo: &[(String, String)]) -> bool {
        match self {
            RandomTicks::All(all) => *all,
            RandomTicks::States { property, values } => combo
                .iter()
                .any(|(name, value)| name == property && values.contains(value)),
        }
    }
}

/// Boxes selected by one property (legacy) or an ordered tuple of properties.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BlockShapes {
    #[serde(default)]
    pub property: String,
    #[serde(default)]
    pub properties: Vec<String>,
    #[serde(default)]
    pub values: BTreeMap<String, Vec<[f64; 6]>>,
    pub default: Option<Vec<[f64; 6]>>,
}

impl BlockShapes {
    pub fn property_names(&self) -> Vec<&str> {
        if self.property.is_empty() {
            self.properties.iter().map(String::as_str).collect()
        } else {
            vec![self.property.as_str()]
        }
    }

    fn check(&self) -> Result<(), String> {
        let names = self.property_names();
        if (!self.property.is_empty() && !self.properties.is_empty())
            || names.iter().any(|name| !state_name(name))
            || names.iter().collect::<BTreeSet<_>>().len() != names.len()
            || (names.is_empty() && !self.values.is_empty())
            || (self.values.is_empty() && self.default.is_none())
        {
            return Err("shapes needs distinct properties and values, or a default".into());
        }
        for key in self.values.keys() {
            let values: Vec<_> = key.split(',').collect();
            if values.len() != names.len() || values.iter().any(|value| !state_name(value)) {
                return Err(format!("invalid shape key {key:?}: one value per property"));
            }
        }
        for boxes in self.values.values().chain(self.default.iter()) {
            if boxes.len() > 16
                || boxes.iter().any(|bounds| {
                    bounds.iter().any(|value| !value.is_finite())
                        || (0..3).any(|axis| bounds[axis] > bounds[axis + 3])
                })
            {
                return Err("shape needs at most 16 finite boxes with ordered bounds".into());
            }
        }
        Ok(())
    }
}

/// A state property a block declares on its own, beside its template's.
/// A block's states become the template's states times these values, so a
/// pipe can have a connection per side without a template that has one.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum StateProperty {
    /// `false` and `true`, in vanilla's spelling.
    Boolean,
    /// A list of values, in the order the state space is built in.
    Enum { values: Vec<String> },
}

impl StateProperty {
    /// Its values, in the order the state space is built in; a boolean's two
    /// come `false` first, as vanilla orders them.
    pub fn values(&self) -> Vec<&str> {
        match self {
            StateProperty::Boolean => vec!["false", "true"],
            StateProperty::Enum { values } => values.iter().map(String::as_str).collect(),
        }
    }
}

/// A vanilla state property's name, and a value's: lower case, digits and
/// underscores (`minecraft:oak_log` is `axis`, a repeater is `delay`).
fn state_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockFile {
    template: String,
    #[serde(default)]
    properties: BlockProperties,
    shapes: Option<BlockShapes>,
    collision_shapes: Option<BlockShapes>,
    #[serde(default)]
    state_properties: BTreeMap<String, StateProperty>,
    #[serde(default)]
    default_state: BTreeMap<String, String>,
    #[serde(default)]
    connects_to: Option<String>,
    #[serde(default)]
    thickness: Option<f32>,
    #[serde(default)]
    wood_type: Option<String>,
    #[serde(default)]
    wall_block: Option<String>,
    #[serde(default = "yes")]
    item: bool,
    #[serde(default)]
    components: BTreeMap<String, serde_json::Value>,
}
fn yes() -> bool {
    true
}

/// A block the mod adds: a vanilla template's behaviour, states and shapes
/// under a new id, with `properties` applied. A native mod may build one in
/// code (`native_blocks`): `..Default::default()` leaves the rest as a file
/// leaves it out, except `item`, which a file sets unless told not to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Block {
    /// `<mod id>:<path>`.
    pub id: String,
    /// The vanilla block it copies, `minecraft:<path>`.
    pub template: String,
    pub properties: BlockProperties,
    pub shapes: Option<BlockShapes>,
    pub collision_shapes: Option<BlockShapes>,
    /// State properties of its own, which its template does not have.
    pub state_properties: BTreeMap<String, StateProperty>,
    /// State properties overlaid on the template default.
    pub default_state: BTreeMap<String, String>,
    /// Also add a block item with the same id.
    pub item: bool,
    /// Its item's component patch, as an [`Item`]'s.
    pub components: BTreeMap<String, serde_json::Value>,
    /// A connecting block (template `minecraft:chorus_plant`, a `PipeBlock`
    /// without the chorus plant's rules): the block tag, without `#`, whose
    /// blocks it joins besides itself (a cable's machines).
    pub connects_to: Option<String>,
    /// A connecting block's width in pixels (2 to 14): its shapes, the
    /// chorus plant's (10) when unset.
    pub thickness: Option<f32>,
    /// A sign's `WoodType` (template a vanilla sign), which names its editor's
    /// background: `<namespace>:<path>` a mod's texture, a bare name vanilla's.
    /// The template's when unset.
    pub wood_type: Option<String>,
    /// The block a standing or ceiling sign's item places on a wall
    /// (`StandingAndWallBlockItem.wallBlock`): one of this mod's blocks, whose
    /// template is the matching wall sign and which has no item. The
    /// template's when unset.
    pub wall_block: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CreativeTabFile {
    title: String,
    icon: String,
    items: Vec<String>,
}

/// A creative inventory tab the mod adds (`data/<id>/horizon/creative_tab/`):
/// its title, the item on its button, and its items in order. Items no mod
/// or vanilla registers are left out when the game loads.
#[derive(Clone, Debug, PartialEq)]
pub struct CreativeTab {
    /// `<mod id>:<path>`.
    pub id: String,
    pub title: String,
    pub icon: String,
    pub items: Vec<String>,
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
    #[serde(default)]
    components: BTreeMap<String, serde_json::Value>,
    crafting_remainder: Option<String>,
    #[serde(default)]
    properties: ItemProperties,
    armor: Option<Armor>,
    tool: Option<Tool>,
}

/// Typed component overrides; omitted values retain the template's components.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ItemProperties {
    pub max_damage: Option<u32>,
    pub max_stack_size: Option<u8>,
    pub rarity: Option<Rarity>,
    /// Zero removes enchantability.
    pub enchantability: Option<u32>,
    /// An item id, or an item tag prefixed with `#`.
    pub repair_item: Option<String>,
    pub fire_resistant: Option<bool>,
    pub attack_damage: Option<f64>,
    pub attack_speed: Option<f64>,
}

/// Armor components applied after copying the vanilla template.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Armor {
    pub slot: String,
    pub defense: f64,
    pub toughness: f64,
    #[serde(default)]
    pub knockback_resistance: f64,
    pub durability: u32,
    /// Equipment layer id, resolved under `textures/entity/equipment/`.
    pub texture: Option<String>,
}

/// Tool components applied after copying the vanilla template.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub kind: String,
    pub attack_damage: f64,
    pub attack_speed: f64,
    pub mining_speed: f64,
    pub durability: u32,
    /// Block tag defining blocks this tier cannot harvest; `#` is optional.
    pub incorrect_blocks_for_drops: String,
    pub enchantability: u32,
    pub repair_item: String,
}

/// An item the mod adds: a vanilla template item's components under a new
/// id, with these changed. A native mod may build one in code
/// (`native_items`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Item {
    pub id: String,
    /// The vanilla item it copies, `minecraft:<path>`.
    pub template: String,
    pub max_stack_size: Option<u8>,
    pub rarity: Option<Rarity>,
    /// A 26.3 data component patch over the template's: `!minecraft:<type>` removes one.
    pub components: BTreeMap<String, serde_json::Value>,
    /// What it leaves in the crafting grid (`Item.Properties.craftRemainder`):
    /// a vanilla item or one of the mod's own.
    pub crafting_remainder: Option<String>,
    pub properties: ItemProperties,
    pub armor: Option<Armor>,
    pub tool: Option<Tool>,
}

fn resource_id(id: &str) -> bool {
    id.len() <= 32767
        && id.split_once(':').is_some_and(|(ns, path)| {
            !ns.is_empty()
                && ns
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && check_path(&format!("assets/{ns}/{path}")).is_ok()
        })
}

fn item_or_tag(id: &str) -> bool {
    resource_id(id.strip_prefix('#').unwrap_or(id))
}

fn check_item_properties(file: &ItemFile, namespaces: &[String]) -> Result<(), String> {
    if !vanilla_id(&file.template) {
        return Err(format!(
            "template {:?} is not a vanilla item (minecraft:...)",
            file.template
        ));
    }
    let properties = &file.properties;
    if file
        .max_stack_size
        .zip(properties.max_stack_size)
        .is_some_and(|(a, b)| a != b)
        || file
            .rarity
            .zip(properties.rarity)
            .is_some_and(|(a, b)| a != b)
    {
        return Err("legacy max_stack_size/rarity contradict properties".into());
    }
    let stack_size = properties.max_stack_size.or(file.max_stack_size);
    if stack_size.is_some_and(|n| !(1..=99).contains(&n)) {
        return Err("max_stack_size is 1 to 99".into());
    }
    if file.armor.is_some() && file.tool.is_some() {
        return Err("armor and tool are mutually exclusive".into());
    }
    if stack_size.is_some_and(|n| n > 1)
        && (properties.max_damage.is_some() || file.armor.is_some() || file.tool.is_some())
    {
        return Err("items with durability require max_stack_size 1".into());
    }
    let durability = |n: u32| {
        if n == 0 || n > i32::MAX as u32 {
            Err("durability/max_damage is 1 to 2147483647".to_owned())
        } else {
            Ok(())
        }
    };
    let enchantability = |n: u32| {
        if n > i32::MAX as u32 {
            Err("enchantability is 0 to 2147483647".to_owned())
        } else {
            Ok(())
        }
    };
    let damage = |n: f64| n.is_finite() && (0.0..=2048.0).contains(&n);
    let speed = |n: f64| n.is_finite() && n > -4.0 && n <= 1024.0;
    if let Some(n) = properties.max_damage {
        durability(n)?;
    }
    if let Some(n) = properties.enchantability {
        enchantability(n)?;
    }
    if properties
        .repair_item
        .as_deref()
        .is_some_and(|id| !item_or_tag(id))
    {
        return Err("repair_item is a resource id or #item_tag".into());
    }
    if properties.attack_damage.is_some_and(|n| !damage(n)) {
        return Err("attack_damage is finite and 0 to 2048".into());
    }
    if properties.attack_speed.is_some_and(|n| !speed(n)) {
        return Err("attack_speed is finite, more than -4 and at most 1024".into());
    }
    if let Some(armor) = &file.armor {
        if !["head", "chest", "legs", "feet"].contains(&armor.slot.as_str()) {
            return Err("armor slot is head, chest, legs or feet".into());
        }
        for (name, n) in [("defense", armor.defense), ("toughness", armor.toughness)] {
            if !n.is_finite() || n < 0.0 {
                return Err(format!("armor {name} is finite and nonnegative"));
            }
        }
        if !armor.knockback_resistance.is_finite()
            || !(0.0..=1.0).contains(&armor.knockback_resistance)
        {
            return Err("armor knockback_resistance is 0 to 1".into());
        }
        durability(armor.durability)?;
        if let Some(texture) = &armor.texture {
            let owned = texture.split_once(':').is_some_and(|(ns, _)| {
                ns == "minecraft" || namespaces.iter().any(|owned| owned == ns)
            });
            if !resource_id(texture) || !owned {
                return Err(
                    "armor texture is a resource id in minecraft or the mod's namespaces".into(),
                );
            }
        }
    }
    if let Some(tool) = &file.tool {
        if !["sword", "pickaxe", "axe", "shovel", "hoe"].contains(&tool.kind.as_str()) {
            return Err("tool kind is sword, pickaxe, axe, shovel or hoe".into());
        }
        if !damage(tool.attack_damage) {
            return Err("tool attack_damage is finite and 0 to 2048".into());
        }
        if !speed(tool.attack_speed) {
            return Err("tool attack_speed is finite, more than -4 and at most 1024".into());
        }
        if !tool.mining_speed.is_finite() || tool.mining_speed < 0.0 {
            return Err("tool mining_speed is finite and nonnegative".into());
        }
        durability(tool.durability)?;
        enchantability(tool.enchantability)?;
        if !item_or_tag(&tool.incorrect_blocks_for_drops) {
            return Err("tool incorrect_blocks_for_drops is a block tag resource id".into());
        }
        if !item_or_tag(&tool.repair_item) {
            return Err("tool repair_item is a resource id or #item_tag".into());
        }
    }
    Ok(())
}

/// The attributes an entity may set, vanilla's names without `minecraft:`.
pub const ENTITY_ATTRIBUTES: [&str; 13] = [
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
    "safe_fall_distance",
    "fall_damage_multiplier",
];
/// Of [`ENTITY_ATTRIBUTES`], those only a type sets: the `attribute` order refuses them.
pub const TYPE_ONLY_ATTRIBUTES: [&str; 3] =
    ["scale", "safe_fall_distance", "fall_damage_multiplier"];
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
    persistent: Option<bool>,
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
    /// Never despawns (`Mob.requiresCustomPersistence`), as tame or boss-like mods' mobs.
    pub persistent: Option<bool>,
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

/// `MobEffectCategory`.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EffectCategory {
    Beneficial,
    Harmful,
    Neutral,
}

/// A mob effect the mod adds: what `new MobEffect(category, color)` takes.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MobEffect {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    pub category: EffectCategory,
    /// RGB, `0x000000` to `0xFFFFFF`.
    pub color: u32,
}

/// the vanilla entity types a mod's type may copy: thrown item
/// projectiles (`ThrowableItemProjectile`), then living mobs
/// ([`LIVING_TEMPLATES`]).
pub const ENTITY_TEMPLATES: [&str; 4] = [
    "minecraft:snowball",
    "minecraft:villager",
    "minecraft:wandering_trader",
    "minecraft:zombie",
];

/// the [`ENTITY_TEMPLATES`] that are living mobs: a type copying one is a mob
/// of its template's class (NeoForge's `EntityType.Builder.of(Zombie::new,
/// ...)`), with its own default attributes (`EntityAttributeCreationEvent`).
/// The wandering trader is the neutral one whose behaviour is all goals,
/// which a native mod can replace (`register_goals`); a villager's is its
/// brain.
pub const LIVING_TEMPLATES: [&str; 3] = [
    "minecraft:villager",
    "minecraft:wandering_trader",
    "minecraft:zombie",
];

/// an entity type the mod adds: a copy of a vanilla template's. A thrown one
/// is thrown from `item` and drawn as it; a living one is its template's mob
/// with other `attributes`.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EntityType {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    /// one of [`ENTITY_TEMPLATES`].
    pub template: String,
    /// a thrown type's: one of the mod's own items, which throws it. A
    /// living type has none.
    #[serde(default)]
    pub item: Option<String>,
    /// a thrown type's: what a hit deals to an entity, as `thrown` damage.
    #[serde(default)]
    pub damage: f32,
    /// a living type's default attribute values that differ from its
    /// template's, by attribute id (`minecraft:max_health`): the template's
    /// own attributes only, checked against its row in `entity-types.json`
    /// when the content root builds.
    #[serde(default)]
    pub attributes: BTreeMap<String, f64>,
}

impl EntityType {
    /// whether the type copies a living mob ([`LIVING_TEMPLATES`]).
    pub fn is_living(&self) -> bool {
        LIVING_TEMPLATES.contains(&self.template.as_str())
    }
}

/// a menu type the mod adds (vanilla's `MenuType`, NeoForge's
/// `IMenuTypeExtension.create`): its container's slot count, the
/// `ContainerData` count its menu syncs, and its take-only slots
/// (`FurnaceResultSlot.mayPlace`). The slots come first in the menu, then the
/// player's 36, and a mod screen (`native_screen`) places them.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MenuType {
    /// `<mod id>:<path>`.
    #[serde(skip)]
    pub id: String,
    /// 1 to [`MAX_MENU_SLOTS`].
    pub slots: u16,
    /// 0 to [`MAX_MENU_SLOTS`].
    #[serde(default)]
    pub data_slots: u16,
    /// slots a player takes from but never puts into, each below `slots`.
    #[serde(default)]
    pub results: Vec<u16>,
}

/// the most container or data slots a mod menu type has, as many as a mod
/// container (`native_block_entity_container`) may.
pub const MAX_MENU_SLOTS: u16 = 256;

#[derive(Debug, Clone)]
enum Source {
    Dir(PathBuf),
    Zip(PathBuf),
    /// No files: blocks and items a native mod declares in code.
    Code,
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
    pub effects: Vec<MobEffect>,
    /// Its creative inventory tabs, by id.
    pub creative_tabs: Vec<CreativeTab>,
    pub entity_types: Vec<EntityType>,
    pub menu_types: Vec<MenuType>,
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
        Self::read_declaring(path, manifest, &[])
    }
    /// [`Content::read`] for a mod whose native code also declares items (`native_items`):
    /// a file's `crafting_remainder` may name one of those `declared` ids, which are not in the
    /// files.
    pub fn read_declaring(
        path: &Path,
        manifest: &Manifest,
        declared: &[String],
    ) -> Result<Content, String> {
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
                let name = entry.name().map_err(|e| e.to_string())?.into_owned();
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
            effects: Vec::new(),
            creative_tabs: Vec::new(),
            entity_types: Vec::new(),
            menu_types: Vec::new(),
            namespaces: manifest.owned_namespaces().map(str::to_owned).collect(),
        };
        content.check(manifest, declared)?;
        Ok(content)
    }

    /// Content with no files, for the blocks and items native mods declare
    /// in code in `namespace` ([`Content::declare`]).
    pub fn declared(namespace: &str) -> Content {
        Content {
            source: Source::Code,
            entities: Vec::new(),
            files: Vec::new(),
            blocks: Vec::new(),
            items: Vec::new(),
            effects: Vec::new(),
            creative_tabs: Vec::new(),
            entity_types: Vec::new(),
            menu_types: Vec::new(),
            namespaces: vec![namespace.to_owned()],
        }
    }

    /// Adds blocks and items a native mod declares in code (`native_blocks`,
    /// `native_items`), checked as declaration files are: ids in this mod's
    /// own namespace (its id, as `data/<id>/horizon/` is) and declared once
    /// with what it already has, a vanilla template, a crafting remainder
    /// that is vanilla or this mod's. All or nothing: on an error nothing is
    /// added.
    pub fn declare(&mut self, blocks: &[Block], items: &[Item]) -> Result<(), String> {
        let own = self
            .namespaces
            .first()
            .map(String::as_str)
            .unwrap_or_default();
        let mut ids: BTreeSet<&str> = self
            .blocks
            .iter()
            .map(|b| b.id.as_str())
            .chain(self.items.iter().map(|i| i.id.as_str()))
            .collect();
        let mut item_ids: BTreeSet<&str> = self
            .blocks
            .iter()
            .filter(|b| b.item)
            .map(|b| b.id.as_str())
            .chain(self.items.iter().map(|i| i.id.as_str()))
            .collect();
        let in_own =
            |id: &str| namespaced_id(id) && id.split_once(':').is_some_and(|(ns, _)| ns == own);
        for block in blocks {
            let id = block.id.as_str();
            if !in_own(id) {
                return Err(format!("block {id:?} is not an id in {own}:"));
            }
            check_block(block).map_err(|e| format!("block {id}: {e}"))?;
            if !ids.insert(id) {
                return Err(format!("block {id} is declared twice"));
            }
            if block.item {
                item_ids.insert(id);
            }
        }
        let all: Vec<&Block> = self.blocks.iter().chain(blocks).collect();
        check_sign_walls(&all)?;
        for item in items {
            let id = item.id.as_str();
            if !in_own(id) {
                return Err(format!("item {id:?} is not an id in {own}:"));
            }
            check_item(item).map_err(|e| format!("item {id}: {e}"))?;
            if !item_ids.insert(id) {
                return Err(format!(
                    "item {id} is declared twice, or is also a block's item"
                ));
            }
        }
        for item in items {
            if let Some(rest) = &item.crafting_remainder
                && (rest == "minecraft:air"
                    || !vanilla_id(rest) && !item_ids.contains(rest.as_str()))
            {
                return Err(format!(
                    "item {}: crafting_remainder {rest:?} is not a vanilla item (minecraft:...) or one of this mod's",
                    item.id
                ));
            }
        }
        self.blocks.extend_from_slice(blocks);
        self.items.extend_from_slice(items);
        Ok(())
    }

    /// Where the files are read from: the mod's folder or its `.hmod`.
    pub fn source(&self) -> &Path {
        match &self.source {
            Source::Dir(path) | Source::Zip(path) => path,
            Source::Code => Path::new(""),
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
        reader_of(&self.source)
    }

    /// Where content may go, and the declarations.
    fn check(&mut self, manifest: &Manifest, declared: &[String]) -> Result<(), String> {
        let id = manifest.id.as_str();
        if !self.files.is_empty() && !manifest.capabilities.contains(&Capability::Registries) {
            return Err(format!(
                "has assets/ or data/ but not capability registries in {MANIFEST}"
            ));
        }
        let mut blocks = Vec::new();
        let mut items = Vec::new();
        let mut entities = Vec::new();
        let mut effects = Vec::new();
        let mut tabs = Vec::new();
        let mut types = Vec::new();
        let mut menus = Vec::new();
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
            // tags and atlases, and to the convention tags mods share.
            let shared = matches!(namespace, "minecraft" | "c");
            let atlas = top == "assets" && namespace == "minecraft" && rest.starts_with("atlases/");
            let allowed = owned || (top == "data" && shared && rest.starts_with("tags/")) || atlas;
            if !allowed {
                return Err(format!(
                    "{name}: a mod adds files in its own namespaces ({top}/{id}/ and those listed in namespaces), to vanilla and convention (c) tags, or to vanilla's atlases"
                ));
            }
            if !own && top == "data" && rest.starts_with("horizon/") {
                return Err(format!(
                    "{name}: blocks, items, entities, mob effects, entity types and menu types are declared in data/{id}/horizon/"
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
            } else if let Some(effect) = parse("mob_effect") {
                effects.push((effect, name.clone()));
            } else if let Some(tab) = parse("creative_tab") {
                tabs.push((tab, name.clone()));
            } else if let Some(kind) = parse("entity_type") {
                types.push((kind, name.clone()));
            } else if let Some(menu) = parse("menu") {
                menus.push((menu, name.clone()));
            } else {
                return Err(format!(
                    "{name}: data/{id}/horizon/ holds block/, item/, entity/, mob_effect/, creative_tab/, entity_type/ and menu/*.json only"
                ));
            }
        }
        // One reader for every declaration: opening a packaged mod reads its
        // whole index, so a reader per file made loading a big mod quadratic.
        let source = self.source.clone();
        let mut reader = reader_of(&source)?;
        let mut ids = BTreeSet::new();
        // Equipment assets remain resource files, including unknown vanilla fields.
        // Validate the optional geometry reference before copying them to the content root.
        for name in &self.files {
            let mut parts = name.splitn(3, '/');
            if parts.next() == Some("assets")
                && parts.next().is_some()
                && parts
                    .next()
                    .is_some_and(|p| p.starts_with("equipment/") && p.ends_with(".json"))
            {
                let file: serde_json::Value = serde_json::from_slice(&self.file(name)?)
                    .map_err(|e| format!("{name}: {e}"))?;
                for kind in ["humanoid", "humanoid_leggings"] {
                    for layer in file["layers"][kind].as_array().into_iter().flatten() {
                        let Some(model) = layer.get("model") else {
                            continue;
                        };
                        let model = model
                            .as_str()
                            .ok_or_else(|| format!("{name}: equipment model must be a string"))?;
                        let (ns, path) = model.split_once(':').unwrap_or_default();
                        if !resource_id(model)
                            || !self.namespaces.iter().any(|n| n == ns)
                            || !path.starts_with("armor/")
                            || path == "armor/"
                        {
                            return Err(format!(
                                "{name}: equipment model must be <mod namespace>:armor/<name>"
                            ));
                        }
                        let geo = format!("assets/{ns}/geo/{path}.geo.json");
                        if self.files.binary_search(&geo).is_err() {
                            return Err(format!("{name}: equipment model has no {geo}"));
                        }
                    }
                }
            }
        }
        for (block, name) in blocks {
            let file: BlockFile =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if file
                .connects_to
                .as_ref()
                .is_some_and(|t| !t.starts_with('#'))
            {
                return Err(format!(
                    "{name}: connects_to is a block tag, #namespace:path"
                ));
            }
            let block = Block {
                id: block,
                template: file.template,
                properties: file.properties,
                shapes: file.shapes,
                collision_shapes: file.collision_shapes,
                state_properties: file.state_properties,
                default_state: file.default_state,
                item: file.item,
                components: file.components,
                connects_to: file
                    .connects_to
                    .map(|t| t.trim_start_matches('#').to_owned()),
                thickness: file.thickness,
                wood_type: file.wood_type,
                wall_block: file.wall_block,
            };
            check_block(&block).map_err(|e| format!("{name}: {e}"))?;
            if block.item {
                ids.insert(block.id.clone());
            }
            self.blocks.push(block);
        }
        // After every block, so a wall form may be declared in a later file.
        check_sign_walls(&self.blocks.iter().collect::<Vec<_>>())?;
        let mut remainders = Vec::new();
        for (entity, name) in entities {
            let file: EntityFile =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
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
                persistent: file.persistent,
                category: file.category,
                texture: file.texture,
                model: file.model,
                animations: file.animations,
            });
        }
        for (item, name) in items {
            let file: ItemFile =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            check_item_properties(&file, &self.namespaces).map_err(|e| format!("{name}: {e}"))?;
            let item = Item {
                id: item,
                template: file.template,
                max_stack_size: file.properties.max_stack_size.or(file.max_stack_size),
                rarity: file.properties.rarity.or(file.rarity),
                properties: file.properties,
                armor: file.armor,
                tool: file.tool,
                components: file.components,
                crafting_remainder: file.crafting_remainder,
            };
            check_item(&item).map_err(|e| format!("{name}: {e}"))?;
            if !ids.insert(item.id.clone()) {
                return Err(format!("{name}: {} is also a block's item", item.id));
            }
            if let Some(rest) = &item.crafting_remainder {
                remainders.push((name, rest.clone()));
            }
            self.items.push(item);
        }
        for (effect, name) in effects {
            let file: MobEffect =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if file.color > 0xFF_FFFF {
                return Err(format!("{name}: color is RGB, 0 to 16777215 (0xFFFFFF)"));
            }
            self.effects.push(MobEffect { id: effect, ..file });
        }
        // After every item, so a remainder may be declared in a later file.
        for (name, rest) in remainders {
            if rest == "minecraft:air" {
                return Err(format!(
                    "{name}: crafting_remainder {rest:?} is no remainder (leave it out)"
                ));
            }
            if !vanilla_id(&rest) && !ids.contains(&rest) && !declared.contains(&rest) {
                return Err(format!(
                    "{name}: crafting_remainder {rest:?} is not a vanilla item (minecraft:...) or one of this mod's"
                ));
            }
        }
        for (tab, name) in tabs {
            let file: CreativeTabFile =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            let id_like = |s: &str| {
                s.split_once(':')
                    .is_some_and(|(n, p)| !n.is_empty() && !p.is_empty())
            };
            if !id_like(&file.icon) || !file.items.iter().all(|i| id_like(i)) {
                return Err(format!(
                    "{name}: icon and items are item ids (namespace:path)"
                ));
            }
            self.creative_tabs.push(CreativeTab {
                id: tab,
                title: file.title,
                icon: file.icon,
                items: file.items,
            });
        }
        for (kind, name) in types {
            let file: EntityType =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if !ENTITY_TEMPLATES.contains(&file.template.as_str()) {
                return Err(format!(
                    "{name}: template {:?} is not one of {ENTITY_TEMPLATES:?}",
                    file.template
                ));
            }
            if file.is_living() {
                // a mob is spawned, not thrown: nothing throws it, and its
                // attack is its `minecraft:attack_damage`.
                if file.item.is_some() || file.damage != 0.0 {
                    return Err(format!(
                        "{name}: a living type has no item or damage (its template is {})",
                        file.template
                    ));
                }
                for (attribute, value) in &file.attributes {
                    let id_like = attribute
                        .split_once(':')
                        .is_some_and(|(n, p)| !n.is_empty() && !p.is_empty());
                    if !id_like {
                        return Err(format!(
                            "{name}: attribute {attribute:?} is an attribute id (namespace:path)"
                        ));
                    }
                    if !value.is_finite() {
                        return Err(format!("{name}: attribute {attribute} is a finite number"));
                    }
                }
                self.entity_types.push(EntityType { id: kind, ..file });
                continue;
            }
            if !file.attributes.is_empty() {
                return Err(format!(
                    "{name}: only a living type has attributes (its template is {})",
                    file.template
                ));
            }
            let Some(item) = &file.item else {
                return Err(format!(
                    "{name}: a thrown type names the item that throws it"
                ));
            };
            // its own item, so no vanilla item loses its use to it.
            if !ids.contains(item) {
                return Err(format!(
                    "{name}: item {item:?} is not one of this mod's items"
                ));
            }
            if self
                .entity_types
                .iter()
                .any(|t| t.item.as_ref() == Some(item))
            {
                return Err(format!(
                    "{name}: item {item:?} already throws another entity type"
                ));
            }
            if !(file.damage.is_finite() && file.damage >= 0.0) {
                return Err(format!("{name}: damage is 0 or more"));
            }
            self.entity_types.push(EntityType { id: kind, ..file });
        }
        for (menu, name) in menus {
            let file: MenuType =
                serde_json::from_slice(&reader.read(&name)?).map_err(|e| format!("{name}: {e}"))?;
            if !(1..=MAX_MENU_SLOTS).contains(&file.slots) || file.data_slots > MAX_MENU_SLOTS {
                return Err(format!(
                    "{name}: slots is 1 to {MAX_MENU_SLOTS}, data_slots 0 to {MAX_MENU_SLOTS}"
                ));
            }
            let mut seen = BTreeSet::new();
            for &r in &file.results {
                if r >= file.slots {
                    return Err(format!(
                        "{name}: result {r} is past the last slot, {}",
                        file.slots - 1
                    ));
                }
                if !seen.insert(r) {
                    return Err(format!("{name}: result {r} is listed twice"));
                }
            }
            self.menu_types.push(MenuType { id: menu, ..file });
        }
        Ok(())
    }
}

/// Namespaced component types, removals as `{}`, and none of `fields`,
/// which the file sets with its own keys.
fn check_components(
    components: &BTreeMap<String, serde_json::Value>,
    fields: &[&str],
) -> Result<(), String> {
    for (key, value) in components {
        let (removed, kind) = match key.strip_prefix('!') {
            Some(kind) => (true, kind),
            None => (false, key.as_str()),
        };
        if !namespaced_id(kind) {
            return Err(format!("component {key:?} must be namespaced and valid"));
        }
        if let Some(own) = kind
            .strip_prefix("minecraft:")
            .filter(|k| fields.contains(k))
        {
            return Err(format!("component {key:?}: set {own} instead"));
        }
        if removed && !value.as_object().is_some_and(|o| o.is_empty()) {
            return Err(format!("component {key:?}: a removal's value is {{}}"));
        }
    }
    Ok(())
}

/// An item's fields, as a file or code gives them.
fn check_item(item: &Item) -> Result<(), String> {
    if !vanilla_id(&item.template) {
        return Err(format!(
            "template {:?} is not a vanilla item (minecraft:...)",
            item.template
        ));
    }
    if item.max_stack_size.is_some_and(|n| !(1..=99).contains(&n)) {
        return Err("max_stack_size is 1 to 99".into());
    }
    check_components(&item.components, &["max_stack_size", "rarity"])
}

/// A block's fields, as a file or code gives them (`connects_to` without
/// its `#`).
fn check_block(file: &Block) -> Result<(), String> {
    if !vanilla_id(&file.template) {
        return Err(format!(
            "template {:?} is not a vanilla block (minecraft:...)",
            file.template
        ));
    }
    for shapes in file.shapes.iter().chain(file.collision_shapes.iter()) {
        shapes.check()?;
    }
    for (name, property) in &file.state_properties {
        if !state_name(name) {
            return Err(format!(
                "state property {name:?} is lower case letters, digits and underscores"
            ));
        }
        let values = property.values();
        if values.is_empty() {
            return Err(format!("state property {name:?} needs a value"));
        }
        let mut seen = BTreeSet::new();
        for value in values {
            if !state_name(value) {
                return Err(format!(
                    "state property {name:?}: value {value:?} is lower case letters, digits and underscores"
                ));
            }
            if !seen.insert(value) {
                return Err(format!("state property {name:?}: value {value:?} repeats"));
            }
        }
    }
    for key in file.default_state.keys() {
        if !state_name(key) {
            return Err(format!(
                "default_state key {key:?} is lower case letters, digits and underscores"
            ));
        }
    }
    let p = &file.properties;
    if let Some(RandomTicks::States { property, values }) = &p.random_ticks {
        let declared = file
            .state_properties
            .get(property)
            .map(StateProperty::values);
        if declared.is_none_or(|all| {
            values.is_empty() || values.iter().any(|v| !all.contains(&v.as_str()))
        }) {
            return Err(format!(
                "random_ticks needs values of the block's own state property {property:?} (state_properties)"
            ));
        }
    }
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
    if !file.item && !file.components.is_empty() {
        return Err("item is false, so the block has no item to take components".into());
    }
    check_components(&file.components, &[])?;
    // A block file has no `max_stack_size` / `rarity` keys, so those two take the same checks here.
    if let Some(n) = file.components.get("minecraft:max_stack_size")
        && !n.as_u64().is_some_and(|n| (1..=99).contains(&n))
    {
        return Err("component \"minecraft:max_stack_size\" is an integer 1 to 99".into());
    }
    if let Some(r) = file.components.get("minecraft:rarity")
        && serde_json::from_value::<Rarity>(r.clone()).is_err()
    {
        return Err("component \"minecraft:rarity\" is common, uncommon, rare or epic".into());
    }
    if let Some(tag) = &file.connects_to {
        if file.template != "minecraft:chorus_plant" {
            return Err("connects_to needs template minecraft:chorus_plant (a PipeBlock)".into());
        }
        let id = tag.as_str();
        let valid = |s: &str| {
            !s.is_empty()
                && s.chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-./".contains(c))
        };
        if !id
            .split_once(':')
            .is_some_and(|(ns, path)| valid(ns) && valid(path))
        {
            return Err("connects_to is a block tag, #namespace:path".into());
        }
    }
    match file.thickness {
        Some(_) if file.connects_to.is_none() => {
            return Err("thickness is a connecting block's (connects_to)".into());
        }
        Some(t) if !(2.0..=14.0).contains(&t) => return Err("thickness is 2 to 14 pixels".into()),
        Some(_) if file.shapes.is_some() => {
            return Err("thickness sets the shapes, so a block has one or the other".into());
        }
        _ => {}
    }
    // Every vanilla `SignBlock`'s id ends in `_sign`, and no other block's does.
    let sign = file.template.ends_with("_sign");
    if let Some(wood) = &file.wood_type {
        if !sign {
            return Err("wood_type needs a vanilla sign template".into());
        }
        let (namespace, path) = wood.split_once(':').unwrap_or(("minecraft", wood));
        if !namespaced_id(&format!("{namespace}:{path}"))
            || check_path(&format!("assets/{namespace}/{path}")).is_err()
        {
            return Err("wood_type is a name, or namespace:path, in lower case".into());
        }
    }
    if let Some(wall) = &file.wall_block {
        if !sign || file.template.contains("_wall_") {
            return Err("wall_block needs a standing or ceiling sign template".into());
        }
        if !file.item {
            return Err("wall_block is where the item places the block, and item is false".into());
        }
        if !namespaced_id(wall) {
            return Err("wall_block is a block id, namespace:path".into());
        }
    }
    Ok(())
}

/// A sign's `wall_block` is one of the mod's own blocks: the wall form of its
/// kind, with no item of its own and no other sign. Checked here, a pair that
/// doesn't fit leaves out its mod alone.
fn check_sign_walls(blocks: &[&Block]) -> Result<(), String> {
    let mut paired = BTreeMap::new();
    for sign in blocks {
        let Some(wall) = &sign.wall_block else {
            continue;
        };
        let id = &sign.id;
        let form = if sign.template.ends_with("_hanging_sign") {
            "_wall_hanging_sign"
        } else {
            "_wall_sign"
        };
        let Some(block) = blocks.iter().find(|b| b.id == *wall) else {
            return Err(format!(
                "{id}: wall_block {wall} is not one of this mod's blocks"
            ));
        };
        if !block.template.ends_with(form) {
            return Err(format!(
                "{id}: wall_block {wall} needs a template ending in {form}"
            ));
        }
        if block.item {
            return Err(format!(
                "{id}: wall_block {wall} has this sign's item, so its own \"item\" is false"
            ));
        }
        if let Some(other) = paired.insert(wall, id) {
            return Err(format!(
                "{id}: wall_block {wall} is already paired with {other}"
            ));
        }
    }
    Ok(())
}

fn reader_of(source: &Source) -> Result<FileReader<'_>, String> {
    Ok(match source {
        Source::Dir(root) => FileReader::Dir(root),
        Source::Zip(path) => FileReader::Zip(Box::new(open_zip(path)?)),
        Source::Code => FileReader::Dir(Path::new("")),
    })
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
    #[test]
    fn shape_tables_validate_tuples_defaults_and_collision() {
        let content = hmod("shape-tuples", MANIFEST_TEXT, &[(
            "data/ruby/horizon/block/a.json",
            br#"{"template":"minecraft:stone","shapes":{"properties":["facing","count"],"values":{"north,2":[[0,0,0,1,0.5,1]]},"default":[[0,0,0,1,1,1]]},"collision_shapes":{"default":[]}}"#,
        )]).unwrap();
        let block = &content.blocks[0];
        assert_eq!(
            block.shapes.as_ref().unwrap().property_names(),
            ["facing", "count"]
        );
        assert_eq!(
            block.collision_shapes.as_ref().unwrap().default,
            Some(vec![])
        );
        for value in [
            r#"{"property":"axis","properties":["count"],"default":[]}"#,
            r#"{"properties":["axis","axis"],"default":[]}"#,
            r#"{"properties":["Axis"],"default":[]}"#,
            r#"{"properties":["axis","count"],"values":{"x":[]}}"#,
            r#"{"properties":["axis"],"values":{"":[]}}"#,
            r#"{"values":{"x":[]},"default":[]}"#,
            r#"{"default":[[1,0,0,0,1,1]]}"#,
        ] {
            let shapes: BlockShapes = serde_json::from_str(value).unwrap();
            assert!(shapes.check().is_err(), "{value}");
            let declaration =
                format!(r#"{{"template":"minecraft:stone","collision_shapes":{value}}}"#);
            assert!(
                hmod(
                    "bad-collision",
                    MANIFEST_TEXT,
                    &[("data/ruby/horizon/block/a.json", declaration.as_bytes(),)]
                )
                .is_err(),
                "{value}"
            );
        }
        let oversized = BlockShapes {
            property: String::new(),
            properties: vec![],
            values: BTreeMap::new(),
            default: Some(vec![[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]; 17]),
        };
        assert!(oversized.check().is_err());
        let mut nonfinite = oversized;
        nonfinite.default = Some(vec![[f64::NAN; 6]]);
        assert!(nonfinite.check().is_err());
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
    fn equipment_models_validate_all_layers_paths_namespaces_and_presence() {
        let geo = "assets/ruby/geo/armor/horn.geo.json";
        let equipment = "assets/ruby/equipment/metal.json";
        let document = |model: serde_json::Value| {
            serde_json::json!({"layers":{
                "humanoid":[{"texture":"ruby:metal"},{"texture":"ruby:metal","model":model}],
                "humanoid_leggings":[{"texture":"ruby:metal","model":"ruby:armor/horn"}]
            }})
            .to_string()
        };
        let valid = document(serde_json::json!("ruby:armor/horn"));
        let content = hmod(
            "equipment-model",
            MANIFEST_TEXT,
            &[(equipment, valid.as_bytes()), (geo, b"{}")],
        )
        .unwrap();
        assert!(content.files.contains(&geo.to_owned()));
        assert!(
            hmod(
                "missing-equipment-model",
                MANIFEST_TEXT,
                &[(equipment, valid.as_bytes())]
            )
            .unwrap_err()
            .contains("equipment model has no")
        );
        for bad in [
            serde_json::json!("minecraft:armor/horn"),
            serde_json::json!("other:armor/horn"),
            serde_json::json!("ruby:horn"),
            serde_json::json!("ruby:armor/../horn"),
            serde_json::json!("ruby:armor/"),
            serde_json::json!("ruby:armor//horn"),
            serde_json::json!("ruby:armor/Bad"),
            serde_json::json!(3),
            serde_json::Value::Null,
        ] {
            let file = document(bad.clone());
            assert!(
                hmod(
                    "invalid-equipment-model",
                    MANIFEST_TEXT,
                    &[(equipment, file.as_bytes()), (geo, b"{}")]
                )
                .unwrap_err()
                .contains("equipment model"),
                "{bad}"
            );
        }
        let old = br#"{"layers":{"humanoid":[{"texture":"ruby:metal"}]}}"#;
        hmod("old-equipment", MANIFEST_TEXT, &[(equipment, old)]).unwrap();
    }

    #[test]
    fn blocks_and_items_declared_in_code_pass_the_files_checks() {
        let block = |id: &str| Block {
            id: id.into(),
            template: "minecraft:iron_block".into(),
            item: true,
            ..Block::default()
        };
        let item = |id: &str| Item {
            id: id.into(),
            template: "minecraft:diamond".into(),
            ..Item::default()
        };
        let mut content = Content::declared("ruby");
        assert!(content.is_empty());
        let wrench = Item {
            crafting_remainder: Some("ruby:gear".into()),
            ..item("ruby:wrench")
        };
        content
            .declare(&[block("ruby:gear"), block("ruby:casing")], &[wrench])
            .unwrap();
        assert_eq!(content.blocks.len(), 2);
        assert_eq!(content.items[0].id, "ruby:wrench");
        // Each refusal adds nothing.
        let refused = |blocks: &[Block], items: &[Item]| {
            let mut c = Content::declared("ruby");
            c.declare(&[block("ruby:gear")], &[]).unwrap();
            let e = c.declare(blocks, items).unwrap_err();
            assert_eq!((c.blocks.len(), c.items.len()), (1, 0), "{e}");
            e
        };
        assert!(refused(&[block("emerald:gear")], &[]).contains("not an id in ruby:"));
        assert!(refused(&[block("minecraft:gear")], &[]).contains("not an id in ruby:"));
        assert!(refused(&[], &[item("ruby")]).contains("not an id in ruby:"));
        assert!(refused(&[block("ruby:gear")], &[]).contains("declared twice"));
        assert!(refused(&[], &[item("ruby:gear")]).contains("block's item"));
        let modded = Block {
            template: "ruby:other".into(),
            ..block("ruby:casing")
        };
        assert!(refused(&[modded], &[]).contains("not a vanilla block"));
        let dim = Block {
            properties: BlockProperties {
                light: Some(16),
                ..BlockProperties::default()
            },
            ..block("ruby:lamp")
        };
        assert!(refused(&[dim], &[]).contains("light is 0 to 15"));
        let stack = Item {
            max_stack_size: Some(100),
            ..item("ruby:ingot")
        };
        assert!(refused(&[], &[stack]).contains("max_stack_size"));
        let rest = Item {
            crafting_remainder: Some("emerald:dust".into()),
            ..item("ruby:dust")
        };
        assert!(refused(&[block("ruby:pipe")], &[rest]).contains("crafting_remainder"));
        // A cable's tag comes bare from code.
        let cable = Block {
            template: "minecraft:chorus_plant".into(),
            connects_to: Some("ruby:cables".into()),
            ..block("ruby:cable")
        };
        content.declare(&[cable], &[]).unwrap();
    }

    #[test]
    fn a_connecting_block_file_names_its_tag_with_a_hash() {
        let cable = br#"{"template": "minecraft:chorus_plant", "connects_to": "ruby:cables"}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/cable.json", cable)];
        let e = hmod("bare-tag", MANIFEST_TEXT, files).unwrap_err();
        assert!(e.contains("#namespace:path"), "{e}");
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
            ("assets/minecraft/atlases/blocks.json", b"{}"),
        ];
        let content = hmod("ok", MANIFEST_TEXT, files).unwrap();
        assert_eq!(content.files.len(), 7);
        assert_eq!(
            content.blocks,
            [Block {
                id: "ruby:ruby_block".into(),
                template: "minecraft:iron_block".into(),
                shapes: None,
                collision_shapes: None,
                state_properties: BTreeMap::new(),
                default_state: BTreeMap::new(),
                properties: BlockProperties {
                    hardness: Some(4.0),
                    light: Some(3),
                    sounds: Some("minecraft:amethyst_block".into()),
                    ..BlockProperties::default()
                },
                item: true,
                components: BTreeMap::new(),
                connects_to: None,
                thickness: None,
                wood_type: None,
                wall_block: None,
            }]
        );
        assert_eq!(content.items[0].id, "ruby:ruby");
        assert_eq!(content.items[0].rarity, Some(Rarity::Uncommon));
        assert!(content.items[0].components.is_empty());
        assert_eq!(content.items[0].crafting_remainder, None);
    }

    #[test]
    fn items_name_their_crafting_remainder() {
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/item/a_stew.json",
                br#"{"template": "minecraft:mushroom_stew", "crafting_remainder": "minecraft:bowl"}"#,
            ),
            (
                "data/ruby/horizon/item/b_flask.json",
                br#"{"template": "minecraft:potion", "crafting_remainder": "ruby:c_flask"}"#,
            ),
            ("data/ruby/horizon/item/c_flask.json", ITEM),
        ];
        let content = hmod("remainder", MANIFEST_TEXT, files).unwrap();
        let rests: Vec<_> = content
            .items
            .iter()
            .map(|i| i.crafting_remainder.as_deref())
            .collect();
        assert_eq!(rests, [Some("minecraft:bowl"), Some("ruby:c_flask"), None]);
    }

    #[test]
    fn a_file_item_may_leave_a_code_declared_item_as_its_remainder() {
        // `native_items` declares ruby:wrench in code; a file item names it as its remainder.
        let files: &[(&str, &[u8])] = &[(
            "data/ruby/horizon/item/tool.json",
            br#"{"template": "minecraft:iron_pickaxe", "crafting_remainder": "ruby:wrench"}"#,
        )];
        let read = |declared: &[String]| {
            let dir = scratch("declared-remainder");
            let path = dir.join("m.hmod");
            std::fs::write(
                &path,
                Package::write_files(MANIFEST_TEXT, None, files).unwrap(),
            )
            .unwrap();
            let package = Package::read(&path).unwrap();
            let content = Content::read_declaring(&path, &package.manifest, declared);
            std::fs::remove_dir_all(&dir).unwrap();
            content
        };
        assert!(read(&[]).is_err(), "a remainder nobody declares is refused");
        let content = read(&["ruby:wrench".to_owned()]).expect("the code-declared item is known");
        assert_eq!(
            content.items[0].crafting_remainder.as_deref(),
            Some("ruby:wrench")
        );
    }

    #[test]
    fn items_patch_their_template_components() {
        let sword = br#"{"template": "minecraft:iron_sword", "components": {
            "minecraft:max_damage": 900,
            "minecraft:attribute_modifiers": [{"type": "minecraft:attack_damage",
                "id": "minecraft:base_attack_damage", "amount": 7.0,
                "operation": "add_value", "slot": "mainhand"}],
            "!minecraft:repairable": {}}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/item/ruby_sword.json", sword)];
        let content = hmod("components", MANIFEST_TEXT, files).unwrap();
        let components = &content.items[0].components;
        assert_eq!(
            components.keys().collect::<Vec<_>>(),
            [
                "!minecraft:repairable",
                "minecraft:attribute_modifiers",
                "minecraft:max_damage"
            ]
        );
        assert_eq!(components["minecraft:max_damage"], 900);
    }

    #[test]
    fn mob_effects_take_a_category_and_a_colour() {
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/mob_effect/glow.json",
                br#"{"category": "beneficial", "color": 16711680}"#,
            ),
            (
                "data/ruby/horizon/mob_effect/curse/dark.json",
                br#"{"category": "harmful", "color": 0}"#,
            ),
        ];
        let content = hmod("effects", MANIFEST_TEXT, files).unwrap();
        let effects: Vec<_> = content
            .effects
            .iter()
            .map(|e| (e.id.as_str(), e.category, e.color))
            .collect();
        assert_eq!(
            effects,
            [
                ("ruby:curse/dark", EffectCategory::Harmful, 0),
                ("ruby:glow", EffectCategory::Beneficial, 0xFF0000),
            ]
        );
        for (case, file, why) in [
            (
                "effect-colour",
                r#"{"category": "neutral", "color": 16777216}"#,
                "color is RGB",
            ),
            (
                "effect-category",
                r#"{"category": "good", "color": 1}"#,
                "unknown variant",
            ),
            (
                "effect-key",
                r#"{"category": "neutral", "color": 1, "id": "x"}"#,
                "unknown field",
            ),
            (
                "effect-missing",
                r#"{"category": "neutral"}"#,
                "missing field",
            ),
        ] {
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/mob_effect/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn menu_types_take_slots_data_slots_and_results() {
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/menu/press.json",
                br#"{"slots": 9, "data_slots": 2, "results": [7, 8]}"#,
            ),
            ("data/ruby/horizon/menu/bag.json", br#"{"slots": 4}"#),
        ];
        let content = hmod("menus", MANIFEST_TEXT, files).unwrap();
        let menus: Vec<_> = content
            .menu_types
            .iter()
            .map(|m| (m.id.as_str(), m.slots, m.data_slots, m.results.clone()))
            .collect();
        assert_eq!(
            menus,
            [("ruby:bag", 4, 0, vec![]), ("ruby:press", 9, 2, vec![7, 8])]
        );
        for (case, file, why) in [
            ("menu-empty", r#"{"slots": 0}"#, "slots is 1 to 256"),
            ("menu-large", r#"{"slots": 257}"#, "slots is 1 to 256"),
            (
                "menu-data",
                r#"{"slots": 1, "data_slots": 257}"#,
                "data_slots 0 to 256",
            ),
            (
                "menu-result",
                r#"{"slots": 3, "results": [3]}"#,
                "result 3 is past the last slot, 2",
            ),
            (
                "menu-twice",
                r#"{"slots": 3, "results": [1, 1]}"#,
                "result 1 is listed twice",
            ),
            ("menu-key", r#"{"slots": 3, "rows": 1}"#, "unknown field"),
            ("menu-missing", r#"{"data_slots": 1}"#, "missing field"),
        ] {
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/menu/a.json", file.as_bytes())];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn entity_types_copy_a_throwable_and_name_its_item() {
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/item/gem.json", ITEM),
            (
                "data/ruby/horizon/entity_type/bolt.json",
                br#"{"template": "minecraft:snowball", "item": "ruby:gem", "damage": 2.5}"#,
            ),
            ("data/ruby/horizon/item/dust.json", ITEM),
            (
                "data/ruby/horizon/entity_type/puff.json",
                br#"{"template": "minecraft:snowball", "item": "ruby:dust"}"#,
            ),
        ];
        let content = hmod("types", MANIFEST_TEXT, files).unwrap();
        let types: Vec<_> = content
            .entity_types
            .iter()
            .map(|t| (t.id.as_str(), t.item.as_deref(), t.damage))
            .collect();
        assert_eq!(
            types,
            [
                ("ruby:bolt", Some("ruby:gem"), 2.5),
                ("ruby:puff", Some("ruby:dust"), 0.0)
            ]
        );
        let snowball = |rest: &str| format!(r#"{{"template": "minecraft:snowball"{rest}}}"#);
        for (case, file, why) in [
            (
                "type-template",
                r#"{"template": "minecraft:egg", "item": "ruby:gem"}"#.to_owned(),
                "is not one of",
            ),
            (
                "type-vanilla-item",
                snowball(r#", "item": "minecraft:stick""#),
                "is not one of this mod's items",
            ),
            (
                "type-unknown-item",
                snowball(r#", "item": "ruby:none""#),
                "is not one of this mod's items",
            ),
            (
                "type-damage",
                snowball(r#", "item": "ruby:gem", "damage": -1"#),
                "damage is 0 or more",
            ),
            (
                "type-key",
                snowball(r#", "item": "ruby:gem", "size": 1"#),
                "unknown field",
            ),
            (
                "type-no-item",
                snowball(""),
                "names the item that throws it",
            ),
            (
                "type-thrown-attributes",
                snowball(r#", "item": "ruby:gem", "attributes": {"minecraft:max_health": 4}"#),
                "only a living type has attributes",
            ),
        ] {
            let files: &[(&str, &[u8])] = &[
                ("data/ruby/horizon/item/gem.json", ITEM),
                ("data/ruby/horizon/entity_type/a.json", file.as_bytes()),
            ];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
        let shared = snowball(r#", "item": "ruby:gem""#);
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/item/gem.json", ITEM),
            ("data/ruby/horizon/entity_type/a.json", shared.as_bytes()),
            ("data/ruby/horizon/entity_type/b.json", shared.as_bytes()),
        ];
        let e = hmod("type-shared", MANIFEST_TEXT, files).unwrap_err();
        assert!(e.contains("already throws another entity type"), "{e}");
    }

    #[test]
    fn living_entity_types_copy_a_mob_with_their_own_attributes() {
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/entity_type/ghoul.json",
                br#"{"template": "minecraft:zombie",
                     "attributes": {"minecraft:max_health": 40, "minecraft:movement_speed": 0.3}}"#,
            ),
            (
                "data/ruby/horizon/entity_type/settler.json",
                br#"{"template": "minecraft:villager"}"#,
            ),
            (
                "data/ruby/horizon/entity_type/merchant.json",
                br#"{"template": "minecraft:wandering_trader"}"#,
            ),
        ];
        let content = hmod("living", MANIFEST_TEXT, files).unwrap();
        let types: Vec<_> = content
            .entity_types
            .iter()
            .map(|t| {
                let attributes: Vec<_> =
                    t.attributes.iter().map(|(a, v)| (a.as_str(), *v)).collect();
                (
                    t.id.as_str(),
                    t.template.as_str(),
                    t.is_living(),
                    attributes,
                )
            })
            .collect();
        assert_eq!(
            types,
            [
                (
                    "ruby:ghoul",
                    "minecraft:zombie",
                    true,
                    vec![
                        ("minecraft:max_health", 40.0),
                        ("minecraft:movement_speed", 0.3)
                    ]
                ),
                ("ruby:merchant", "minecraft:wandering_trader", true, vec![]),
                ("ruby:settler", "minecraft:villager", true, vec![]),
            ]
        );
        let zombie = |rest: &str| format!(r#"{{"template": "minecraft:zombie"{rest}}}"#);
        for (case, file, why) in [
            (
                "living-item",
                zombie(r#", "item": "ruby:gem""#),
                "a living type has no item or damage",
            ),
            (
                "living-damage",
                zombie(r#", "damage": 2"#),
                "a living type has no item or damage",
            ),
            (
                "living-attribute-id",
                zombie(r#", "attributes": {"max_health": 40}"#),
                "is an attribute id",
            ),
            (
                "living-attribute-value",
                zombie(r#", "attributes": {"minecraft:max_health": "40"}"#),
                "invalid type",
            ),
        ] {
            let files: &[(&str, &[u8])] = &[
                ("data/ruby/horizon/item/gem.json", ITEM),
                ("data/ruby/horizon/entity_type/a.json", file.as_bytes()),
            ];
            let e = hmod(case, MANIFEST_TEXT, files).unwrap_err();
            assert!(e.contains(why), "{case}: {e}");
        }
    }

    #[test]
    fn a_block_may_occlude_nothing() {
        let block = |occludes: &str| {
            let text = format!(
                r#"{{"template": "minecraft:barrel", "properties": {{"occludes": {occludes}}}}}"#
            );
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/block/panel.json", text.as_bytes())];
            hmod("occludes", MANIFEST_TEXT, files)
        };
        let occludes = |c: Result<Content, String>| c.unwrap().blocks[0].properties.occludes;
        assert_eq!(occludes(block("false")), Some(false));
        assert_eq!(occludes(block("true")), Some(true));
        assert!(block(r#""no""#).is_err());
    }

    #[test]
    fn blocks_choose_their_render_shape() {
        let block = |shape: &str| {
            let text = format!(
                r#"{{"template": "minecraft:stone", "properties": {{"render_shape": {shape}}}}}"#
            );
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/gear.json", text.as_bytes())];
            hmod("render-shape", MANIFEST_TEXT, files)
        };
        let shape = |c: Result<Content, String>| c.unwrap().blocks[0].properties.render_shape;
        assert_eq!(shape(block(r#""MODEL""#)), Some(RenderShape::Model));
        assert_eq!(
            shape(block(r#""ENTITYBLOCK_ANIMATED""#)),
            Some(RenderShape::EntityblockAnimated)
        );
        assert_eq!(shape(block(r#""INVISIBLE""#)), Some(RenderShape::Invisible));
        // Only vanilla's constants, as written there.
        for bad in [r#""animated""#, r#""model""#, "1"] {
            assert!(block(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn blocks_pick_the_states_that_tick_randomly() {
        let own = r#""state_properties": {"grown": {"type": "boolean"}}"#;
        let block = |ticks: &str| {
            let text = format!(
                r#"{{"template": "minecraft:dirt", {own}, "properties": {{"random_ticks": {ticks}}}}}"#
            );
            let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/crop.json", text.as_bytes())];
            hmod("random-ticks", MANIFEST_TEXT, files)
        };
        let ticks =
            |c: Result<Content, String>| c.unwrap().blocks[0].properties.random_ticks.clone();
        let state = |v: &str| [("grown".to_owned(), v.to_owned())];
        // Every state, whatever the template does; `false` stops a template that ticks.
        let all = ticks(block("true")).unwrap();
        assert!(all.applies(&state("true")) && all.applies(&[]));
        assert!(!ticks(block("false")).unwrap().applies(&state("false")));
        // Chosen states: a crop that stops ticking once it is grown.
        let chosen = ticks(block(r#"{"property": "grown", "values": ["false"]}"#)).unwrap();
        assert!(chosen.applies(&state("false")) && !chosen.applies(&state("true")));
        assert!(
            !chosen.applies(&[]),
            "a state without the property does not tick"
        );
        // Not the block's own property, no values, or a value it doesn't have.
        for bad in [
            r#"{"property": "age", "values": ["0"]}"#,
            r#"{"property": "grown", "values": []}"#,
            r#"{"property": "grown", "values": ["maybe"]}"#,
        ] {
            let e = block(bad).err().unwrap();
            assert!(e.contains("random_ticks"), "{bad}: {e}");
        }
        assert!(block("3").is_err());
    }

    #[test]
    fn block_items_patch_their_components() {
        let block = br#"{"template": "minecraft:iron_block", "components": {
            "minecraft:max_stack_size": 16, "!minecraft:lore": {}}}"#;
        let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/ruby_block.json", block)];
        let content = hmod("block-components", MANIFEST_TEXT, files).unwrap();
        let components = &content.blocks[0].components;
        assert_eq!(
            components.keys().collect::<Vec<_>>(),
            ["!minecraft:lore", "minecraft:max_stack_size"]
        );
        assert_eq!(components["minecraft:max_stack_size"], 16);
    }

    #[test]
    fn item_equipment_and_properties_round_trip() {
        let armor = br##"{"template":"minecraft:stick","properties":{
            "max_stack_size":1,"rarity":"epic","enchantability":25,
            "repair_item":"#ruby:ingots","fire_resistant":true},
            "armor":{"slot":"head","defense":6,"toughness":3,
            "knockback_resistance":0.1,"durability":800,"texture":"ruby:dragon"}}"##;
        let tool = br#"{"template":"minecraft:iron_pickaxe","tool":{
            "kind":"pickaxe","attack_damage":6,"attack_speed":-2.8,
            "mining_speed":9,"durability":1800,
            "incorrect_blocks_for_drops":"minecraft:incorrect_for_diamond_tool",
            "enchantability":0,"repair_item":"ruby:ingot"}}"#;
        let bow = br#"{"template":"minecraft:bow","properties":{"max_damage":500,
            "attack_damage":4,"attack_speed":-2}}"#;
        let files: &[(&str, &[u8])] = &[
            ("data/ruby/horizon/item/helmet.json", armor),
            ("data/ruby/horizon/item/pickaxe.json", tool),
            ("data/ruby/horizon/item/bow.json", bow),
        ];
        let content = hmod("item-equipment", MANIFEST_TEXT, files).unwrap();
        let bow = &content.items[0];
        assert_eq!(bow.template, "minecraft:bow");
        assert_eq!(bow.properties.max_damage, Some(500));
        assert_eq!(bow.properties.attack_damage, Some(4.0));
        let helmet = &content.items[1];
        assert_eq!(helmet.max_stack_size, Some(1));
        assert_eq!(helmet.rarity, Some(Rarity::Epic));
        assert_eq!(helmet.properties.fire_resistant, Some(true));
        assert_eq!(helmet.armor.as_ref().unwrap().slot, "head");
        assert_eq!(
            helmet.armor.as_ref().unwrap().texture.as_deref(),
            Some("ruby:dragon")
        );
        let tool = content.items[2].tool.as_ref().unwrap();
        assert_eq!(tool.kind, "pickaxe");
        assert_eq!(tool.attack_speed, -2.8);
        assert_eq!(tool.enchantability, 0);
    }

    #[test]
    fn item_properties_keep_legacy_fields_and_reject_contradictions() {
        let parse = |json: &str| -> Result<Content, String> {
            hmod(
                "item-legacy",
                MANIFEST_TEXT,
                &[("data/ruby/horizon/item/a.json", json.as_bytes())],
            )
        };
        let old =
            parse(r#"{"template":"minecraft:stick","max_stack_size":16,"rarity":"rare"}"#).unwrap();
        assert_eq!(old.items[0].max_stack_size, Some(16));
        assert_eq!(old.items[0].rarity, Some(Rarity::Rare));
        assert_eq!(old.items[0].properties, ItemProperties::default());
        parse(r#"{"template":"minecraft:stick","max_stack_size":16,"properties":{"max_stack_size":16}}"#).unwrap();
        for json in [
            r#"{"template":"minecraft:stick","max_stack_size":16,"properties":{"max_stack_size":32}}"#,
            r#"{"template":"minecraft:stick","rarity":"rare","properties":{"rarity":"epic"}}"#,
        ] {
            assert!(parse(json).unwrap_err().contains("contradict"));
        }
    }

    #[test]
    fn item_properties_reject_invalid_components() {
        let valid = serde_json::json!({
            "template": "minecraft:stick",
            "armor": {"slot": "legs", "defense": 5, "toughness": 2,
                "durability": 600, "texture": "minecraft:iron"}
        });
        let cases = [
            ("/armor/slot", serde_json::json!("hand"), "slot"),
            ("/armor/defense", serde_json::json!(-1), "defense"),
            ("/armor/toughness", serde_json::json!(-1), "toughness"),
            ("/armor/durability", serde_json::json!(0), "durability"),
            (
                "/armor/durability",
                serde_json::json!(2147483648u32),
                "durability",
            ),
            ("/armor/texture", serde_json::json!("other:iron"), "texture"),
            (
                "/armor/texture",
                serde_json::json!("ruby:../iron"),
                "texture",
            ),
        ];
        let namespaces = vec!["ruby".to_owned()];
        for (pointer, value, error) in cases {
            let mut json = valid.clone();
            *json.pointer_mut(pointer).unwrap() = value;
            let file: ItemFile = serde_json::from_value(json).unwrap();
            assert!(
                check_item_properties(&file, &namespaces)
                    .unwrap_err()
                    .contains(error),
                "{pointer}"
            );
        }
        let mut file: ItemFile = serde_json::from_value(valid).unwrap();
        file.armor.as_mut().unwrap().knockback_resistance = 1.1;
        assert!(
            check_item_properties(&file, &namespaces)
                .unwrap_err()
                .contains("knockback")
        );
        file.armor.as_mut().unwrap().knockback_resistance = 0.0;
        file.properties.max_stack_size = Some(2);
        assert!(
            check_item_properties(&file, &namespaces)
                .unwrap_err()
                .contains("max_stack_size 1")
        );
        for properties in [
            serde_json::json!({"max_damage": 0}),
            serde_json::json!({"max_damage": 2147483648u32}),
            serde_json::json!({"max_damage": 10,"max_stack_size":2}),
            serde_json::json!({"enchantability":2147483648u32}),
            serde_json::json!({"repair_item":"ruby:../ingot"}),
            serde_json::json!({"attack_damage":-1}),
            serde_json::json!({"attack_speed":-4}),
        ] {
            let file: ItemFile = serde_json::from_value(serde_json::json!({
                "template":"minecraft:bow", "properties":properties
            }))
            .unwrap();
            assert!(check_item_properties(&file, &namespaces).is_err());
        }
        let mut file: ItemFile = serde_json::from_str(r#"{"template":"minecraft:stick"}"#).unwrap();
        file.properties.attack_damage = Some(f64::NAN);
        assert!(check_item_properties(&file, &namespaces).is_err());
        assert!(
            serde_json::from_str::<ItemFile>(
                r#"{"template":"minecraft:stick","properties":{"unknown":1}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn item_tools_validate_stats_tags_and_exclusivity() {
        let base = serde_json::json!({"template":"minecraft:stick","tool":{
            "kind":"sword","attack_damage":7,"attack_speed":-2.4,
            "mining_speed":8,"durability":1000,"incorrect_blocks_for_drops":"#minecraft:incorrect_for_iron_tool",
            "enchantability":10,"repair_item":"#c:ingots/silver"}});
        let namespaces = vec!["ruby".to_owned()];
        let file: ItemFile = serde_json::from_value(base.clone()).unwrap();
        check_item_properties(&file, &namespaces).unwrap();
        let mut zero_stats = base.clone();
        zero_stats["tool"]["mining_speed"] = serde_json::json!(0);
        zero_stats["tool"]["enchantability"] = serde_json::json!(0);
        let file: ItemFile = serde_json::from_value(zero_stats).unwrap();
        check_item_properties(&file, &namespaces).unwrap();
        for (field, value) in [
            ("kind", serde_json::json!("drill")),
            ("attack_damage", serde_json::json!(-1)),
            ("attack_speed", serde_json::json!(-4)),
            ("mining_speed", serde_json::json!(-1)),
            ("durability", serde_json::json!(0)),
            ("enchantability", serde_json::json!(2147483648u32)),
            ("repair_item", serde_json::json!("ruby:../ingot")),
            (
                "incorrect_blocks_for_drops",
                serde_json::json!("minecraft/iron"),
            ),
        ] {
            let mut json = base.clone();
            json["tool"][field] = value;
            let file: ItemFile = serde_json::from_value(json).unwrap();
            assert!(
                check_item_properties(&file, &namespaces)
                    .unwrap_err()
                    .contains(field),
                "{field}"
            );
        }
        let mut both = base;
        both["armor"] =
            serde_json::json!({"slot":"head","defense":1,"toughness":0,"durability":10});
        let file: ItemFile = serde_json::from_value(both).unwrap();
        assert!(
            check_item_properties(&file, &namespaces)
                .unwrap_err()
                .contains("mutually exclusive")
        );
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

    /// A connecting block names a block tag and copies a `PipeBlock`.
    #[test]
    fn connecting_blocks_copy_a_pipe_and_name_a_tag() {
        let block = |json: &str| {
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/block/cable.json", json.as_bytes())];
            hmod("cable", MANIFEST_TEXT, files)
        };
        let content = block(
            r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:cable_joins"}"##,
        )
        .unwrap();
        assert_eq!(
            content.blocks[0].connects_to.as_deref(),
            Some("ruby:cable_joins")
        );
        let thin =
            r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:j", "thickness": 4}"##;
        assert_eq!(block(thin).unwrap().blocks[0].thickness, Some(4.0));
        let e = block(r#"{"template": "minecraft:chorus_plant", "thickness": 4}"#).unwrap_err();
        assert!(e.contains("connects_to"), "{e}");
        let e = block(r##"{"template": "minecraft:chorus_plant", "connects_to": "#ruby:j", "thickness": 16}"##)
            .unwrap_err();
        assert!(e.contains("2 to 14"), "{e}");
        let e = block(r##"{"template": "minecraft:stone", "connects_to": "#ruby:cable_joins"}"##)
            .unwrap_err();
        assert!(e.contains("chorus_plant"), "{e}");
        for tag in ["ruby:cable_joins", "#cable_joins", "#Ruby:Cable"] {
            let json =
                format!(r#"{{"template": "minecraft:chorus_plant", "connects_to": "{tag}"}}"#);
            let e = block(&json).unwrap_err();
            assert!(e.contains("block tag"), "{tag}: {e}");
        }
    }

    /// A sign names its wood; a standing or ceiling sign names the wall form
    /// its item places. Neither means anything on another template.
    #[test]
    fn signs_name_their_wood_and_wall_form() {
        let block = |json: &str| {
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/block/notice.json", json.as_bytes())];
            hmod("sign", MANIFEST_TEXT, files)
        };
        let files: &[(&str, &[u8])] = &[
            (
                "data/ruby/horizon/block/notice.json",
                br#"{"template": "minecraft:oak_sign", "wood_type": "ruby:elm", "wall_block": "ruby:notice_wall"}"#,
            ),
            (
                "data/ruby/horizon/block/notice_wall.json",
                br#"{"template": "minecraft:oak_wall_sign", "item": false}"#,
            ),
        ];
        let content = hmod("sign-pair", MANIFEST_TEXT, files).unwrap();
        assert_eq!(content.blocks[0].wood_type.as_deref(), Some("ruby:elm"));
        assert_eq!(
            content.blocks[0].wall_block.as_deref(),
            Some("ruby:notice_wall")
        );
        // Vanilla's woods go by their bare names, as `WoodType.name` does.
        let cherry = r#"{"template": "minecraft:cherry_wall_hanging_sign", "wood_type": "cherry", "item": false}"#;
        assert_eq!(
            block(cherry).unwrap().blocks[0].wood_type.as_deref(),
            Some("cherry")
        );
        assert_eq!(
            block(r#"{"template": "minecraft:oak_sign", "wood_type": "ruby-wood:old/elm"}"#)
                .unwrap()
                .blocks[0]
                .wood_type
                .as_deref(),
            Some("ruby-wood:old/elm")
        );
        for bad in [
            r#"{"template": "minecraft:stone", "wood_type": "ruby:elm"}"#,
            r#"{"template": "minecraft:oak_sign", "wood_type": "Ruby:Elm"}"#,
            r#"{"template": "minecraft:oak_sign", "wood_type": "a:b:c"}"#,
            r#"{"template": "minecraft:oak_sign", "wood_type": "../elm"}"#,
        ] {
            assert!(block(bad).unwrap_err().contains("wood_type"), "{bad}");
        }
        for bad in [
            r#"{"template": "minecraft:stone", "wall_block": "ruby:notice_wall"}"#,
            r#"{"template": "minecraft:oak_wall_sign", "wall_block": "ruby:notice_wall", "item": false}"#,
            r#"{"template": "minecraft:oak_sign", "wall_block": "notice_wall"}"#,
            r#"{"template": "minecraft:oak_sign", "wall_block": ":wall"}"#,
            r#"{"template": "minecraft:oak_sign", "wall_block": "ruby:"}"#,
            r#"{"template": "minecraft:oak_sign", "wall_block": "Ruby:wall"}"#,
            r#"{"template": "minecraft:oak_sign", "wall_block": "ruby:wall:other"}"#,
            r#"{"template": "minecraft:oak_sign", "wall_block": "ruby:notice_wall", "item": false}"#,
        ] {
            assert!(block(bad).unwrap_err().contains("wall_block"), "{bad}");
        }
    }

    /// A sign's wall form is one of the mod's blocks, of the matching wall
    /// template, with no item and no other sign. Anything else refuses this
    /// mod when it is read, and leaves the other mods alone.
    #[test]
    fn a_signs_wall_block_is_its_own_mods_wall_form() {
        let refused = |template: &str, wall_block: &str, wall: &str, wanted: &str| {
            let sign =
                format!(r#"{{"template": "minecraft:{template}", "wall_block": "{wall_block}"}}"#);
            let second =
                format!(r#"{{"template": "minecraft:{template}", "wall_block": "ruby:wall"}}"#);
            let mut files: Vec<(&str, &[u8])> = vec![
                ("data/ruby/horizon/block/sign.json", sign.as_bytes()),
                ("data/ruby/horizon/block/wall.json", wall.as_bytes()),
            ];
            if wanted == "already paired" {
                files.push(("data/ruby/horizon/block/second.json", second.as_bytes()));
            }
            let e = hmod("sign-wall", MANIFEST_TEXT, &files).unwrap_err();
            assert!(e.contains(wanted), "{wanted}: {e}");
        };
        let wall = r#"{"template": "minecraft:oak_wall_sign", "item": false}"#;
        for (template, wall_template) in [
            ("oak_sign", "stone"),
            ("oak_sign", "oak_wall_hanging_sign"),
            ("oak_hanging_sign", "oak_wall_sign"),
        ] {
            let other = format!(r#"{{"template": "minecraft:{wall_template}", "item": false}}"#);
            refused(template, "ruby:wall", &other, "needs a template");
        }
        refused(
            "oak_sign",
            "ruby:wall",
            r#"{"template": "minecraft:oak_wall_sign"}"#,
            "\"item\" is false",
        );
        refused("oak_sign", "ruby:wall", wall, "already paired");
        for absent in ["ruby:elsewhere", "other:wall", "minecraft:oak_wall_sign"] {
            refused("oak_sign", absent, wall, "not one of this mod's blocks");
        }

        // Declared in code, the same; nothing is added.
        let block = |id: &str, template: &str, item, wall_block: Option<&str>| Block {
            id: id.into(),
            template: template.into(),
            item,
            wall_block: wall_block.map(str::to_owned),
            ..Block::default()
        };
        let mut content = Content::declared("ruby");
        let sign = block("ruby:sign", "minecraft:oak_sign", true, Some("ruby:wall"));
        assert!(content.declare(std::slice::from_ref(&sign), &[]).is_err());
        assert!(content.blocks.is_empty());
        let wall = block("ruby:wall", "minecraft:oak_wall_sign", false, None);
        content.declare(&[sign, wall], &[]).unwrap();
        assert_eq!(content.blocks.len(), 2);
    }

    /// A creative tab: a title, an icon and its items, all item ids.
    #[test]
    fn creative_tabs_name_their_items() {
        let tab = |json: &str| {
            let files: &[(&str, &[u8])] =
                &[("data/ruby/horizon/creative_tab/gems.json", json.as_bytes())];
            hmod("tab", MANIFEST_TEXT, files)
        };
        let content =
            tab(r#"{"title": "Ruby Gems", "icon": "ruby:ruby", "items": ["ruby:ruby", "minecraft:diamond"]}"#)
                .unwrap();
        assert_eq!(
            content.creative_tabs,
            [CreativeTab {
                id: "ruby:gems".into(),
                title: "Ruby Gems".into(),
                icon: "ruby:ruby".into(),
                items: vec!["ruby:ruby".into(), "minecraft:diamond".into()],
            }]
        );
        let e = tab(r#"{"title": "Gems", "icon": "ruby", "items": []}"#).unwrap_err();
        assert!(e.contains("item ids"), "{e}");
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
        refused("shape-reversed", &[("data/ruby/horizon/block/a.json", br#"{"template":"minecraft:stone","shapes":{"property":"axis","values":{"x":[[1,0,0,0,1,1]]}}}"#)]);
        refused("shape-malformed", &[("data/ruby/horizon/block/a.json", br#"{"template":"minecraft:stone","shapes":{"property":"axis","values":{"x":[[0,0,0,1,1]]}}}"#)]);
        refused(
            "shape-no-values",
            &[(
                "data/ruby/horizon/block/a.json",
                br#"{"template":"minecraft:stone","shapes":{"property":"axis","values":{}}}"#,
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
        for (case, components, why) in [
            (
                "bare-component",
                r#"{"max_damage": 3}"#,
                "must be namespaced",
            ),
            (
                "uppercase-path",
                r#"{"minecraft:Foo": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "uppercase-namespace",
                r#"{"Example:charge": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "empty-namespace",
                r#"{":charge": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "empty-path",
                r#"{"example:": 3}"#,
                "must be namespaced and valid",
            ),
            (
                "invalid-removal",
                r#"{"!example:Foo": {}}"#,
                "must be namespaced and valid",
            ),
            (
                "own-key",
                r#"{"minecraft:rarity": "epic"}"#,
                "set rarity instead",
            ),
            (
                "own-removal",
                r#"{"!minecraft:max_stack_size": {}}"#,
                "set max_stack_size",
            ),
            (
                "removal-value",
                r#"{"!minecraft:food": true}"#,
                "a removal's value is {}",
            ),
        ] {
            let item = format!(r#"{{"template": "minecraft:stick", "components": {components}}}"#);
            let e = refused(case, &[("data/ruby/horizon/item/a.json", item.as_bytes())]);
            assert!(e.contains(why), "{case}: {e}");
        }
        let glass = br#"{"template": "minecraft:glass", "item": false}"#;
        for (case, rest) in [
            ("bare-remainder", "bowl"),
            ("missing-remainder", "ruby:bowl"),
            ("other-mod-remainder", "jade:bowl"),
            ("itemless-remainder", "ruby:glass"),
            ("air-remainder", "minecraft:air"),
        ] {
            let item =
                format!(r#"{{"template": "minecraft:stick", "crafting_remainder": "{rest}"}}"#);
            let files: &[(&str, &[u8])] = &[
                ("data/ruby/horizon/item/a.json", item.as_bytes()),
                ("data/ruby/horizon/block/glass.json", glass),
            ];
            let e = refused(case, files);
            assert!(e.contains("crafting_remainder"), "{case}: {e}");
        }
        for (case, block, why) in [
            (
                "block-removal-value",
                r#"{"template": "minecraft:stone", "components": {"!minecraft:lore": []}}"#,
                "a removal's value is {}",
            ),
            (
                "block-stack-zero",
                r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": 0}}"#,
                "integer 1 to 99",
            ),
            (
                "block-stack-100",
                r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": 100}}"#,
                "integer 1 to 99",
            ),
            (
                "block-stack-string",
                r#"{"template": "minecraft:stone", "components": {"minecraft:max_stack_size": "16"}}"#,
                "integer 1 to 99",
            ),
            (
                "block-rarity",
                r#"{"template": "minecraft:stone", "components": {"minecraft:rarity": "bogus"}}"#,
                "common, uncommon",
            ),
            (
                "block-without-item",
                r#"{"template": "minecraft:glass", "item": false,
                    "components": {"minecraft:max_stack_size": 16}}"#,
                "item is false",
            ),
        ] {
            let e = refused(
                case,
                &[("data/ruby/horizon/block/a.json", block.as_bytes())],
            );
            assert!(e.contains(why), "{case}: {e}");
        }
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
