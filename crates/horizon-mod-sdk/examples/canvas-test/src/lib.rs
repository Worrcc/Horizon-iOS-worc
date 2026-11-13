//! An example of capability `render`'s canvas and capability `world-read`
//! (API level 26): a 64 × 64 gradient the mod paints itself, shown round in
//! a box at the top right and behind an item icon on its canvas screen, and
//! the top block of the column under the
//! player, read from the chunk with `client-world.columns`, said in chat
//! whenever it changes (`/top` says it again).
use horizon_mod_sdk::{
    Mod, NO_TOP, Registration,
    canvas::{self, ImageShape, Shape},
    chat,
    client_world::{self, BlockStates, Layer},
    game, input,
    render::{Corner, FillShape, ItemShape, Point, Rect},
};

/// The image's side in texels, and the box's margin around it.
const SIDE: u32 = 64;
const MARGIN: f32 = 2.0;

/// The gradient: red across, green down, blue fading out across.
fn gradient() -> Vec<u8> {
    let mut rgba = Vec::with_capacity((SIDE * SIDE * 4) as usize);
    for y in 0..SIDE {
        for x in 0..SIDE {
            rgba.extend_from_slice(&[(x * 4) as u8, (y * 4) as u8, 255 - (x * 2) as u8, 255]);
        }
    }
    rgba
}

struct CanvasTest {
    table: BlockStates,
    /// What was last said, so a line comes only when it changes.
    said: Option<String>,
    image: u32,
}

impl CanvasTest {
    /// M (or /canvas) opens the gradient as a full-window canvas.
    fn open_canvas(&self) {
        let (w, h) = horizon_mod_sdk::render::gui_size();
        canvas::open_canvas_screen(
            "Canvas test",
            &[Shape::Image(ImageShape {
                image: self.image,
                src: Rect {
                    x: 0.0,
                    y: 0.0,
                    w: SIDE as f32,
                    h: SIDE as f32,
                },
                dst: Rect {
                    x: 12.0,
                    y: 12.0,
                    w: w - 24.0,
                    h: h - 24.0,
                },
                tint: 0xFFFF_FFFF,
                round: false,
            })],
        );
        canvas::set_screen_items(&[ItemShape {
            item: "minecraft:diamond".into(),
            at: Point { x: 16.0, y: 16.0 },
            scale: 2.0,
        }]);
    }
    /// The block id of state `id`, reading the table again when the packs
    /// reloaded or it holds no such state.
    fn block(&mut self, id: u32) -> Option<String> {
        if id as usize >= self.table.state_block.len() {
            self.table = client_world::block_table();
        }
        let block = *self.table.state_block.get(id as usize)?;
        self.table.blocks.get(usize::from(block)).cloned()
    }
    /// "Top block under you: minecraft:grass_block (y 63)", from the chunk's
    /// columns; none outside a world.
    fn top_line(&mut self) -> Option<String> {
        let p = game::get_player()?;
        let (bx, bz) = (p.x.floor() as i32, p.z.floor() as i32);
        let chunk = (bx >> 4, bz >> 4);
        let read = client_world::columns(&[chunk], Layer::Surface, 1).ok()?;
        let Some(Some(columns)) = read.into_iter().next() else {
            return Some("The chunk under you isn't loaded".into());
        };
        let i = ((bz & 15) * 16 + (bx & 15)) as usize;
        let top = *columns.tops.get(i)?;
        if top == NO_TOP {
            return Some("No block under you".into());
        }
        let state = *columns.states.get(i)?;
        let name = self
            .block(state)
            .unwrap_or_else(|| format!("state {state}"));
        Some(format!("Top block under you: {name} (y {top})"))
    }
}

impl Mod for CanvasTest {
    fn init() -> Result<(Self, Registration), String> {
        let image = canvas::create_image(SIDE, SIDE)?;
        canvas::update_image(image, 0, 0, SIDE, SIDE, &gradient())?;
        let side = SIDE as f32;
        let full = side + 2.0 * MARGIN;
        canvas::set_placed(
            Corner::TopRight,
            12.0,
            20.0,
            full,
            full,
            &[
                Shape::Fill(FillShape {
                    rect: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: full,
                        h: full,
                    },
                    color: 0x7800_0000,
                }),
                Shape::Image(ImageShape {
                    image,
                    src: Rect {
                        x: 0.0,
                        y: 0.0,
                        w: side,
                        h: side,
                    },
                    dst: Rect {
                        x: MARGIN,
                        y: MARGIN,
                        w: side,
                        h: side,
                    },
                    tint: 0xFFFF_FFFF,
                    round: true,
                }),
            ],
        );
        input::register_key("canvas", "Canvas screen", "key.keyboard.m")?;
        let table = client_world::block_table();
        let hooks = Registration::new()
            .command("top")
            .command("canvas")
            .on_tick()
            .on_frame();
        Ok((
            CanvasTest {
                table,
                said: None,
                image,
            },
            hooks,
        ))
    }
    fn tick(&mut self, _tick: u64) {
        let line = self.top_line();
        if line != self.said {
            if let Some(line) = &line {
                chat::show(line);
            }
            self.said = line;
        }
    }
    fn frame(&mut self, _seconds: f32) {
        for event in input::poll() {
            if matches!(event, input::Event::KeyDown(ref key) if key == "canvas") {
                self.open_canvas();
            }
        }
    }
    fn command(&mut self, name: &str, _args: &str) -> Result<Option<String>, String> {
        if name == "canvas" {
            self.open_canvas();
            return Ok(Some("Canvas opened".into()));
        }
        self.top_line()
            .map(Some)
            .ok_or_else(|| "Not in a world".into())
    }
}
horizon_mod_sdk::export_mod!(CanvasTest);
