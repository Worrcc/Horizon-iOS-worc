//! A mod for the runtime's `canvas` and `client-world` tests. Each command
//! answers in its feedback: `/image W H` makes an image (`image N`),
//! `/update ID X Y W H` fills that rectangle of it with one colour,
//! `/update_short ID` sends too few bytes, `/drop ID` drops it, `/box ID`
//! shows it in a canvas box; `/budget MICROS` asks for a frame budget
//! (`granted N`), `/work on|off` makes each frame work until `work-left`
//! says the budget is spent; `/world_id`, `/table`, `/texture NAME`,
//! `/columns N DEPTH` (N chunks around 0, 0) and `/changes SINCE RADIUS`
//! say what `game.world-id` and `client-world` read.
use horizon_mod_sdk::{
    Mod, PointerKind, Registration,
    canvas::{self, ImageShape, Shape},
    client_world::{self, Layer},
    game, input,
    render::{Corner, FillShape, ItemShape, Point, Rect},
};

struct Guest {
    working: bool,
    see_through: Vec<u32>,
    events: Vec<String>,
    screen_frames: bool,
    /// Work done in frames, so the loop isn't optimised away.
    spun: u64,
}

fn nums(args: &str) -> Vec<u32> {
    args.split_whitespace()
        .filter_map(|a| a.parse().ok())
        .collect()
}

impl Mod for Guest {
    fn init() -> Result<(Self, Registration), String> {
        input::register_key("box", "Canvas box", "key.keyboard.m")?;
        let hooks = [
            "image",
            "update",
            "update_short",
            "drop",
            "box",
            "box_key",
            "box_poll",
            "placed",
            "clear",
            "screen",
            "screen_set",
            "screen_close",
            "screen_items",
            "box_items",
            "events",
            "screen_frames",
            "budget",
            "work",
            "world_id",
            "block_at",
            "table",
            "texture",
            "columns",
            "see_through",
            "prepared",
            "prepared_read",
            "changes",
        ]
        .into_iter()
        .fold(Registration::new(), Registration::command)
        .on_frame();
        Ok((
            Guest {
                working: false,
                see_through: Vec::new(),
                events: Vec::new(),
                screen_frames: false,
                spun: 0,
            },
            hooks,
        ))
    }
    fn frame(&mut self, _seconds: f32) {
        if self.screen_frames {
            canvas::set_canvas_screen(&[Shape::Fill(FillShape {
                rect: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 10.0,
                    h: 10.0,
                },
                color: 0xFF00_FF00,
            })]);
        }
        if !self.working {
            return;
        }
        // A little work at a time, as a baker would: one column's worth.
        while canvas::work_left() > 20_000 {
            for i in 0..200u64 {
                self.spun = std::hint::black_box(self.spun.wrapping_mul(31).wrapping_add(i));
            }
        }
    }
    fn ui_pointer(&mut self, kind: PointerKind, x: f32, y: f32, button: u32) {
        let kind = match kind {
            PointerKind::Down => "down",
            PointerKind::Up => "up",
            PointerKind::Drag => "drag",
            PointerKind::Move => "move",
        };
        self.events.push(format!("{kind} {x} {y} {button}"));
    }
    fn ui_scroll(&mut self, x: f32, y: f32, steps: f32) {
        self.events.push(format!("scroll {x} {y} {steps}"));
    }
    fn ui_screen_key(&mut self, key: &str, down: bool) {
        self.events.push(format!("key {key} {down}"));
    }
    fn ui_closed(&mut self) {
        self.events.push("closed".into());
    }
    fn command(&mut self, name: &str, args: &str) -> Result<Option<String>, String> {
        let n = nums(args);
        let at = |i: usize| n.get(i).copied().unwrap_or(0);
        Ok(Some(match name {
            "box_key" => {
                canvas::set_box_key((args.trim() != "none").then(|| args.trim()))?;
                "box key set".into()
            }
            "box_poll" => input::poll()
                .into_iter()
                .filter_map(|event| match event {
                    input::Event::KeyDown(name) => Some(format!("down {name}")),
                    input::Event::KeyUp(name) => Some(format!("up {name}")),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(";"),
            "block_at" => format!(
                "{:?}",
                client_world::block_at(at(0) as i32, at(1) as i32, at(2) as i32)
            ),
            "image" => format!("image {}", canvas::create_image(at(0), at(1))?),
            "update" => {
                let (w, h) = (at(3), at(4));
                let rgba = [255u8, 0, 0, 255].repeat((w * h) as usize);
                canvas::update_image(at(0), at(1), at(2), w, h, &rgba)?;
                "updated".into()
            }
            "update_short" => {
                canvas::update_image(at(0), 0, 0, 2, 2, &[0; 15])?;
                "updated".into()
            }
            "drop" => {
                canvas::drop_image(at(0));
                "dropped".into()
            }
            "box" => {
                let r = |w: f32| Rect {
                    x: 0.0,
                    y: 0.0,
                    w,
                    h: w,
                };
                canvas::set_corner(
                    Corner::BottomLeft,
                    32.0,
                    32.0,
                    &[Shape::Image(ImageShape {
                        image: at(0),
                        src: r(16.0),
                        dst: r(32.0),
                        tint: 0xFFFF_FFFF,
                        round: false,
                    })],
                );
                "shown".into()
            }
            "placed" => {
                let f: Vec<f32> = args
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                let at = |i: usize| f.get(i).copied().unwrap_or(0.0);
                let corner = match at(0) as u32 {
                    0 => Corner::TopLeft,
                    1 => Corner::TopRight,
                    2 => Corner::BottomLeft,
                    _ => Corner::BottomRight,
                };
                let shape = Shape::Fill(FillShape {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 8.0,
                        h: 8.0,
                    },
                    color: 0xFFFF_FFFF,
                });
                canvas::set_placed(
                    corner,
                    at(1),
                    at(2),
                    at(3),
                    at(4),
                    &vec![shape; at(5) as usize],
                );
                "placed".into()
            }
            "clear" => {
                canvas::clear_corner();
                "cleared".into()
            }
            "screen" | "screen_set" => {
                let shape = Shape::Fill(FillShape {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: 8.0,
                        h: 8.0,
                    },
                    color: 0xFFFF_FFFF,
                });
                let shapes = vec![shape; at(0) as usize];
                if name == "screen" {
                    canvas::open_canvas_screen("Canvas guest", &shapes);
                } else {
                    canvas::set_canvas_screen(&shapes);
                }
                "screen shapes".into()
            }
            "screen_items" | "box_items" => {
                let f: Vec<f32> = args
                    .split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect();
                let items = vec![
                    ItemShape {
                        item: "minecraft:stone".into(),
                        at: Point {
                            x: f.get(1).copied().unwrap_or(8.0),
                            y: f.get(2).copied().unwrap_or(9.0)
                        },
                        scale: f.get(3).copied().unwrap_or(1.0),
                    };
                    at(0) as usize
                ];
                if name == "screen_items" {
                    canvas::set_screen_items(&items);
                } else {
                    canvas::set_box_items(&items);
                }
                "items set".into()
            }
            "screen_close" => {
                canvas::close_canvas_screen();
                "closed".into()
            }
            "screen_frames" => {
                self.screen_frames = true;
                "frames on".into()
            }
            "events" => std::mem::take(&mut self.events).join(";"),
            "budget" => format!("granted {}", canvas::request_frame_budget(at(0))),
            "work" => {
                self.working = args.trim() == "on";
                format!("working {}", self.working)
            }
            "world_id" => format!("{:?}", game::world_id()),
            "table" => {
                let t = client_world::block_table();
                format!(
                    "{} states, {} blocks, {} sprites",
                    t.state_block.len(),
                    t.blocks.len(),
                    t.sprites.len()
                )
            }
            "texture" => match client_world::texture(args.trim()) {
                Some(t) => format!("{}x{} {} bytes", t.width, t.height, t.rgba.len()),
                None => "none".into(),
            },
            "columns" => {
                let chunks: Vec<(i32, i32)> = (0..at(0) as i32).map(|i| (i, 0)).collect();
                let read = client_world::columns(&chunks, Layer::Surface, at(1) as u8)?;
                let loaded = read.iter().flatten().count();
                format!("{} chunks, {loaded} loaded", read.len())
            }
            "see_through" => {
                let table = client_world::block_table();
                let states: Vec<u32> = if args.trim() == "clear" {
                    Vec::new()
                } else if args.trim() == "over" {
                    vec![0; 65537]
                } else {
                    table
                        .state_block
                        .iter()
                        .enumerate()
                        .filter_map(|(id, &block)| {
                            matches!(
                                table.blocks[usize::from(block)].as_str(),
                                "minecraft:air" | "minecraft:poppy" | "minecraft:glass"
                            )
                            .then_some(id as u32)
                        })
                        .collect()
                };
                client_world::set_see_through(&states)?;
                self.see_through = states;
                "set".into()
            }
            "prepared_read" => {
                let chunks: Vec<_> = (0..at(0) as i32).map(|i| (i, 0)).collect();
                let read = client_world::prepared_columns(&chunks, Layer::Surface, at(1) as u8)?;
                let loaded = read.iter().flatten().count();
                format!("{} chunks, {loaded} loaded", read.len())
            }
            "prepared" => {
                let chunks: Vec<_> = (0..at(0) as i32).map(|i| (i, 0)).collect();
                let depth = at(1) as u8;
                let layer = if n.len() > 2 {
                    Layer::Below(at(2) as i32)
                } else {
                    Layer::Surface
                };
                let prepared = client_world::prepared_columns(&chunks, layer, depth)?;
                let raw = client_world::columns(&chunks, layer, depth)?;
                assert_eq!(prepared.len(), raw.len());
                let (mut changed, mut skipped) = (0, 0);
                for (got, original) in prepared.into_iter().zip(raw) {
                    let (got, mut original) = match (got, original) {
                        (Some(got), Some(original)) => (got, original),
                        (None, None) => continue,
                        _ => panic!("loaded chunk mismatch"),
                    };
                    // Independent reference: walk the prefix, then rebuild each
                    // row from the original with clamped source indices.
                    let d = usize::from(depth);
                    let mut counts = vec![0; 256];
                    let mut overlays = Vec::new();
                    for (col, count) in counts.iter_mut().enumerate() {
                        if original.tops[col] == i16::MIN {
                            continue;
                        }
                        let row = original.states[col * d..(col + 1) * d].to_vec();
                        let mut k = 0;
                        while k < d && self.see_through.contains(&row[k]) {
                            k += 1;
                        }
                        if k == 0 || k == d {
                            continue;
                        }
                        *count = k as u8;
                        overlays.extend_from_slice(&row[..k]);
                        original.tops[col] -= k as i16;
                        for j in 0..d {
                            original.states[col * d + j] = row[(j + k).min(d - 1)];
                        }
                    }
                    assert_eq!(got.columns.tops, original.tops);
                    assert_eq!(got.columns.states, original.states);
                    assert_eq!(got.columns.water, original.water);
                    assert_eq!(got.columns.light, original.light);
                    assert_eq!(got.columns.tints, original.tints);
                    assert_eq!(got.columns.revision, original.revision);
                    assert_eq!(got.skips, counts);
                    assert_eq!(got.skipped_states, overlays);
                    changed += counts.iter().filter(|&&k| k > 0).count();
                    skipped += overlays.len();
                }
                format!("{changed} changed, {skipped} skipped")
            }
            "changes" => {
                let (now, chunks) = client_world::changes(u64::from(at(0)), at(1));
                format!("now {now}: {chunks:?}")
            }
            _ => return Err("unknown".into()),
        }))
    }
}
horizon_mod_sdk::export_mod!(Guest);
