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
    Mod, Registration,
    canvas::{self, ImageShape, Shape},
    client_world::{self, Layer},
    game,
    render::{Corner, Rect},
};

struct Guest {
    working: bool,
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
                spun: 0,
            },
            hooks,
        ))
    }
    fn frame(&mut self, _seconds: f32) {
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
