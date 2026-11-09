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
    game,
    render::{Corner, FillShape, Rect},
};

struct Guest {
    working: bool,
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
        let hooks = [
            "image",
            "update",
            "update_short",
            "drop",
            "box",
            "placed",
            "clear",
            "screen",
            "screen_set",
            "screen_close",
            "events",
            "screen_frames",
            "budget",
            "work",
            "world_id",
            "table",
            "texture",
            "columns",
            "changes",
        ]
        .into_iter()
        .fold(Registration::new(), Registration::command)
        .on_frame();
        Ok((
            Guest {
                working: false,
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
            "changes" => {
                let (now, chunks) = client_world::changes(u64::from(at(0)), at(1));
                format!("now {now}: {chunks:?}")
            }
            _ => return Err("unknown".into()),
        }))
    }
}
horizon_mod_sdk::export_mod!(Guest);
