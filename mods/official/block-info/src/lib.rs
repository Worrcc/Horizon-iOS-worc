//! Horizon's official block info, in the spirit of the "what am I looking at" tooltip mods
//! (Jade, WTHIT), written for Horizon's API: the block under the crosshair (`game.raycast`), its
//! item's icon and name in the player's language (`recipes.items`), and its namespace as the
//! game or mod it comes from, in a box at the top of the screen. Entities wait for the planned
//! `target` interface (docs/mods.md).
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
/// Blocks: survival reach, or farther.
const REACHES: [(f32, &str); 3] = [(4.5, "Reach"), (16.0, "16 blocks"), (64.0, "64 blocks")];

/// Settings page buttons.
const PLACE_BUTTON: u32 = 1;
const REACH_BUTTON: u32 = 2;
const SOURCE_BUTTON: u32 = 3;
const COORDS_BUTTON: u32 = 4;

struct BlockInfo {
    place: usize,
    reach: usize,
    source: bool,
    coords: bool,
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

impl BlockInfo {
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
        if coords {
            lines.push((
                format!("{}, {}, {}", hit.block_x, hit.block_y, hit.block_z),
                DETAIL,
            ));
        }
        if source {
            lines.push((source_name(namespace), SOURCE));
        }
        (item, lines)
    }

    fn clear(&mut self) {
        if self.shown.take().is_some() {
            render::set_hud(&[]);
            render::clear_corner();
        }
    }

    fn draw(&mut self) {
        let Some(hit) = game::raycast(REACHES[self.reach].0) else {
            self.clear();
            return;
        };
        let (item, lines) = self.lines(&hit);
        let key: Vec<String> = item
            .iter()
            .cloned()
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
            .map(|(t, _)| render::text_width(t))
            .fold(0.0, f32::max);
        let w = text_x + widest + PAD;
        let h = (PAD * 2.0 + lines.len() as f32 * LINE).max(PAD * 2.0 + ICON);
        let mut shapes = vec![
            Shape::Fill(FillShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h,
                },
                color: BORDER,
            }),
            Shape::Fill(FillShape {
                rect: Rect {
                    x: 1.0,
                    y: 1.0,
                    w: w - 2.0,
                    h: h - 2.0,
                },
                color: BACKGROUND,
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
        ]);
    }

    fn save(&self) {
        let flag = |b: bool| if b { "true" } else { "false" };
        let _ = storage::set("position", PLACES[self.place].1.as_bytes());
        let _ = storage::set("distance", REACHES[self.reach].0.to_string().as_bytes());
        let _ = storage::set("source", flag(self.source).as_bytes());
        let _ = storage::set("coords", flag(self.coords).as_bytes());
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
