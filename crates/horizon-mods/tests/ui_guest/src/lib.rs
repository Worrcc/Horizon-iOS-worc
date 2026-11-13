//! A mod for the runtime's `ui` tests: `/ui_screen` opens a screen with one
//! of each widget, `/ui_settings` sets a settings page, `/ui_close` closes
//! the screen; the `ui-events` calls answer in chat so the host sees them.
//! `/render_hud`, `/render_clear` and `/render_shake` test `render`;
//! `/input_claim`, `/input_poll`, `/input_held` and `/input_wheel` test
//! `input`; `/zoom FACTOR SMOOTH TURN` tests `render.set-zoom`;
//! `/corner` and `/readings` the calls the official extras mods use;
//! `/raycast DISTANCE` says what `game.raycast` sees; `/quiet A;B` runs
//! lines with `command.run-quiet` (`/quiet_many N`: N of them). `/recipes`
//! counts what `recipes` reads; `/recipes ID` describes that recipe.
//! `/browser ID...` fills `browser`'s panel (`/browser N`: the first N
//! items, `/browser` alone empties it) and registers the keys `recipe` (R)
//! and `uses` (U); `/browser_poll` says what the player did on it;
//! `/browser_recipes ID` shows the recipes making ID (`/browser_recipes`
//! alone: the items again). `/recipe_place ID [all]` places that recipe
//! with `recipes.place`; `/recipe_give ITEM [COUNT]` gives with
//! `recipes.give`.
use horizon_mod_sdk::{
    ContainerPointerKind, Mod, Registration, browser, browser_ext, channel, chat, command,
    container, game,
    input::{self, Event},
    recipes,
    render::{self, FillShape, ItemShape, Point, QuadShape, Rect, Shape, SpriteShape, TextShape},
    ui::{self, ItemWidget, ListWidget, Screen, TextWidget, Widget},
};

/// One of each shape, the text too long by 44 characters.
fn shapes() -> Vec<Shape> {
    let (w, h) = render::gui_size();
    let p = |x, y| Point { x, y };
    vec![
        Shape::Fill(FillShape {
            rect: Rect {
                x: 4.0,
                y: 4.0,
                w: 60.0,
                h: 20.0,
            },
            color: 0x80FF0000,
        }),
        Shape::Quad(QuadShape {
            corners: (
                p(w / 2.0, 10.0),
                p(w / 2.0 + 20.0, 30.0),
                p(w / 2.0, 50.0),
                p(w / 2.0 - 20.0, 30.0),
            ),
            color: 0xC000FF00,
        }),
        Shape::Sprite(SpriteShape {
            sprite: "hud/heart/full".into(),
            rect: Rect {
                x: 70.0,
                y: 4.0,
                w: 18.0,
                h: 18.0,
            },
            color: 0xFFFFFFFF,
        }),
        Shape::Item(ItemShape {
            item: "minecraft:diamond".into(),
            at: p(92.0, 4.0),
            scale: 2.0,
        }),
        Shape::Text(TextShape {
            text: format!("gui {w}x{h} {}", "x".repeat(300)),
            at: p(w / 2.0, h / 2.0 - 30.0),
            color: 0xFFFF55,
            centered: true,
            shadow: true,
        }),
    ]
}

struct Guest {
    /// `/recipe_place ID [all] later`: placed on the first tick a crafting
    /// screen is open.
    pending: Option<(i32, bool, String)>,
    container_latch: bool,
}

fn widgets() -> Vec<Widget> {
    vec![
        Widget::Text(TextWidget {
            id: 1,
            text: "Pick a row".into(),
            color: 0,
        }),
        Widget::Item(ItemWidget {
            id: 2,
            item: "minecraft:diamond".into(),
            name: String::new(),
        }),
        Widget::List(ListWidget {
            id: 7,
            rows: vec!["first".into(), "second".into(), "third".into()],
            selected: Some(1),
        }),
    ]
}

impl Mod for Guest {
    fn init() -> Result<(Self, Registration), String> {
        let hooks = [
            "container_trap",
            "container_state",
            "container_click",
            "container_take",
            "container_latch",
            "container_buttons",
            "channel28",
            "ui_screen",
            "chat_open",
            "ui_settings",
            "ui_close",
            "render_hud",
            "world_lines",
            "touch_button",
            "touch_context",
            "render_clear",
            "render_shake",
            "input_claim",
            "input_poll",
            "input_held",
            "input_wheel",
            "aim_assist",
            "look_device",
            "zoom",
            "minimap",
            "far_terrain",
            "raycast",
            "quiet",
            "quiet_many",
            "quiet_mod",
            "quiet_mod_many",
            "recipes",
            "recipe_facts",
            "browser",
            "browser_poll",
            "bookmarks",
            "browser_keep_empty",
            "item_lines",
            "bookmarks_poll",
            "browser_recipes",
            "custom_recipes",
            "recipe_place",
            "recipe_give",
            "corner",
            "readings",
        ]
        .into_iter()
        .fold(Registration::new(), Registration::command)
        .on_tick();
        container::set_screen_button(0, -24.0, 0.0, "minecraft:diamond", false, "Shift");
        Ok((
            Guest {
                pending: None,
                container_latch: false,
            },
            hooks,
        ))
    }
    fn command(&mut self, name: &str, args: &str) -> Result<Option<String>, String> {
        match name {
            "channel28" => {
                assert!(channel::send("bad:scope", &[]).is_err());
                assert!(channel::send_to_player("Alice", "state", &[]).is_err());
                channel::send("state", args.as_bytes())?;
            }

            "container_trap" => panic!("container test trap"),
            "container_state" => match container::state() {
                Some(state) => chat::show(&format!(
                    "container {} carried {}",
                    state.slots.len(),
                    state.carried.map_or(0, |s| s.count)
                )),
                None => chat::show("no container"),
            },
            "container_click" => {
                let args: Vec<_> = args.split_whitespace().collect();
                container::click(
                    args.first().and_then(|s| s.parse().ok()).unwrap_or(0),
                    args.get(1).and_then(|s| s.parse().ok()).unwrap_or(0),
                    args.get(2) == Some(&"shift"),
                );
                if let Some(state) = container::state() {
                    chat::show(&format!(
                        "after click carried {}",
                        state.carried.map_or(0, |s| s.count)
                    ));
                }
            }
            "container_take" => container::take_drag(),
            "container_latch" => {
                self.container_latch = args == "on";
                container::set_shift_latch(self.container_latch);
            }
            "container_buttons" => {
                if args == "off" {
                    container::clear_screen_button(0);
                } else {
                    for id in 0..10 {
                        container::set_screen_button(
                            id,
                            -24.0,
                            id as f32 * 22.0,
                            "minecraft:diamond",
                            true,
                            &"tip".repeat(100),
                        );
                    }
                }
            }

            "world_lines" => {
                let count = args.parse::<usize>().unwrap_or(0);
                render::set_world_lines(
                    &vec![
                        render::WorldLine {
                            x1: 1.0,
                            y1: 2.0,
                            z1: 3.0,
                            x2: 4.0,
                            y2: 5.0,
                            z2: 6.0
                        };
                        count
                    ],
                    0xFF112233,
                    0x80112233,
                );
            }
            "touch_context" => {
                input::register_key("mine", "Mine", "key.keyboard.r")?;
                if args == "clear" {
                    input::set_touch_button_context("mine", None)?;
                } else {
                    let mut parts = args.split_whitespace();
                    let name = parts.next().unwrap_or("mine");
                    let ids = |s: &str| {
                        if s == "-" {
                            Vec::new()
                        } else {
                            s.split(',').map(str::to_owned).collect()
                        }
                    };
                    let context = input::TouchContext {
                        riding: ids(parts.next().unwrap_or("-")),
                        holding: ids(parts.next().unwrap_or("-")),
                    };
                    input::set_touch_button_context(name, Some(&context))?;
                }
            }
            "touch_button" => {
                input::register_key("mine", "Mine", "key.keyboard.r")?;
                input::set_touch_button(
                    "mine",
                    (args != "off")
                        .then(|| input::TouchButton {
                            label: "Mine!".into(),
                            icon: Some("minecraft:diamond_pickaxe".into()),
                            anchor: render::Corner::TopLeft,
                            dx: 4.0,
                            dy: 5.0,
                            size: 24.0,
                            lit: true,
                        })
                        .as_ref(),
                )?;
                chat::show(&format!("touch {}", input::touch_active()));
            }
            "ui_screen" => ui::open_screen(&Screen {
                title: "Guest's screen".into(),
                widgets: widgets(),
            }),
            "chat_open" => chat::open(args),
            "ui_settings" => ui::set_settings(&widgets()),
            "ui_close" => ui::close_screen(),
            "render_hud" => render::set_hud(&shapes()),
            "render_clear" => render::set_hud(&[]),
            // An FPS-counter-like box: as wide as its text plus 4.
            "corner" => {
                let text = "60 fps";
                let w = render::text_width(text) + 4.0;
                render::set_corner(
                    render::Corner::TopRight,
                    w,
                    12.0,
                    &[
                        Shape::Fill(FillShape {
                            rect: Rect {
                                x: 0.0,
                                y: 0.0,
                                w,
                                h: 12.0,
                            },
                            color: 0x78000000,
                        }),
                        Shape::Text(TextShape {
                            text: text.into(),
                            at: Point { x: 2.0, y: 2.0 },
                            color: 0xFFFFFF,
                            centered: false,
                            shadow: true,
                        }),
                    ],
                );
                ui::set_settings(&[Widget::Button(ui::ButtonWidget {
                    id: 3,
                    label: "Corner: Top Right".into(),
                })]);
            }
            "readings" => {
                let f = render::frame_stats();
                let fwd = input::mapping("key.forward")
                    .map_or("none".into(), |m| format!("{} {}", m.label, m.down));
                let (l, r) = input::clicks_per_second();
                let none = input::mapping("key.inventory").is_none();
                chat::show(&format!(
                    "{} {} {}; forward {fwd}; cps {l} {r}; other {none}",
                    f.fps, f.min, f.low
                ));
            }
            "input_claim" => {
                input::claim_items(&["minecraft:stick".into()]);
                input::register_key("wheel", "Open the wheel", "key.keyboard.r")?;
                // Not a key: refused.
                let bad = input::register_key("bad", "Bad", "R").unwrap_err();
                chat::show(&format!("claimed; {bad}"));
            }
            "input_poll" => {
                for event in input::poll() {
                    chat::show(&match event {
                        Event::UseStart(u) => format!("use-start {} {}", u.item.id, u.ticks),
                        Event::UseStop(u) => format!("use-stop {} {}", u.item.id, u.ticks),
                        Event::KeyDown(k) => format!("key-down {k}"),
                        Event::KeyUp(k) => format!("key-up {k}"),
                        Event::Wheel(steps) => format!("wheel {steps}"),
                    });
                }
            }
            "input_held" => {
                let held = input::held().map_or("nothing".into(), |h| {
                    format!("{} x{} in {}", h.id, h.count, h.slot)
                });
                chat::show(&format!("holding {held}"));
            }
            "input_wheel" => input::claim_wheel(args != "off"),
            "look_device" => chat::show(match input::look_device() {
                input::LookDeviceKind::Mouse => "mouse",
                input::LookDeviceKind::Controller => "controller",
                input::LookDeviceKind::Touch => "touch",
            }),
            "aim_assist" => {
                let mut words = args.split_whitespace();
                let mut number = || {
                    words
                        .next()
                        .and_then(|w| w.parse().ok())
                        .unwrap_or(f32::NAN)
                };
                let (range, cone, slowdown, pull) = (number(), number(), number(), number());
                let targets = words.next().unwrap_or("*");
                input::set_aim_assist(
                    (args != "off")
                        .then(|| input::AimAssist {
                            targets: if targets == "-" {
                                vec![]
                            } else {
                                targets.split(',').map(str::to_owned).collect()
                            },
                            range,
                            cone,
                            slowdown,
                            pull,
                            controller: true,
                            touch: true,
                        })
                        .as_ref(),
                )?;
            }
            "zoom" => {
                let mut words = args.split(' ');
                let mut number = || {
                    words
                        .next()
                        .and_then(|w| w.parse().ok())
                        .unwrap_or(f32::NAN)
                };
                let factor = number();
                let smooth = number() == 1.0;
                let turn = number();
                render::set_zoom(render::Zoom {
                    factor,
                    smooth,
                    turn,
                });
            }
            // `minimap off`, or `minimap SIZE BLOCKS-PER-PIXEL ROTATE ROUND`.
            "minimap" => {
                let words: Vec<&str> = args.split(' ').collect();
                let number = |i: usize| {
                    words
                        .get(i)
                        .and_then(|w| w.parse().ok())
                        .unwrap_or(f32::NAN)
                };
                let map = (args != "off").then(|| render::Minimap {
                    corner: render::Corner::BottomLeft,
                    size: number(0),
                    blocks_per_pixel: number(1),
                    rotate: words.get(2) == Some(&"1"),
                    round: words.get(3) == Some(&"1"),
                });
                render::set_minimap(map);
            }
            // `far_terrain off`, or `far_terrain DISTANCE REDUCED`.
            "far_terrain" => {
                let words: Vec<&str> = args.split(' ').collect();
                let far = (args != "off").then(|| render::FarTerrain {
                    distance: words.first().and_then(|w| w.parse().ok()).unwrap_or(0),
                    reduced: words.get(1) == Some(&"1"),
                });
                render::set_far_terrain(far);
            }
            "quiet_mod" => {
                command::run_mod_quiet(&args.split(';').map(str::to_owned).collect::<Vec<_>>())
            }
            "quiet_mod_many" => {
                let n: usize = args.parse().unwrap_or(0);
                command::run_mod_quiet(&(0..n).map(|i| format!("beacon {i}")).collect::<Vec<_>>());
            }
            "quiet" => command::run_quiet(&args.split(';').map(str::to_owned).collect::<Vec<_>>()),
            "quiet_many" => {
                let n: usize = args.parse().unwrap_or(0);
                command::run_quiet(
                    &(0..n)
                        .map(|i| format!("setblock {i} 300 0 stone"))
                        .collect::<Vec<_>>(),
                );
            }
            "raycast" => {
                let distance = args.parse().unwrap_or(f32::NAN);
                chat::show(&match game::raycast(distance) {
                    Some(h) => format!(
                        "hit {} at {} {} {} ({:.1} {:.1} {:.1})",
                        h.block, h.block_x, h.block_y, h.block_z, h.x, h.y, h.z
                    ),
                    None => "no hit".into(),
                });
            }
            "recipe_facts" => {
                let all = recipes::all();
                let count = browser_ext::recipe_count();
                let mut page = Vec::new();
                let mut start = 0;
                loop {
                    let next = browser_ext::recipes_page(start, 7);
                    if next.is_empty() {
                        break;
                    }
                    start += next.len() as u32;
                    page.extend(next);
                }
                let count_matches = count as usize == all.len();
                let same = page.len() == all.len()
                    && page.iter().zip(&all).all(|(a, b)| {
                        a.book_id == b.book_id
                            && a.results.len() == b.results.len()
                            && a.results.iter().zip(&b.results).all(|(x, y)| {
                                x.item == y.item
                                    && x.count == y.count
                                    && x.components == y.components
                            })
                    });
                let empty_edges = browser_ext::recipes_page(count, 5).is_empty()
                    && browser_ext::recipes_page(u32::MAX, u32::MAX).is_empty();
                chat::show(&format!(
                    "recipe count {}, all {}, count matches {}, page matches {}, empty edges {}",
                    count,
                    all.len(),
                    count_matches,
                    same,
                    empty_edges
                ));
                let facts = browser_ext::item_facts();
                let others = browser_ext::other_recipes();
                chat::show(&format!(
                    "{} facts, {} other recipes",
                    facts.len(),
                    others.len()
                ));
                for f in facts {
                    chat::show(&format!(
                        "{}: burn {}, compost {}, repaired by {:?}",
                        f.item, f.burn_ticks, f.compost_chance, f.repaired_by
                    ));
                }
                for r in others {
                    chat::show(&format!("{}: {} {}", r.id, r.kind, r.json));
                }
            }
            "recipes" => {
                let items = recipes::items();
                let all = recipes::all();
                let known = recipes::known();
                let name = |s: &recipes::Stack| {
                    let id = items.get(s.item as usize).map_or("?", |i| i.id.as_str());
                    format!("{} {id}{}", s.count, s.components)
                };
                chat::show(&match all.iter().find(|r| r.id == args) {
                    Some(r) => format!(
                        "{} {:?} {}x{} {}t slots {}, the first {} -> {} known {}",
                        r.id,
                        r.kind,
                        r.width,
                        r.height,
                        r.cooking_time,
                        r.slots.len(),
                        r.slots
                            .first()
                            .and_then(|s| s.first())
                            .map_or("-".into(), name),
                        r.results.first().map_or("-".into(), name),
                        known.contains(&r.book_id)
                    ),
                    None => format!(
                        "{} items, {} recipes, {} known, {} brewing",
                        items.len(),
                        all.len(),
                        known.len(),
                        all.iter()
                            .filter(|r| r.kind == recipes::Kind::Brewing)
                            .count()
                    ),
                });
            }
            "browser" => {
                let stack = |item: &str| {
                    let (item, components) = match item.find('[') {
                        Some(at) => (&item[..at], &item[at..]),
                        None => (item, ""),
                    };
                    browser::Stack {
                        item: item.into(),
                        count: 1,
                        components: components.into(),
                    }
                };
                let items: Vec<browser::Stack> = match args.parse::<usize>() {
                    Ok(n) => recipes::items()
                        .iter()
                        .skip(1)
                        .take(n)
                        .map(|i| stack(&i.id))
                        .collect(),
                    Err(_) => args.split_whitespace().map(stack).collect(),
                };
                browser::set_items(&items);
                input::register_key("recipe", "Show recipes", "key.keyboard.r")?;
                input::register_key("uses", "Show uses", "key.keyboard.u")?;
                chat::show(&format!("panel of {}", items.len()));
            }
            "browser_recipes" => {
                let items = recipes::items();
                let id = |i: u32| {
                    items
                        .get(i as usize)
                        .map_or(String::new(), |x| x.id.clone())
                };
                let stack = |s: &recipes::Stack| browser::Stack {
                    item: id(s.item),
                    count: s.count,
                    components: s.components.clone(),
                };
                let views: Vec<browser::RecipeView> = recipes::all()
                    .iter()
                    .filter(|r| !args.is_empty() && r.results.iter().any(|s| id(s.item) == args))
                    .map(|r| {
                        use recipes::Kind;
                        let crafting =
                            matches!(r.kind, Kind::CraftingShaped | Kind::CraftingShapeless);
                        let shapeless = (r.slots.len() as u32).clamp(1, 3);
                        browser::RecipeView {
                            layout: match r.kind {
                                Kind::CraftingShaped | Kind::CraftingShapeless => {
                                    browser::Layout::Grid
                                }
                                Kind::Stonecutting => browser::Layout::Single,
                                Kind::Smithing => browser::Layout::Smithing,
                                Kind::Brewing => browser::Layout::Brewing,
                                _ => browser::Layout::Cooking,
                            },
                            width: if r.width == 0 { shapeless } else { r.width },
                            height: if r.height == 0 {
                                (r.slots.len() as u32).div_ceil(3).max(1)
                            } else {
                                r.height
                            },
                            slots: r
                                .slots
                                .iter()
                                .map(|s| s.iter().map(stack).collect())
                                .collect(),
                            results: r.results.iter().map(stack).collect(),
                            note: if r.cooking_time > 0 {
                                format!("{} ticks", r.cooking_time)
                            } else {
                                String::new()
                            },
                            action: crafting.then_some(r.kind == Kind::CraftingShaped),
                        }
                    })
                    .collect();
                // A tab per layout shown, the first selected.
                let mut tabs: Vec<browser::Stack> = Vec::new();
                for v in &views {
                    let icon = match v.layout {
                        browser::Layout::Grid => "minecraft:crafting_table",
                        browser::Layout::Cooking => "minecraft:furnace",
                        browser::Layout::Single => "minecraft:stonecutter",
                        browser::Layout::Smithing => "minecraft:smithing_table",
                        browser::Layout::Brewing => "minecraft:brewing_stand",
                    };
                    if !tabs.iter().any(|t| t.item == icon) {
                        tabs.push(browser::Stack {
                            item: icon.into(),
                            count: 1,
                            components: String::new(),
                        });
                    }
                }
                browser::set_recipes(&format!("Recipes for {args}"), &tabs, 0, &views);
                chat::show(&format!("recipes shown: {}", views.len()));
            }
            "custom_recipes" => {
                use browser_ext::{CustomView, PlacedSlot, PlacedSprite, PlacedText, RecipeEntry};
                let stack = browser::Stack {
                    item: "minecraft:stone".into(),
                    count: 2,
                    components: String::new(),
                };
                let standard = browser::RecipeView {
                    layout: browser::Layout::Grid,
                    width: 99,
                    height: 0,
                    slots: vec![vec![stack.clone()]; 10],
                    results: vec![stack.clone()],
                    note: "standard".into(),
                    action: None,
                };
                let slot = PlacedSlot {
                    x: -20,
                    y: 500,
                    stacks: vec![stack.clone(); 65],
                };
                let custom = RecipeEntry::Custom(CustomView {
                    width: 300,
                    height: 300,
                    slots: vec![slot; 10],
                    result: (args != "no_result").then_some(PlacedSlot {
                        x: 500,
                        y: -20,
                        stacks: vec![stack.clone()],
                    }),
                    texts: vec![
                        PlacedText {
                            x: 500,
                            y: -20,
                            text: "界".repeat(300),
                            color: 0x123456
                        };
                        17
                    ],
                    sprites: vec![
                        PlacedSprite {
                            x: -20,
                            y: 500,
                            sprite: "container/furnace/lit_progress".into()
                        };
                        17
                    ],
                    action: Some(true),
                });
                if args == "standard" {
                    browser::set_recipes("standard", &[], 0, &[standard]);
                } else {
                    let entries = if args == "empty" {
                        vec![]
                    } else {
                        vec![custom, RecipeEntry::Standard(standard)]
                    };
                    browser_ext::set_recipes("custom", &vec![stack; 17], 3, &entries);
                }
            }
            "recipe_place" => {
                let mut words = args.split_whitespace();
                let id = words.next().unwrap_or_default();
                let rest: Vec<&str> = words.collect();
                let all = rest.contains(&"all");
                let book = recipes::all()
                    .iter()
                    .find(|r| r.id == id)
                    .map(|r| r.book_id);
                if let Some(b) = book.filter(|_| rest.contains(&"later")) {
                    self.pending = Some((b, all, id.to_owned()));
                    chat::show(&format!("place {id}: waiting"));
                    return Ok(None);
                }
                chat::show(&match book.map(|b| recipes::place(b, all)) {
                    Some(Ok(())) => format!("place {id}: ok"),
                    Some(Err(e)) => format!("place {id}: {e}"),
                    None => format!("place {id}: no such recipe"),
                });
            }
            "recipe_give" => {
                let mut words = args.split_whitespace();
                let id = words.next().unwrap_or_default();
                let count = words.next().and_then(|c| c.parse().ok()).unwrap_or(1);
                let item = recipes::items().iter().position(|i| i.id == id);
                let result = item.map(|i| {
                    recipes::give(&recipes::Stack {
                        item: i as u32,
                        count,
                        components: String::new(),
                    })
                });
                chat::show(&match result {
                    Some(Ok(())) => format!("give {id}: ok"),
                    Some(Err(e)) => format!("give {id}: {e}"),
                    None => format!("give {id}: no such item"),
                });
            }
            "bookmarks" => {
                let count = args.parse().unwrap_or(0);
                let mut items = vec![
                    browser::Stack {
                        item: "minecraft:stone".into(),
                        count: 7,
                        components: "[minecraft:damage=1]".into(),
                    };
                    count
                ];
                if count > 1024 {
                    items[0].item = "é".repeat(300);
                    items[0].components = "界".repeat(4200);
                }
                browser_ext::set_bookmarks(&items);
            }
            // /item_lines BOOKMARKS COUNT LINE|LINE... replaces that panel's lines.
            "item_lines" => {
                let mut args = args.splitn(3, ' ');
                let bookmarks = args.next() == Some("true");
                let count = args.next().and_then(|s| s.parse().ok()).unwrap_or(0);
                let lines: Vec<String> = args
                    .next()
                    .unwrap_or("")
                    .split('|')
                    .map(str::to_owned)
                    .collect();
                let items: Vec<_> = (0..count)
                    .map(|index| browser_ext::ItemLines {
                        index,
                        lines: lines.clone(),
                    })
                    .collect();
                browser_ext::set_item_lines(bookmarks, &items);
            }
            "browser_keep_empty" => browser_ext::keep_empty(args == "true"),
            "bookmarks_poll" => {
                let events = browser_ext::poll();
                chat::show(&format!("{} bookmark events", events.len()));
                for browser_ext::Event::BookmarkClicked(c) in events.into_iter().take(1) {
                    chat::show(&format!("bookmark {} {}", c.index, c.right));
                }
            }
            "browser_poll" => {
                let stack = |s: &browser::Stack| format!("{} x{}{}", s.item, s.count, s.components);
                // A hook shows at most 8 chat lines: the count, then the first 7.
                let events = browser::poll();
                chat::show(&format!("{} events", events.len()));
                for event in events.into_iter().take(7) {
                    chat::show(&match event {
                        browser::Event::Clicked(c) => {
                            format!(
                                "clicked {} {}",
                                c.index,
                                if c.right { "right" } else { "left" }
                            )
                        }
                        browser::Event::Search(text) => format!("search {text:?}"),
                        browser::Event::Key(k) => format!("key {} on {}", k.key, stack(&k.stack)),
                        browser::Event::SlotClicked(c) => {
                            format!("slot {} {} {}", c.recipe, c.slot, stack(&c.stack))
                        }
                        browser::Event::Action(i) => format!("action {i}"),
                        browser::Event::Page(first) => format!("page {first}"),
                        browser::Event::Back => "back".into(),
                        browser::Event::Tab(i) => format!("tab {i}"),
                    });
                }
            }
            // Clamped to (1, 10); the two after it shake nothing.
            "render_shake" => {
                render::shake(2.0, 99.0);
                render::shake(0.0, 1.0);
                render::shake(f32::NAN, 1.0);
            }
            _ => {}
        }
        Ok(None)
    }
    fn channel_message(&mut self, name: &str, data: &[u8]) {
        chat::show(&format!("channel {name} {}", data.len()));
    }
    fn tick(&mut self, _tick: u64) {
        if let Some((book, all, id)) = self.pending.take() {
            match recipes::place(book, all) {
                Err(e) if e.starts_with("no screen") => self.pending = Some((book, all, id)),
                Ok(()) => chat::show(&format!("place {id}: ok")),
                Err(e) => chat::show(&format!("place {id}: {e}")),
            }
        }
    }
    fn container_pointer(
        &mut self,
        kind: ContainerPointerKind,
        x: f32,
        y: f32,
        button: u32,
        slot: Option<u32>,
        shift: bool,
    ) {
        if kind == ContainerPointerKind::Down {
            container::take_drag();
        }
        chat::show(&format!(
            "container pointer {kind:?} {x} {y} {button} {slot:?} {shift}"
        ));
    }
    fn container_scroll(&mut self, _x: f32, _y: f32, steps: f32, slot: Option<u32>) -> bool {
        chat::show(&format!("container scroll {steps} {slot:?}"));
        slot.is_some()
    }
    fn container_button(&mut self, id: u32) {
        self.container_latch = !self.container_latch;
        container::set_shift_latch(self.container_latch);
        chat::show(&format!("container button {id}"));
    }
    fn container_opened(&mut self) {
        chat::show("container opened");
    }
    fn container_closed(&mut self) {
        chat::show("container closed");
    }
    fn ui_select(&mut self, id: u32, row: u32) {
        chat::show(&format!("selected {id}:{row}"));
    }
    fn ui_closed(&mut self) {
        chat::show("closed");
    }
    fn ui_settings_opened(&mut self) {
        ui::set_settings(&[
            Widget::Text(TextWidget {
                id: 1,
                text: "fresh".into(),
                color: 0x33CC33,
            }),
            Widget::Button(ui::ButtonWidget {
                id: 3,
                label: "Corner: Top Right".into(),
            }),
        ]);
        chat::show("settings opened");
    }
}

horizon_mod_sdk::export_mod!(Guest);
