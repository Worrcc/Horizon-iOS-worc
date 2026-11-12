//! Horizon's official block info, in the spirit of the "what am I looking at" tooltip mods
//! (Jade, WTHIT), written for Horizon's API: the block under the crosshair (`game.raycast`), its
//! item's icon and name in the player's language (`recipes.items`), and its namespace as the
//! game or mod it comes from, in a box at the top of the screen. Living entities show their
//! display name and current/maximum health through `game.raycast-entity`.
use horizon_mod_sdk::{
    Mod, Registration, game, recipes,
    render::{self, Corner, FillShape, ItemShape, Point, Rect, Shape, TextShape},
    storage,
    ui::{self, ButtonWidget, Widget},
};
use std::collections::HashMap;

/// The box: the tooltip's dark fill and a one-pixel border.
const BACKGROUND: u32 = 0xE010_0010;
const BORDER: u32 = 0xFF50_00A0;
const NAME: u32 = 0xFF_FFFF;
/// The source line: blue, as tooltips name a mod.
const SOURCE: u32 = 0x55_55FF;
const DETAIL: u32 = 0xAA_AAAA;
const PAD: f32 = 4.0;
const ICON: f32 = 16.0;
const LINE: f32 = 10.0;

#[derive(Clone, Copy, PartialEq)]
enum Place {
    Top,
    Corner(Corner),
}
const PLACES: [(Place, &str, &str); 3] = [
    (Place::Top, "top", "Top"),
    (Place::Corner(Corner::TopLeft), "top_left", "Top left"),
    (Place::Corner(Corner::TopRight), "top_right", "Top right"),
];
/// Targets: survival block reach, or farther.
const REACHES: [(f32, &str); 3] = [(4.5, "Reach"), (16.0, "16 blocks"), (64.0, "64 blocks")];
const OPACITIES: [u32; 4] = [100, 75, 50, 25];

/// Settings page buttons.
const PLACE_BUTTON: u32 = 1;
const REACH_BUTTON: u32 = 2;
const SOURCE_BUTTON: u32 = 3;
const COORDS_BUTTON: u32 = 4;
const OPACITY_BUTTON: u32 = 5;

struct BlockInfo {
    place: usize,
    reach: usize,
    source: bool,
    coords: bool,
    opacity: usize,
    /// Item names by id, in the player's language: read once in a world.
    names: Option<HashMap<String, String>>,
    /// What the box shows now (and where), to redraw only on a change.
    shown: Option<(Vec<String>, usize)>,
}

/// `minecraft:oak_wall_sign` → "Oak Wall Sign": for a block with no item of its own.
fn pretty(path: &str) -> String {
    path.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().chain(c).collect::<String>())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The game or mod a namespace is: "Minecraft", else its words.
fn source_name(namespace: &str) -> String {
    match namespace {
        "minecraft" => "Minecraft".to_owned(),
        other => pretty(other),
    }
}

/// The item that stands for a block: its own, or the one a wall or potted variant comes from
/// (`wall_torch` → `torch`, `oak_wall_sign` → `oak_sign`, `potted_fern` → `flower_pot`).
fn item_for(block: &str, names: &HashMap<String, String>) -> Option<String> {
    let (namespace, path) = block.split_once(':').unwrap_or(("minecraft", block));
    let candidates = [
        path.to_owned(),
        path.replace("_wall_", "_"),
        path.trim_start_matches("wall_").to_owned(),
        if path.starts_with("potted_") {
            "flower_pot".to_owned()
        } else {
            String::new()
        },
    ];
    candidates
        .into_iter()
        .filter(|p| !p.is_empty())
        .map(|p| format!("{namespace}:{p}"))
        .find(|id| names.contains_key(id))
}

/// A simple tool hint from the block's path, independent of its namespace.
fn tool_for(block: &str) -> Option<&'static str> {
    let path = block.split_once(':').map_or(block, |(_, path)| path);
    if path.contains("ore")
        || [
            "stone",
            "cobblestone",
            "brick",
            "concrete",
            "obsidian",
            "deepslate",
            "netherrack",
            "terracotta",
        ]
        .iter()
        .any(|name| path.ends_with(name))
    {
        Some("minecraft:iron_pickaxe")
    } else if [
        "log",
        "wood",
        "planks",
        "stem",
        "hyphae",
        "chest",
        "crafting_table",
        "bookshelf",
        "sign",
        "fence",
        "door",
    ]
    .iter()
    .any(|name| path.contains(name))
    {
        Some("minecraft:iron_axe")
    } else if [
        "dirt",
        "grass_block",
        "sand",
        "gravel",
        "clay",
        "soul_sand",
        "snow",
        "mud",
        "podzol",
        "mycelium",
        "farmland",
    ]
    .iter()
    .any(|name| path.contains(name))
    {
        Some("minecraft:iron_shovel")
    } else {
        None
    }
}

fn with_opacity(color: u32, percent: u32) -> u32 {
    ((color >> 24) * percent / 100) << 24 | (color & 0x00FF_FFFF)
}

impl BlockInfo {
    fn cycle_opacity(&mut self) {
        self.opacity = (self.opacity + 1) % OPACITIES.len();
    }

    fn names(&mut self) -> &HashMap<String, String> {
        self.names.get_or_insert_with(|| {
            recipes::items()
                .into_iter()
                .map(|item| (item.id, item.name))
                .collect()
        })
    }

    /// The box's lines for a block: the icon's item, then the text lines.
    fn lines(&mut self, hit: &game::Hit) -> (Option<String>, Vec<(String, u32)>) {
        let (source, coords) = (self.source, self.coords);
        let names = self.names();
        let item = item_for(&hit.block, names);
        let (namespace, path) = hit
            .block
            .split_once(':')
            .unwrap_or(("minecraft", &hit.block));
        // A wall or potted variant is named as itself, not as the item it shows.
        let name = names
            .get(&hit.block)
            .cloned()
            .unwrap_or_else(|| pretty(path));
        let mut lines = vec![(name, NAME)];
        if source {
            lines.push((source_name(namespace), SOURCE));
        } else if coords {
            lines.push((
                format!("{}, {}, {}", hit.block_x, hit.block_y, hit.block_z),
                DETAIL,
            ));
        }
        (item, lines)
    }

    fn clear(&mut self) {
        if self.shown.take().is_some() {
            render::set_hud(&[]);
            render::clear_corner();
        }
    }

    fn entity_lines(&self, hit: &game::EntityHit) -> Vec<(String, u32)> {
        let (namespace, path) = hit.kind.split_once(':').unwrap_or(("minecraft", &hit.kind));
        let name = if hit.name.is_empty()
            || hit.name == format!("entity.{}", hit.kind.replace(':', "."))
        {
            pretty(path)
        } else {
            hit.name.clone()
        };
        let mut lines = vec![(name, NAME)];
        if self.source {
            lines.push((source_name(namespace), SOURCE));
        } else {
            lines.push((
                format!("Health: {} / {}", hit.health, hit.max_health),
                DETAIL,
            ));
        }
        lines
    }

    fn draw(&mut self) {
        let distance = REACHES[self.reach].0;
        let target = game::raycast_entity(distance)
            .map(|hit| (None, None, self.entity_lines(&hit)))
            .or_else(|| {
                game::raycast(distance).map(|hit| {
                    let (item, lines) = self.lines(&hit);
                    (item, tool_for(&hit.block), lines)
                })
            });
        let Some((item, tool, lines)) = target else {
            self.clear();
            return;
        };
        let key: Vec<String> = item
            .iter()
            .cloned()
            .chain(tool.map(str::to_owned))
            .chain(lines.iter().map(|(t, _)| t.clone()))
            .collect();
        if self.shown.as_ref() == Some(&(key.clone(), self.place)) {
            return;
        }
        let text_x = if item.is_some() {
            PAD + ICON + PAD
        } else {
            PAD
        };
        let widest = lines
            .iter()
            .enumerate()
            .map(|(i, (t, _))| {
                render::text_width(t)
                    + if i == 0 && tool.is_some() {
                        PAD + ICON
                    } else {
                        0.0
                    }
            })
            .fold(0.0, f32::max);
        let w = text_x + widest + PAD;
        let icon_height = if item.is_some() || tool.is_some() {
            ICON
        } else {
            0.0
        };
        let h = PAD * 2.0 + (lines.len() as f32 * LINE).max(icon_height);
        let mut shapes = vec![
            Shape::Fill(FillShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                },
                color: with_opacity(BORDER, OPACITIES[self.opacity]),
            }),
            Shape::Fill(FillShape {
                rect: Rect {
                    x: 1.0,
                    y: 1.0,
                    w: w - 2.0,
                    h: h - 2.0,
                },
                color: with_opacity(BACKGROUND, OPACITIES[self.opacity]),
            }),
        ];
        if let Some(item) = item {
            shapes.push(Shape::Item(ItemShape {
                item,
                at: Point {
                    x: PAD,
                    y: (h - ICON) / 2.0,
                },
                scale: 1.0,
            }));
        }
        let top = (h - lines.len() as f32 * LINE) / 2.0 + 1.0;
        if let Some(tool) = tool {
            shapes.push(Shape::Item(ItemShape {
                item: tool.to_owned(),
                at: Point {
                    x: text_x + render::text_width(&lines[0].0) + PAD,
                    y: top - 1.0 + (LINE - ICON) / 2.0,
                },
                scale: 1.0,
            }));
        }
        for (i, (text, color)) in lines.into_iter().enumerate() {
            shapes.push(Shape::Text(TextShape {
                text,
                at: Point {
                    x: text_x,
                    y: top + i as f32 * LINE,
                },
                color,
                centered: false,
                shadow: true,
            }));
        }
        match PLACES[self.place].0 {
            Place::Corner(corner) => {
                render::set_hud(&[]);
                render::set_corner(corner, w, h, &shapes);
            }
            Place::Top => {
                render::clear_corner();
                let (width, _) = render::gui_size();
                let (dx, dy) = (((width - w) / 2.0).floor(), 4.0);
                for shape in &mut shapes {
                    offset(shape, dx, dy);
                }
                render::set_hud(&shapes);
            }
        }
        self.shown = Some((key, self.place));
    }

    /// The settings page: the screen's title is the mod's name already.
    fn page(&self) {
        let on = |b: bool| if b { "ON" } else { "OFF" };
        ui::set_settings(&[
            Widget::Button(ButtonWidget {
                id: PLACE_BUTTON,
                label: format!("Position: {}", PLACES[self.place].2),
            }),
            Widget::Button(ButtonWidget {
                id: REACH_BUTTON,
                label: format!("Distance: {}", REACHES[self.reach].1),
            }),
            Widget::Button(ButtonWidget {
                id: SOURCE_BUTTON,
                label: format!("Game or mod: {}", on(self.source)),
            }),
            Widget::Button(ButtonWidget {
                id: COORDS_BUTTON,
                label: format!("Coordinates: {}", on(self.coords)),
            }),
            Widget::Button(ButtonWidget {
                id: OPACITY_BUTTON,
                label: format!("Opacity: {}%", OPACITIES[self.opacity]),
            }),
        ]);
    }

    fn save(&self) {
        let flag = |b: bool| if b { "true" } else { "false" };
        let _ = storage::set("position", PLACES[self.place].1.as_bytes());
        let _ = storage::set("distance", REACHES[self.reach].0.to_string().as_bytes());
        let _ = storage::set("source", flag(self.source).as_bytes());
        let _ = storage::set("coords", flag(self.coords).as_bytes());
        let _ = storage::set("opacity", OPACITIES[self.opacity].to_string().as_bytes());
    }
}

fn offset(shape: &mut Shape, dx: f32, dy: f32) {
    match shape {
        Shape::Fill(f) => {
            f.rect.x += dx;
            f.rect.y += dy;
        }
        Shape::Item(i) => {
            i.at.x += dx;
            i.at.y += dy;
        }
        Shape::Text(t) => {
            t.at.x += dx;
            t.at.y += dy;
        }
        Shape::Quad(_) | Shape::Sprite(_) => {}
    }
}

fn stored(key: &str) -> Option<String> {
    storage::get(key)
        .and_then(|v| String::from_utf8(v).ok())
        .map(|v| v.trim().to_owned())
}

impl Mod for BlockInfo {
    fn init() -> Result<(Self, Registration), String> {
        let place = stored("position")
            .and_then(|p| PLACES.iter().position(|k| k.1 == p))
            .unwrap_or(0);
        let reach = stored("distance")
            .and_then(|d| d.parse::<f32>().ok())
            .and_then(|d| REACHES.iter().position(|r| r.0 == d))
            .unwrap_or(0);
        let info = BlockInfo {
            place,
            reach,
            source: stored("source").is_none_or(|v| v == "true"),
            coords: stored("coords").is_some_and(|v| v == "true"),
            opacity: stored("opacity")
                .and_then(|v| v.parse::<u32>().ok())
                .and_then(|v| OPACITIES.iter().position(|&o| o == v))
                .unwrap_or(0),
            names: None,
            shown: None,
        };
        info.page();
        Ok((info, Registration::new().on_tick()))
    }
    fn tick(&mut self, _tick: u64) {
        if game::get_player().is_none() {
            // Left the world: names are the next world's (its language, its mods).
            self.names = None;
            self.clear();
            return;
        }
        self.draw();
    }
    fn ui_select(&mut self, id: u32, _row: u32) {
        match id {
            PLACE_BUTTON => self.place = (self.place + 1) % PLACES.len(),
            REACH_BUTTON => self.reach = (self.reach + 1) % REACHES.len(),
            SOURCE_BUTTON => self.source = !self.source,
            COORDS_BUTTON => self.coords = !self.coords,
            OPACITY_BUTTON => self.cycle_opacity(),
            _ => return,
        }
        self.save();
        self.page();
        // Settings, including opacity, invalidate the cached shapes.
        self.clear();
        self.draw();
    }
    fn ui_settings_opened(&mut self) {
        self.page();
    }
}

horizon_mod_sdk::export_mod!(BlockInfo);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_lines_show_live_health_and_settings() {
        let mut info = BlockInfo {
            place: 0,
            reach: 0,
            source: true,
            coords: false,
            opacity: 0,
            names: None,
            shown: None,
        };
        let mut hit = game::EntityHit {
            kind: "ruby:ruby_golem".into(),
            name: "Ruby Guardian".into(),
            health: 37.5,
            max_health: 80.0,
            x: -0.5,
            y: 64.0,
            z: 3.0,
        };
        assert_eq!(
            info.entity_lines(&hit),
            vec![("Ruby Guardian".into(), NAME), ("Ruby".into(), SOURCE)]
        );
        hit.health = 12.0;
        hit.name = "entity.ruby.ruby_golem".into();
        info.source = false;
        info.coords = true;
        assert_eq!(
            info.entity_lines(&hit),
            vec![
                ("Ruby Golem".into(), NAME),
                ("Health: 12 / 80".into(), DETAIL),
            ]
        );
        for source in [false, true] {
            for coords in [false, true] {
                info.source = source;
                info.coords = coords;
                assert_eq!(info.entity_lines(&hit).len(), 2);
            }
        }
    }

    #[test]
    fn block_lines_prefer_source_and_never_exceed_two() {
        let mut info = BlockInfo {
            place: 0,
            reach: 0,
            source: true,
            coords: true,
            opacity: 0,
            names: Some(HashMap::from([("minecraft:stone".into(), "Stone".into())])),
            shown: None,
        };
        let hit = game::Hit {
            block: "minecraft:stone".into(),
            block_x: -1,
            block_y: 64,
            block_z: 3,
            x: -0.5,
            y: 64.0,
            z: 3.0,
        };
        for source in [false, true] {
            for coords in [false, true] {
                info.source = source;
                info.coords = coords;
                let (item, lines) = info.lines(&hit);
                assert_eq!(item.as_deref(), Some("minecraft:stone"));
                let mut expected = vec![("Stone".into(), NAME)];
                if source {
                    expected.push(("Minecraft".into(), SOURCE));
                } else if coords {
                    expected.push(("-1, 64, 3".into(), DETAIL));
                }
                assert_eq!(lines, expected);
                assert!(lines.len() <= 2);
            }
        }
    }

    #[test]
    fn tools_follow_block_name_rules() {
        for path in [
            "diamond_ore",
            "stone",
            "cobblestone",
            "brick",
            "white_concrete",
            "obsidian",
            "deepslate",
            "netherrack",
            "red_terracotta",
        ] {
            assert_eq!(tool_for(path), Some("minecraft:iron_pickaxe"), "{path}");
        }
        for path in [
            "oak_log",
            "oak_wood",
            "oak_planks",
            "crimson_stem",
            "warped_hyphae",
            "chest",
            "crafting_table",
            "bookshelf",
            "oak_wall_sign",
            "oak_fence",
            "oak_door",
        ] {
            assert_eq!(tool_for(path), Some("minecraft:iron_axe"), "{path}");
        }
        for path in [
            "dirt",
            "grass_block",
            "sand",
            "gravel",
            "clay",
            "soul_sand",
            "snow_block",
            "mud",
            "podzol",
            "mycelium",
            "farmland",
        ] {
            assert_eq!(tool_for(path), Some("minecraft:iron_shovel"), "{path}");
        }
        assert_eq!(tool_for("ruby:ruby_ore"), Some("minecraft:iron_pickaxe"));
        assert_eq!(
            tool_for("minecraft:sandstone"),
            Some("minecraft:iron_pickaxe")
        );
        assert_eq!(tool_for("minecraft:fire"), None);
        assert_eq!(tool_for("ore_wood:glass"), None);
        assert_eq!(tool_for("minecraft:stone_bricks"), None);
    }

    #[test]
    fn opacity_cycles_and_scales_only_fill_alpha() {
        let mut info = BlockInfo {
            place: 0,
            reach: 0,
            source: true,
            coords: false,
            opacity: 0,
            names: None,
            shown: None,
        };
        for percent in [100, 75, 50, 25, 100] {
            assert_eq!(OPACITIES[info.opacity], percent);
            assert_eq!(with_opacity(BACKGROUND, percent) >> 24, 224 * percent / 100);
            assert_eq!(with_opacity(BORDER, percent) >> 24, 255 * percent / 100);
            assert_eq!(with_opacity(BACKGROUND, percent) & 0x00FF_FFFF, 0x10_0010);
            assert_eq!(with_opacity(BORDER, percent) & 0x00FF_FFFF, 0x50_00A0);
            info.cycle_opacity();
        }
    }

    #[test]
    fn names_for_blocks_without_items() {
        assert_eq!(pretty("oak_wall_sign"), "Oak Wall Sign");
        assert_eq!(source_name("minecraft"), "Minecraft");
        assert_eq!(source_name("cool_mod"), "Cool Mod");
        let names: HashMap<String, String> = [
            "minecraft:torch",
            "minecraft:oak_sign",
            "minecraft:flower_pot",
            "minecraft:stone",
        ]
        .into_iter()
        .map(|id| (id.to_owned(), String::new()))
        .collect();
        let item = |block: &str| item_for(block, &names);
        assert_eq!(item("minecraft:stone").as_deref(), Some("minecraft:stone"));
        assert_eq!(
            item("minecraft:wall_torch").as_deref(),
            Some("minecraft:torch")
        );
        assert_eq!(
            item("minecraft:oak_wall_sign").as_deref(),
            Some("minecraft:oak_sign")
        );
        assert_eq!(
            item("minecraft:potted_fern").as_deref(),
            Some("minecraft:flower_pot")
        );
        assert_eq!(item("minecraft:fire"), None);
    }
}
