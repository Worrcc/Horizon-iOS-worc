//! Capability `render`'s canvas (docs/mods.md, `canvas`): the mods' own
//! images on the HUD atlas's mod-image page (`horizon/modimage`, the slot
//! native mods' `native_mod_image` draws into), and their canvas boxes,
//! stacked in the corners with the other panels (`extras.rs`'s
//! `draw_extras`: after the minimap, the PvP HUD, the mods' text panels and
//! their `render` corner panels, before the native mods' HUD).
//!
//! The runtime hands out places on the page and keeps the images' texels;
//! here they go into the atlas once a frame as texture patches (the path
//! the minimap and map items take), all of them again after the atlas was
//! made anew. Everything Horizon-specific about drawing them is here, so a
//! later merge of the game touches this file and not the runtime.
use super::Game;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "mods")]
use {
    super::mod_render::argb,
    horizon_client::{extras::Corner, hud::MOD_IMAGE_SPRITE},
    horizon_client::{extras_hud::Corners, hud::HudBatch},
    horizon_mods::{CanvasImageShape, CanvasShape, ItemShape, RenderCorner, Shape},
};

/// A client mod that is on imports `canvas`, as of the last time the
/// resources were read: the atlas keeps the page for it.
static WANTED: AtomicBool = AtomicBool::new(false);

/// Whether the HUD atlas holds the mod-image page (16 MiB): a native mod is
/// built in, or a client mod that is on draws its own images.
pub(super) fn mod_image_slot() -> bool {
    super::mods::built_with_native_mods() || WANTED.load(Ordering::Relaxed)
}

/// Before the resources load: whether a client mod that is on (`enabled`,
/// or a dev mod) imports `canvas`, for [`mod_image_slot`].
#[cfg(feature = "mods")]
pub(super) fn note_canvas_mods(
    enabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
) {
    let here = std::env::current_dir().unwrap_or_default();
    let mut paths = horizon_mods::find(&here.join(horizon_client::mod_list::DIR));
    paths.retain(|p| !dev.contains(p));
    paths.extend(dev.iter().cloned());
    let wanted = paths.iter().any(|path| {
        let Ok(package) = horizon_mods::Package::read(path) else {
            return false;
        };
        let m = &package.manifest;
        let on = enabled.contains(&m.id) || dev.contains(path);
        on && m.side != horizon_mods::Side::Server
            && m.capabilities.contains(&horizon_mods::Capability::Render)
            && package.wasm.as_deref().is_some_and(|wasm| {
                horizon_mods::api_level::imports(wasm)
                    .is_ok_and(|f| f.iter().any(|f| f.starts_with("canvas.")))
            })
    });
    WANTED.store(wanted, Ordering::Relaxed);
}
#[cfg(not(feature = "mods"))]
pub(super) fn note_canvas_mods(
    _enabled: &std::collections::BTreeSet<String>,
    _dev: &[std::path::PathBuf],
) {
}

/// What the window keeps for the canvas between frames.
#[cfg(feature = "mods")]
#[derive(Default)]
pub(super) struct CanvasHost {
    /// The atlas is new: every image goes up whole.
    upload_all: bool,
    /// Draw order and exact drawn bounds, including other corner overlays.
    hits: Vec<(usize, [f32; 4])>,
    hit_size: (f32, f32),
}

/// The sides of a round image's fan.
#[cfg(feature = "mods")]
const ROUND_SEGMENTS: usize = 48;

#[cfg(feature = "mods")]
impl Game<'_> {
    /// The atlas was made again (a resource reload, a new renderer): the
    /// page in it is blank.
    pub(super) fn mod_canvas_forget(&mut self) {
        self.mods.canvas.upload_all = true;
    }
    /// After the mods started: a mod drawing images needs the page, which
    /// an atlas made before it was on lacks. The resources load again then.
    pub(super) fn ensure_mod_canvas_page(&mut self) {
        if !self.mods.mods.uses_canvas()
            || !self.options.execution_role.is_client()
            || self.gui.atlas.sprites.contains_key(MOD_IMAGE_SPRITE)
        {
            return;
        }
        WANTED.store(true, Ordering::Relaxed);
        log::info!("Horizon: a mod draws its own images: reloading resources for its page");
        self.reload_packs();
    }
    /// Once a frame: the texels the mods' images changed into the atlas.
    pub(super) fn upload_mod_canvas(&mut self) {
        let all = std::mem::take(&mut self.mods.canvas.upload_all);
        let uploads = self.mods.mods.take_canvas_uploads(all);
        if uploads.is_empty() {
            return;
        }
        let Some(page) = self.gui.atlas.sprites.get(MOD_IMAGE_SPRITE).copied() else {
            return;
        };
        let (w, h) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
        let (x0, y0) = ((page.u0 * w).round() as u32, (page.v0 * h).round() as u32);
        for u in uploads {
            self.map_patches.push(horizon_client::font::Patch {
                x: x0 + u.x,
                y: y0 + u.y,
                width: u.width,
                height: u.height,
                rgba: u.rgba,
            });
        }
    }
    /// The mods' canvas boxes, stacked with the other panels in `corners`.
    pub(super) fn draw_canvas_panels(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        let mut hits = Vec::new();
        for (owner, panel) in self.mods.mods.canvas_panels() {
            if self
                .mods
                .canvas_screen
                .as_ref()
                .is_some_and(|s| s.owner == owner)
            {
                continue;
            }
            let corner = match panel.corner {
                RenderCorner::TopLeft => Corner::TopLeft,
                RenderCorner::TopRight => Corner::TopRight,
                RenderCorner::BottomLeft => Corner::BottomLeft,
                RenderCorner::BottomRight => Corner::BottomRight,
            };
            // F3 takes the top left, as it does from the other panels.
            if self.debug && corner == Corner::TopLeft {
                continue;
            }
            let at = panel
                .placed_origin((batch.width, batch.height))
                .unwrap_or_else(|| corners.place(corner, panel.width, panel.height));
            let start = batch.vertices.len();
            hits.push((owner, [at.0, at.1, panel.width, panel.height]));
            self.draw_canvas_shapes(batch, owner, &panel.shapes, &panel.items, at);
            if panel.placed.is_some() {
                batch.clip_since(
                    start,
                    at.0,
                    at.1,
                    (at.0 + panel.width).min(batch.width),
                    (at.1 + panel.height).min(batch.height),
                );
            }
        }
        self.mods.canvas.hits = hits;
        self.mods.canvas.hit_size = (batch.width, batch.height);
    }

    pub(super) fn mod_canvas_box_hit(
        &self,
        at: (f32, f32),
        size: (f32, f32),
    ) -> Option<(usize, String)> {
        if self.ui.hide_gui || self.mods.canvas.hit_size != size {
            return None;
        }
        self.mods
            .canvas
            .hits
            .iter()
            .rev()
            .find_map(|&(owner, rect)| {
                let key = self.mods.mods.canvas_box_key(owner)?;
                let (_, panel) = self.mods.mods.canvas_panels().find(|(i, _)| *i == owner)?;
                if self.debug && panel.corner == RenderCorner::TopLeft {
                    return None;
                }
                box_contains(rect, size, at).then(|| (owner, key.to_owned()))
            })
    }
    /// Shared canvas primitives, from the box or window's top left.
    pub(super) fn draw_canvas_shapes(
        &self,
        batch: &mut HudBatch,
        owner: usize,
        shapes: &[CanvasShape],
        items: &[ItemShape],
        at: (f32, f32),
    ) {
        for primitive in canvas_draw_order(shapes, items) {
            let shape = match primitive {
                CanvasPrimitive::Item(item) => {
                    self.draw_shapes(batch, &[Shape::Item(item.clone())], at);
                    continue;
                }
                CanvasPrimitive::Shape(shape) => shape,
            };
            let plain = match shape {
                CanvasShape::Image(image) => {
                    self.draw_canvas_image(batch, owner, image, at);
                    continue;
                }
                CanvasShape::Fill(f) => Shape::Fill(*f),
                CanvasShape::Quad(q) => Shape::Quad(*q),
                CanvasShape::Sprite(s) => Shape::Sprite(s.clone()),
                CanvasShape::Text(t) => Shape::Text(t.clone()),
            };
            self.draw_shapes(batch, std::slice::from_ref(&plain), at);
        }
    }
    /// One image shape: its `src` texels, clipped to the image, stretched
    /// over the matching part of `dst`, round as a fan when asked.
    fn draw_canvas_image(
        &self,
        batch: &mut HudBatch,
        owner: usize,
        shape: &CanvasImageShape,
        (ox, oy): (f32, f32),
    ) {
        let Some(image) = self.mods.mods.canvas_image(owner, shape.image) else {
            return;
        };
        let Some(page) = self.gui.atlas.sprites.get(MOD_IMAGE_SPRITE).copied() else {
            return;
        };
        let (src, dst) = (shape.src, shape.dst);
        if src.w <= 0.0 || src.h <= 0.0 || dst.w <= 0.0 || dst.h <= 0.0 {
            return;
        }
        let Some((s, d)) = clip_to_image(
            [src.x, src.y, src.w, src.h],
            [ox + dst.x, oy + dst.y, dst.w, dst.h],
            (image.width as f32, image.height as f32),
        ) else {
            return;
        };
        let (aw, ah) = (self.gui.atlas.width as f32, self.gui.atlas.height as f32);
        let u = |x: f32| page.u0 + (image.x as f32 + x) / aw;
        let v = |y: f32| page.v0 + (image.y as f32 + y) / ah;
        let color = argb(shape.tint);
        if !shape.round {
            let [x, y, w, h] = d;
            batch.quad(
                [[x, y], [x + w, y], [x + w, y + h], [x, y + h]],
                [
                    [u(s[0]), v(s[1])],
                    [u(s[0] + s[2]), v(s[1])],
                    [u(s[0] + s[2]), v(s[1] + s[3])],
                    [u(s[0]), v(s[1] + s[3])],
                ],
                color,
            );
            return;
        }
        // The circle inscribed in the whole `dst`, cut to the drawn part:
        // each point's texel follows from where it lies in `dst`.
        let [dx, dy, dw, dh] = [ox + dst.x, oy + dst.y, dst.w, dst.h];
        let (cx, cy, rx, ry) = (dx + dw / 2.0, dy + dh / 2.0, dw / 2.0, dh / 2.0);
        let texel = |px: f32, py: f32| {
            let fx = src.x + (px - dx) / dw * src.w;
            let fy = src.y + (py - dy) / dh * src.h;
            [
                u(fx.clamp(s[0], s[0] + s[2])),
                v(fy.clamp(s[1], s[1] + s[3])),
            ]
        };
        let point = |i: usize| {
            let a = i as f32 / ROUND_SEGMENTS as f32 * std::f32::consts::TAU;
            let px = (cx + rx * a.cos()).clamp(d[0], d[0] + d[2]);
            let py = (cy + ry * a.sin()).clamp(d[1], d[1] + d[3]);
            [px, py]
        };
        let c = [cx.clamp(d[0], d[0] + d[2]), cy.clamp(d[1], d[1] + d[3])];
        let tc = texel(c[0], c[1]);
        for i in 0..ROUND_SEGMENTS {
            let (p0, p1) = (point(i), point(i + 1));
            let (t0, t1) = (texel(p0[0], p0[1]), texel(p1[0], p1[1]));
            batch.quad([c, p0, p1, p1], [tc, t0, t1, t1], color);
        }
    }
}

#[cfg(feature = "mods")]
enum CanvasPrimitive<'a> {
    Shape(&'a CanvasShape),
    Item(&'a ItemShape),
}

/// Keep legacy shape ordering without items. An active item layer lifts text
/// above items, while keeping order within each of the three layers.
#[cfg(feature = "mods")]
fn canvas_draw_order<'a>(
    shapes: &'a [CanvasShape],
    items: &'a [ItemShape],
) -> impl Iterator<Item = CanvasPrimitive<'a>> {
    shapes
        .iter()
        .filter(move |s| items.is_empty() || !matches!(s, CanvasShape::Text(_)))
        .map(CanvasPrimitive::Shape)
        .chain(items.iter().map(CanvasPrimitive::Item))
        .chain(
            shapes
                .iter()
                .filter(move |s| !items.is_empty() && matches!(s, CanvasShape::Text(_)))
                .map(CanvasPrimitive::Shape),
        )
}

/// Exact box bounds as drawn, intersected with the viewport; no touch padding.
#[cfg(feature = "mods")]
fn box_contains([x, y, w, h]: [f32; 4], size: (f32, f32), at: (f32, f32)) -> bool {
    at.0 >= x.max(0.0)
        && at.1 >= y.max(0.0)
        && at.0 < (x + w).min(size.0)
        && at.1 < (y + h).min(size.1)
}

#[cfg(feature = "mods")]
/// `src` (texels `[x, y, w, h]`) cut to an image of `size`, and the part of
/// `dst` (GUI pixels) it then covers; none when nothing is left.
fn clip_to_image(src: [f32; 4], dst: [f32; 4], size: (f32, f32)) -> Option<([f32; 4], [f32; 4])> {
    let (x0, y0) = (src[0].max(0.0), src[1].max(0.0));
    let (x1, y1) = ((src[0] + src[2]).min(size.0), (src[1] + src[3]).min(size.1));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let (sx, sy) = (dst[2] / src[2], dst[3] / src[3]);
    Some((
        [x0, y0, x1 - x0, y1 - y0],
        [
            dst[0] + (x0 - src[0]) * sx,
            dst[1] + (y0 - src[1]) * sy,
            (x1 - x0) * sx,
            (y1 - y0) * sy,
        ],
    ))
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn mod_canvas_forget(&mut self) {}
}

#[cfg(all(test, feature = "mods"))]
mod tests {
    use super::*;

    #[test]
    fn canvas_items_draw_between_backgrounds_and_text() {
        use horizon_mods::{FillShape, RenderPoint, RenderRect, TextShape};
        let text = CanvasShape::Text(TextShape {
            text: "count".into(),
            at: RenderPoint { x: 8.0, y: 8.0 },
            color: 0xffffff,
            centered: false,
            shadow: true,
        });
        let fill = CanvasShape::Fill(FillShape {
            rect: RenderRect {
                x: 0.0,
                y: 0.0,
                w: 16.0,
                h: 16.0,
            },
            color: 0xff000000,
        });
        let shapes = [text.clone(), fill.clone(), text, fill];
        let items = [ItemShape {
            item: "minecraft:stone".into(),
            at: RenderPoint { x: 0.0, y: 0.0 },
            scale: 1.0,
        }];
        let order = |items: &[ItemShape]| {
            canvas_draw_order(&shapes, items)
                .map(|p| match p {
                    CanvasPrimitive::Shape(CanvasShape::Text(_)) => "text",
                    CanvasPrimitive::Shape(_) => "fill",
                    CanvasPrimitive::Item(_) => "item",
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(order(&items), ["fill", "fill", "item", "text", "text"]);
        assert_eq!(order(&[]), ["text", "fill", "text", "fill"]);
    }

    #[test]
    fn box_hit_uses_drawn_bounds_and_clips_to_window() {
        let size = (320.0, 180.0);
        // A stacked box includes the overlays above it in its drawn origin.
        let stacked = [2.0, 52.0, 40.0, 30.0];
        assert!(box_contains(stacked, size, (2.0, 52.0)));
        for at in [(2.0, 51.0), (1.0, 60.0), (42.0, 60.0), (20.0, 82.0)] {
            assert!(!box_contains(stacked, size, at));
        }
        let panel = horizon_mods::CanvasPanel {
            corner: RenderCorner::BottomRight,
            placed: Some((-100.0, 1000.0)),
            width: 40.0,
            height: 30.0,
            shapes: Vec::new(),
            items: Vec::new(),
        };
        let at = panel.placed_origin(size).unwrap();
        assert_eq!(at, (280.0, 0.0));
        let rect = [at.0, at.1, panel.width, panel.height];
        assert!(box_contains(rect, size, (319.0, 29.0)));
        assert!(!box_contains(rect, size, (320.0, 29.0)));
        assert!(!box_contains(rect, size, (279.0, 0.0)));
        // Oversized boxes and corner boxes partially outside the viewport.
        assert!(box_contains([-2.0, -2.0, 400.0, 200.0], size, (0.0, 0.0)));
        assert!(!box_contains([-2.0, -2.0, 400.0, 200.0], size, (-1.0, 0.0)));
        assert!(!box_contains([0.0, 0.0, 0.0, 10.0], size, (0.0, 0.0)));
    }

    #[test]
    fn a_source_past_the_image_draws_only_the_image() {
        // All of a 64 × 64 image into 128 GUI pixels: as asked.
        let whole = clip_to_image(
            [0.0, 0.0, 64.0, 64.0],
            [10.0, 10.0, 128.0, 128.0],
            (64.0, 64.0),
        );
        assert_eq!(
            whole,
            Some(([0.0, 0.0, 64.0, 64.0], [10.0, 10.0, 128.0, 128.0]))
        );
        // Its right half from x 32 to 96: only 32 to 64 exist, the left half
        // of `dst` at the same scale.
        let half = clip_to_image(
            [32.0, 0.0, 64.0, 64.0],
            [0.0, 0.0, 64.0, 64.0],
            (64.0, 64.0),
        );
        assert_eq!(
            half,
            Some(([32.0, 0.0, 32.0, 64.0], [0.0, 0.0, 32.0, 64.0]))
        );
        // Before its top left: shifted into `dst`.
        let before = clip_to_image(
            [-16.0, -16.0, 32.0, 32.0],
            [0.0, 0.0, 32.0, 32.0],
            (64.0, 64.0),
        );
        assert_eq!(
            before,
            Some(([0.0, 0.0, 16.0, 16.0], [16.0, 16.0, 16.0, 16.0]))
        );
        assert_eq!(
            clip_to_image([70.0, 0.0, 8.0, 8.0], [0.0; 4], (64.0, 64.0)),
            None
        );
    }
}
