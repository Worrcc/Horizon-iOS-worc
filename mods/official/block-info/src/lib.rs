//! Horizon's official block info, in the spirit of the "what am I looking at" tooltip mods
//! (Jade, WTHIT), written for Horizon's API: the block under the crosshair (`game.raycast`), its
//! item's icon and name in the player's language (`recipes.items`), and its namespace as the
//! game or mod it comes from, in a box at the top of the screen. Living entities show their
//! display name and current/maximum health through `game.raycast-entity`.
use horizon_mod_sdk::{
    Mod, Registration, client_world, game, recipes,
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
/// The harvest mark: the held item drops the block, or it does not.
const CAN_HARVEST: u32 = 0x55_FF55;
const CANNOT_HARVEST: u32 = 0xFF_5555;
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
const TOOL_BUTTON: u32 = 5;
const OPACITY_BUTTON: u32 = 6;

struct BlockInfo {
    place: usize,
    reach: usize,
    source: bool,
    coords: bool,
    /// The tool (and tier) a block needs.
    tool: bool,
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

fn with_opacity(color: u32, percent: u32) -> u32 {
    ((color >> 24) * percent / 100) << 24 | (color & 0x00FF_FFFF)
}

impl BlockInfo {
    fn cycle_opacity(&mut self) {
        self.opacity = (self.opacity + 1) % OPACITIES.len();
    }

    fn shown_key(
        &self,
        item: &Option<String>,
        lines: &[(String, u32)],
        harvest: &Option<HarvestRow>,
    ) -> Vec<String> {
        item.iter()
            .cloned()
            .chain(lines.iter().map(|(t, c)| format!("{c:06X}{t}")))
            .chain(harvest.iter().filter_map(|row| match row {
                HarvestRow::Icons(items, frame) => Some(format!("{frame:?}{}", items.join(","))),
                HarvestRow::Text(..) => None,
            }))
            .chain(std::iter::once(format!(
                "opacity:{}",
                OPACITIES[self.opacity]
            )))
            .collect()
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
    fn lines(
        &mut self,
        hit: &game::Hit,
    ) -> (Option<String>, Vec<(String, u32)>, Option<HarvestRow>) {
        let (source, coords, tool) = (self.source, self.coords, self.tool);
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
        let harvest = if tool {
            client_world::harvest(&hit.block).and_then(|h| harvest_row(&h))
        } else {
            None
        };
        (item, lines, harvest)
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
            .map(|hit| (None, self.entity_lines(&hit), None))
            .or_else(|| game::raycast(distance).map(|hit| self.lines(&hit)));
        let Some((item, lines, harvest)) = target else {
            self.clear();
            return;
        };
        let key = self.shown_key(&item, &lines, &harvest);
        if self.shown.as_ref() == Some(&(key.clone(), self.place)) {
            return;
        }
        let (w, h, mut shapes) = box_shapes(
            item,
            lines,
            harvest,
            OPACITIES[self.opacity],
            render::text_width,
        );
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
                id: TOOL_BUTTON,
                label: format!("Tool needed: {}", on(self.tool)),
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
        let _ = storage::set("tool", flag(self.tool).as_bytes());
        let _ = storage::set("opacity", OPACITIES[self.opacity].to_string().as_bytes());
    }
}

/// Build the box independently of the host, using its font's measured widths.
fn box_shapes(
    item: Option<String>,
    lines: Vec<(String, u32)>,
    harvest: Option<HarvestRow>,
    opacity: u32,
    text_width: impl Fn(&str) -> f32,
) -> (f32, f32, Vec<Shape>) {
    // Text-only harvest hints are omitted to keep the box to two text lines.
    let icons = match harvest {
        Some(HarvestRow::Icons(items, frame)) => Some((items, frame)),
        _ => None,
    };
    let text_x = if item.is_some() {
        PAD + ICON + PAD
    } else {
        PAD
    };
    let icons_w = icons.as_ref().map_or(0.0, |(items, frame)| {
        PAD + items.len() as f32 * ICON_STEP + if frame.is_some() { MARK_W } else { 0.0 }
    });
    let name_w = text_width(&lines[0].0);
    let widest = lines
        .iter()
        .skip(1)
        .map(|(text, _)| text_width(text))
        .fold(name_w + icons_w, f32::max);
    let w = text_x + widest + PAD;
    let icon_height = if item.is_some() || icons.is_some() {
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
            color: with_opacity(BORDER, opacity),
        }),
        Shape::Fill(FillShape {
            rect: Rect {
                x: 1.0,
                y: 1.0,
                w: w - 2.0,
                h: h - 2.0,
            },
            color: with_opacity(BACKGROUND, opacity),
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
    if let Some((items, frame)) = icons {
        let x = text_x + name_w + PAD;
        let y = top - 1.0 + (LINE - ICON) / 2.0;
        let n = items.len();
        for (k, item) in items.into_iter().enumerate() {
            shapes.push(Shape::Item(ItemShape {
                item,
                at: Point {
                    x: x + k as f32 * ICON_STEP,
                    y,
                },
                scale: 1.0,
            }));
        }
        if let Some(color) = frame {
            mark(
                &mut shapes,
                x + n as f32 * ICON_STEP,
                y + (ICON - 6.0) / 2.0,
                color == CAN_HARVEST,
            );
        }
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
    (w, h, shapes)
}

/// What a block needs, shown beside its name when represented by icons.
#[derive(Clone, Debug, PartialEq)]
enum HarvestRow {
    /// Tool icons; framed green or red (the held item drops the block or not)
    /// when the tool is needed, unframed when it only mines faster.
    Icons(Vec<String>, Option<u32>),
    Text(String, u32),
}

/// The tool a block needs as its icon (`minecraft:diamond_pickaxe`), the
/// tools that only mine it faster (their wooden ones), or "Unbreakable";
/// none for a block a hand breaks as well as anything.
fn harvest_row(h: &client_world::HarvestInfo) -> Option<HarvestRow> {
    if h.hardness < 0.0 {
        return Some(HarvestRow::Text("Unbreakable".to_owned(), DETAIL));
    }
    if h.needs_tool {
        let color = if h.held_drops {
            CAN_HARVEST
        } else {
            CANNOT_HARVEST
        };
        return Some(match (&h.tier, &h.tier_tool) {
            (Some(tier), Some(tool)) => {
                HarvestRow::Icons(vec![format!("minecraft:{tier}_{tool}")], Some(color))
            }
            _ => HarvestRow::Text("Needs the right tool".to_owned(), color),
        });
    }
    if h.tools.is_empty() {
        return None;
    }
    Some(HarvestRow::Icons(
        h.tools
            .iter()
            .map(|t| format!("minecraft:wooden_{t}"))
            .collect(),
        None,
    ))
}

/// One step of an icon row: the icon and a gap.
const ICON_STEP: f32 = ICON + 2.0;
/// The mark after a needed tool: a check when the held item drops the block,
/// an X when not, as pixel art (7 x 6), with a dark shadow.
const CHECK: [&str; 6] = [
    "......#", ".....##", "#...##.", "##.##..", ".###...", "..#....",
];
const CROSS: [&str; 6] = [
    "#....#.", ".#..#..", "..##...", "..##...", ".#..#..", "#....#.",
];
const MARK_W: f32 = 9.0;
const SHADOW: u32 = 0xFF20_2020;

/// The check or X at `(x, y)`: a pixel each, shadow first.
fn mark(shapes: &mut Vec<Shape>, x: f32, y: f32, ok: bool) {
    let (rows, color) = if ok {
        (CHECK, 0xFF00_0000 | CAN_HARVEST)
    } else {
        (CROSS, 0xFF00_0000 | CANNOT_HARVEST)
    };
    for (shadow, color) in [(1.0, SHADOW), (0.0, color)] {
        for (j, row) in rows.iter().enumerate() {
            for (i, c) in row.chars().enumerate() {
                if c == '#' {
                    shapes.push(Shape::Fill(FillShape {
                        rect: Rect {
                            x: x + i as f32 + shadow,
                            y: y + j as f32 + shadow,
                            w: 1.0,
                            h: 1.0,
                        },
                        color,
                    }));
                }
            }
        }
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
            tool: stored("tool").is_none_or(|v| v == "true"),
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
            TOOL_BUTTON => self.tool = !self.tool,
            OPACITY_BUTTON => self.cycle_opacity(),
            _ => return,
        }
        self.save();
        self.page();
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

    fn harvest(
        tools: &[&str],
        tier: Option<(&str, &str)>,
        needs: bool,
        held: bool,
    ) -> client_world::HarvestInfo {
        client_world::HarvestInfo {
            tools: tools.iter().map(|t| (*t).to_owned()).collect(),
            tier: tier.map(|t| t.0.to_owned()),
            tier_tool: tier.map(|t| t.1.to_owned()),
            needs_tool: needs,
            held_drops: held,
            hardness: 1.0,
        }
    }

    #[test]
    fn harvest_rows_show_the_needed_tool_framed_for_the_held_item() {
        let obsidian = harvest(&["pickaxe"], Some(("diamond", "pickaxe")), true, false);
        assert_eq!(
            harvest_row(&obsidian),
            Some(HarvestRow::Icons(
                vec!["minecraft:diamond_pickaxe".to_owned()],
                Some(CANNOT_HARVEST)
            ))
        );
        let iron = harvest(&["pickaxe"], Some(("stone", "pickaxe")), true, true);
        assert_eq!(
            harvest_row(&iron),
            Some(HarvestRow::Icons(
                vec!["minecraft:stone_pickaxe".to_owned()],
                Some(CAN_HARVEST)
            ))
        );
        let log = harvest(&["axe"], None, false, true);
        assert_eq!(
            harvest_row(&log),
            Some(HarvestRow::Icons(
                vec!["minecraft:wooden_axe".to_owned()],
                None
            ))
        );
        assert_eq!(harvest_row(&harvest(&[], None, false, true)), None);
        let mut bedrock = harvest(&[], None, false, true);
        bedrock.hardness = -1.0;
        assert_eq!(
            harvest_row(&bedrock),
            Some(HarvestRow::Text("Unbreakable".to_owned(), DETAIL))
        );
    }

    #[test]
    fn entity_lines_show_live_health_and_settings() {
        let mut info = BlockInfo {
            place: 0,
            reach: 0,
            source: true,
            coords: false,
            tool: true,
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
                let lines = info.entity_lines(&hit);
                assert_eq!(lines.len(), 2);
                assert_eq!(
                    lines[1],
                    if source {
                        ("Ruby".into(), SOURCE)
                    } else {
                        ("Health: 12 / 80".into(), DETAIL)
                    }
                );
            }
        }
    }

    #[test]
    fn block_lines_prefer_source_and_cap_text_at_two() {
        let mut info = BlockInfo {
            place: 0,
            reach: 0,
            source: true,
            coords: true,
            tool: false,
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
                let (item, lines, harvest) = info.lines(&hit);
                assert_eq!(item.as_deref(), Some("minecraft:stone"));
                assert_eq!(harvest, None);
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

    fn stone_lines() -> Vec<(String, u32)> {
        vec![("Stone".into(), NAME), ("Minecraft".into(), SOURCE)]
    }

    // A fixed-width font isolates the layout from the game's host calls.
    fn measure(text: &str) -> f32 {
        text.len() as f32 * 6.0
    }

    #[test]
    fn harvest_icons_sit_right_of_name_without_adding_a_row() {
        for frame in [None, Some(CAN_HARVEST), Some(CANNOT_HARVEST)] {
            let harvest = Some(HarvestRow::Icons(
                vec!["minecraft:wooden_axe".into(), "minecraft:wooden_hoe".into()],
                frame,
            ));
            let (w, h, shapes) = box_shapes(
                Some("minecraft:stone".into()),
                stone_lines(),
                harvest,
                100,
                measure,
            );
            assert_eq!(h, 28.0);
            assert_eq!(w, if frame.is_some() { 107.0 } else { 98.0 });
            let items: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Item(item) => Some(item),
                    _ => None,
                })
                .collect();
            assert_eq!(items.len(), 3);
            assert_eq!(items[0].item, "minecraft:stone");
            assert_eq!((items[0].at.x, items[0].at.y), (4.0, 6.0));
            assert_eq!(items[1].item, "minecraft:wooden_axe");
            assert_eq!((items[1].at.x, items[1].at.y), (58.0, 1.0));
            assert_eq!(items[2].item, "minecraft:wooden_hoe");
            assert_eq!((items[2].at.x, items[2].at.y), (76.0, 1.0));
            let texts: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Text(text) => Some(text),
                    _ => None,
                })
                .collect();
            assert_eq!(texts.len(), 2);
            assert_eq!((texts[0].at.x, texts[0].at.y), (24.0, 5.0));
            assert_eq!((texts[1].at.x, texts[1].at.y), (24.0, 15.0));
            let marks: Vec<_> = shapes
                .iter()
                .skip(2)
                .filter_map(|shape| match shape {
                    Shape::Fill(fill) => Some(fill),
                    _ => None,
                })
                .collect();
            if let Some(color) = frame {
                assert!(marks.iter().any(|fill| fill.color == 0xFF00_0000 | color));
                assert!(marks.iter().all(|fill| fill.rect.x >= 94.0));
            } else {
                assert!(marks.is_empty());
            }
        }
    }

    #[test]
    fn box_fits_long_source_and_single_line_icons() {
        let (w, h, _) = box_shapes(
            Some("minecraft:stone".into()),
            vec![
                ("Stone".into(), NAME),
                ("A very long mod name".into(), SOURCE),
            ],
            harvest_row(&harvest(&["axe"], None, false, true)),
            100,
            measure,
        );
        assert_eq!((w, h), (148.0, 28.0));
        for item in [None, Some("minecraft:stone".into())] {
            let (w, h, shapes) = box_shapes(
                item.clone(),
                vec![("Stone".into(), NAME)],
                harvest_row(&harvest(&["axe"], None, false, true)),
                100,
                measure,
            );
            assert_eq!(h, 24.0);
            assert_eq!(w, if item.is_some() { 80.0 } else { 60.0 });
            for shape in shapes {
                if let Shape::Item(icon) = shape {
                    assert!(icon.at.y >= 0.0);
                    assert!(icon.at.y + 16.0 <= h);
                    assert!(icon.at.x + 16.0 <= w);
                }
            }
        }
    }

    #[test]
    fn text_harvest_hints_do_not_add_a_third_line() {
        let mut bedrock = harvest(&[], None, false, true);
        bedrock.hardness = -1.0;
        for data in [bedrock, harvest(&[], None, true, false)] {
            let hint = harvest_row(&data);
            assert!(matches!(hint, Some(HarvestRow::Text(..))));
            let (w, h, shapes) = box_shapes(
                Some("minecraft:stone".into()),
                stone_lines(),
                hint,
                100,
                measure,
            );
            assert_eq!((w, h), (82.0, 28.0));
            let texts: Vec<_> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect();
            assert_eq!(texts, ["Stone", "Minecraft"]);
        }
    }

    #[test]
    fn opacity_cycles_scales_box_fills_and_invalidates_cache() {
        let mut info = BlockInfo {
            place: 0,
            reach: 0,
            source: true,
            coords: false,
            tool: true,
            opacity: 0,
            names: None,
            shown: None,
        };
        let item = Some("minecraft:stone".into());
        let hint = harvest_row(&harvest(
            &["pickaxe"],
            Some(("stone", "pickaxe")),
            true,
            true,
        ));
        let first_key = info.shown_key(&item, &stone_lines(), &hint);
        let mut previous_key = None;
        for (percent, border, background) in [
            (100, 0xFF50_00A0, 0xE010_0010),
            (75, 0xBF50_00A0, 0xA810_0010),
            (50, 0x7F50_00A0, 0x7010_0010),
            (25, 0x3F50_00A0, 0x3810_0010),
            (100, 0xFF50_00A0, 0xE010_0010),
        ] {
            assert_eq!(OPACITIES[info.opacity], percent);
            let key = info.shown_key(&item, &stone_lines(), &hint);
            assert_ne!(previous_key.as_ref(), Some(&key));
            previous_key = Some(key.clone());
            let (_, _, shapes) =
                box_shapes(item.clone(), stone_lines(), hint.clone(), percent, measure);
            for (shape, expected) in shapes.iter().take(2).zip([border, background]) {
                let Shape::Fill(fill) = shape else {
                    panic!("expected a box fill")
                };
                assert_eq!(fill.color, expected);
            }
            for shape in shapes.iter().skip(2) {
                match shape {
                    Shape::Text(text) => {
                        assert_eq!(text.color, if text.text == "Stone" { NAME } else { SOURCE })
                    }
                    Shape::Fill(fill) => assert_eq!(fill.color >> 24, 255),
                    _ => {}
                }
            }
            if percent == 100 {
                assert_eq!(key, first_key);
            }
            info.cycle_opacity();
        }
        info.opacity = 0;
        let cannot_harvest = harvest_row(&harvest(
            &["pickaxe"],
            Some(("stone", "pickaxe")),
            true,
            false,
        ));
        assert_ne!(
            info.shown_key(&item, &stone_lines(), &cannot_harvest),
            first_key
        );
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
