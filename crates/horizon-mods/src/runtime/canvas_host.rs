//! Capability `render`'s `canvas` interface (API level 18): a mod's own
//! images on one shared atlas page, its canvas corner box, and the frame
//! budget it asks for (docs/mods.md, "Canvas").
//!
//! The page is the game's `horizon/modimage` HUD atlas slot, [`CANVAS_PAGE`]
//! texels square, shared by every running mod: images take whole cells of
//! [`CELL`] texels, first fit, and give them back when dropped or when their
//! mod stops. Each image keeps its texels here too, so the game can upload
//! them again whole after its atlas is rebuilt (a resource pack reload).
//!
//! The budget: a mod has no clock, so the game turns the microseconds it was
//! granted into fuel for each `frame` call, from how fast the mod burned fuel
//! in its earlier calls on this device (an interpreter, Pulley on iOS, burns
//! it slower than compiled code). `work-left` counts down that soft budget;
//! the hook's fuel stays the hard cap that stops a runaway mod.
pub use super::render_bindings::horizon::mods::canvas::{
    ImageShape as CanvasImageShape, Shape as CanvasShape,
};
use super::{
    ItemShape, Mods, RenderCorner, SHAPES, State, clip,
    render_bindings::horizon::mods::canvas::{self, Image as ImageId},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// The side of the shared image page, in texels (the game's
/// `MOD_IMAGE_SIZE`).
pub const CANVAS_PAGE: u32 = 2048;
/// The page is handed out in cells of this many texels square.
const CELL: u32 = 16;
const CELLS: usize = (CANVAS_PAGE / CELL) as usize;
/// The longest side of one image.
pub const IMAGE_SIDE: u32 = 1024;
/// Texels all of a mod's images may hold together (4 MiB of RGBA).
pub const IMAGE_TEXELS: u64 = 1024 * 1024;
/// RGBA bytes a mod may upload with `update-image` between two frames.
pub const UPLOAD_BYTES: usize = 1024 << 10;
/// The default frame budget, in µs.
pub const FRAME_BUDGET: u32 = 100;
/// The largest frame budget without the mod's own canvas screen open, in µs.
pub const FRAME_BUDGET_MAX: u32 = 500;
/// The largest frame budget with the mod's own canvas screen open, in µs.
pub const SCREEN_BUDGET_MAX: u32 = 4000;
/// The soft budget's floor (the hook's hard cap always wins).
const SOFT_MIN: u64 = 50_000;
/// Leave fuel to finish the last work batch and return from the hook after
/// `work-left` reaches zero, even when a fast device reaches the hard cap.
const SOFT_RESERVE: u64 = 50_000;
/// `work-left` at the start of a call.
pub(super) const WORK_SCALE: u64 = 1_000_000;
/// Fuel per microsecond assumed before a mod's first measured frame: low, so
/// the first frames err short (compiled code burns about 1000).
const FIRST_RATE: f64 = 100.0;
/// A frame call burning less fuel than this, or over in less time, says
/// little about the rate (the call's own overhead dominates): not counted.
const SAMPLE_FUEL: u64 = 20_000;
/// How much each measured frame moves the rate.
const RATE_WEIGHT: f64 = 0.25;

/// Which cells of the page are taken, a bit per cell, a row per cell row.
#[derive(Debug)]
pub(super) struct Page {
    rows: [u128; CELLS],
}
impl Default for Page {
    fn default() -> Self {
        Self { rows: [0; CELLS] }
    }
}
/// A run of `w` cells from `x`, as a row's bits.
fn bits(x: usize, w: usize) -> u128 {
    let run = if w >= 128 {
        u128::MAX
    } else {
        (1u128 << w) - 1
    };
    run << x
}
impl Page {
    /// The first free `w` × `h` cells, top to bottom then left to right.
    fn alloc(&mut self, w: usize, h: usize) -> Option<(usize, usize)> {
        if w == 0 || h == 0 || w > CELLS || h > CELLS {
            return None;
        }
        for y in 0..=CELLS - h {
            for x in 0..=CELLS - w {
                let run = bits(x, w);
                if self.rows[y..y + h].iter().all(|row| row & run == 0) {
                    for row in &mut self.rows[y..y + h] {
                        *row |= run;
                    }
                    return Some((x, y));
                }
            }
        }
        None
    }
    fn free(&mut self, (x, y, w, h): (usize, usize, usize, usize)) {
        for row in &mut self.rows[y..y + h] {
            *row &= !bits(x, w);
        }
    }
}
/// The page every mod's store shares.
pub(super) type PageSlot = Arc<Mutex<Page>>;

/// Where an image is on the page, in texels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Texels for the game to write into the page before the next frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanvasUpload {
    /// The top left, on the page.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// `width × height × 4` bytes, rows top first.
    pub rgba: Vec<u8>,
}

struct Image {
    /// Its cells: x, y, columns, rows.
    cells: (usize, usize, usize, usize),
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    /// Texels changed since the last upload: x0, y0, x1, y1 (exclusive).
    dirty: Option<(u32, u32, u32, u32)>,
}
impl Image {
    fn rect(&self) -> ImageRect {
        ImageRect {
            x: self.cells.0 as u32 * CELL,
            y: self.cells.1 as u32 * CELL,
            width: self.width,
            height: self.height,
        }
    }
    fn touch(&mut self, x0: u32, y0: u32, x1: u32, y1: u32) {
        self.dirty = Some(match self.dirty {
            None => (x0, y0, x1, y1),
            Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
        });
    }
}

/// A mod's canvas box: stacked in a corner, or placed at inward offsets.
#[derive(Clone, Debug)]
pub struct CanvasPanel {
    pub corner: RenderCorner,
    /// Inward offsets for set-placed; none for a stacked corner box.
    pub placed: Option<(f32, f32)>,
    pub width: f32,
    pub height: f32,
    pub shapes: Vec<CanvasShape>,
    pub items: Vec<ItemShape>,
}

impl CanvasPanel {
    /// The placed box's top left in GUI pixels, clamped after a resize too.
    /// A box larger than the window is clipped to the window by the game.
    pub fn placed_origin(&self, size: (f32, f32)) -> Option<(f32, f32)> {
        let (dx, dy) = self.placed?;
        let x = (size.0 - self.width).max(0.0);
        let y = (size.1 - self.height).max(0.0);
        let right = matches!(
            self.corner,
            RenderCorner::TopRight | RenderCorner::BottomRight
        );
        let bottom = matches!(
            self.corner,
            RenderCorner::BottomLeft | RenderCorner::BottomRight
        );
        Some((
            if right { x - dx } else { dx }.clamp(0.0, x),
            if bottom { y - dy } else { dy }.clamp(0.0, y),
        ))
    }
}

/// A mod's full-window canvas, in GUI pixels from the window top left.
#[derive(Clone, Debug)]
pub struct CanvasScreen {
    pub title: String,
    pub shapes: Vec<CanvasShape>,
    pub items: Vec<ItemShape>,
}
/// The most shapes a full-window canvas may hold.
pub const SCREEN_SHAPES: usize = 4096;
/// The most items on one canvas screen or box.
pub const CANVAS_ITEMS: usize = 256;

/// The granted frame budget and how fast the mod burns fuel here.
#[derive(Clone, Copy, Debug)]
pub(super) struct Budget {
    /// The mod's request, retained across screen and thermal changes.
    requested: u32,
    /// Microseconds per `frame` call.
    pub(super) granted: u32,
    /// Fuel per microsecond, measured over the mod's frame calls.
    rate: Option<f64>,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            requested: FRAME_BUDGET,
            granted: FRAME_BUDGET,
            rate: None,
        }
    }
}
impl Budget {
    /// The soft budget of the next frame call, in fuel.
    pub(super) fn allowance(&self, cap: u64) -> u64 {
        let fuel = self.rate.unwrap_or(FIRST_RATE) * f64::from(self.granted);
        (fuel as u64)
            .max(SOFT_MIN)
            .min(cap.saturating_sub(SOFT_RESERVE))
    }
    /// The fuel `took` of host time counts for at the measured rate.
    pub(super) fn fuel_for(&self, took: Duration) -> u64 {
        (self.rate.unwrap_or(FIRST_RATE) * took.as_secs_f64() * 1e6) as u64
    }
    /// A frame call burned `fuel` in `took` of the mod's own code (its
    /// charged host calls' time taken off).
    pub(super) fn observe(&mut self, fuel: u64, took: Duration) {
        let micros = took.as_secs_f64() * 1e6;
        if fuel < SAMPLE_FUEL || micros < 1.0 {
            return;
        }
        let rate = fuel as f64 / micros;
        self.rate = Some(match self.rate {
            None => rate,
            Some(old) => old + (rate - old) * RATE_WEIGHT,
        });
    }
}

/// Wall-clock times of a mod's `frame` calls over a window, for the log
/// `HORIZON_MOD_FRAME_TIMES=N` asks for (one line per N calls): the budget
/// controller works in fuel, this checks what it bought in time.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct FrameTimes {
    calls: u32,
    sum: Duration,
    max: Duration,
    /// Calls over the 0.3 ms and the 0.5 ms marks.
    over_300: u32,
    over_500: u32,
}
impl FrameTimes {
    /// Adds a call; past `window` calls, the window's line, and a new window.
    pub(super) fn record(&mut self, took: Duration, window: u32) -> Option<String> {
        self.calls += 1;
        self.sum += took;
        self.max = self.max.max(took);
        self.over_300 += u32::from(took > Duration::from_micros(300));
        self.over_500 += u32::from(took > Duration::from_micros(500));
        if self.calls < window.max(1) {
            return None;
        }
        let line = format!(
            "{} calls: avg {} µs, max {} µs, {} over 300 µs, {} over 500 µs",
            self.calls,
            (self.sum / self.calls).as_micros(),
            self.max.as_micros(),
            self.over_300,
            self.over_500,
        );
        *self = Self::default();
        Some(line)
    }
}
/// The window of `HORIZON_MOD_FRAME_TIMES`, read once; None when unset.
pub(super) fn frame_times_window() -> Option<u32> {
    static WINDOW: std::sync::OnceLock<Option<u32>> = std::sync::OnceLock::new();
    *WINDOW.get_or_init(|| {
        std::env::var("HORIZON_MOD_FRAME_TIMES")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .filter(|&n: &u32| n > 0)
    })
}

/// The current hook call's fuel at its start and its soft budget, for
/// `work-left`.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Work {
    pub(super) start: u64,
    pub(super) soft: u64,
    /// Fuel the call's dear host calls (reading chunks, uploading texels)
    /// count for: they burn none, but take the frame's time.
    pub(super) host: u64,
    /// Their time, so the rate is measured over the mod's own code alone.
    pub(super) host_time: Duration,
}

/// `work-left` from the fuel the store has left now.
pub(super) fn work_left(work: Work, fuel_now: u64) -> u64 {
    if work.soft == 0 {
        return 0;
    }
    let used = work
        .start
        .saturating_sub(fuel_now)
        .saturating_add(work.host);
    let left = work.soft.saturating_sub(used);
    (u128::from(left) * u128::from(WORK_SCALE) / u128::from(work.soft)) as u64
}

/// What capability `render`'s canvas holds for one mod.
pub(super) struct CanvasState {
    page: PageSlot,
    images: BTreeMap<ImageId, Image>,
    next: ImageId,
    texels: u64,
    /// RGBA bytes `update-image` took since the game last took the uploads.
    uploaded: usize,
    pub(super) panel: Option<CanvasPanel>,
    pub(super) box_key: Option<String>,
    pub(super) screen: Option<CanvasScreen>,
    screen_budget_limited: bool,
    pub(super) budget: Budget,
    pub(super) work: Work,
    pub(super) times: FrameTimes,
    /// The same for its other hooks' calls (mostly `tick`).
    pub(super) other_times: FrameTimes,
}
impl CanvasState {
    pub(super) fn set_screen(&mut self, screen: Option<CanvasScreen>) {
        self.screen = screen;
        self.refresh_budget();
    }
    fn refresh_budget(&mut self) {
        let max = if self.screen.is_some() && !self.screen_budget_limited {
            SCREEN_BUDGET_MAX
        } else {
            FRAME_BUDGET_MAX
        };
        self.budget.granted = self.budget.requested.clamp(1, max);
    }
    pub(super) fn limit_screen_budget(&mut self, limited: bool) {
        self.screen_budget_limited = limited;
        self.refresh_budget();
    }
    /// Counts the host's time since `started` against the call's budget.
    pub(super) fn charge(&mut self, started: Instant) {
        let took = started.elapsed();
        self.work.host_time += took;
        self.work.host = self.work.host.saturating_add(self.budget.fuel_for(took));
    }
    pub(super) fn new(page: PageSlot) -> Self {
        Self {
            page,
            images: BTreeMap::new(),
            next: 1,
            texels: 0,
            uploaded: 0,
            panel: None,
            box_key: None,
            screen: None,
            screen_budget_limited: false,
            budget: Budget::default(),
            work: Work::default(),
            times: FrameTimes::default(),
            other_times: FrameTimes::default(),
        }
    }
    fn page(&self) -> std::sync::MutexGuard<'_, Page> {
        self.page
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    fn take_uploads(&mut self, all: bool, out: &mut Vec<CanvasUpload>) {
        self.uploaded = 0;
        for image in self.images.values_mut() {
            let dirty = if all {
                Some((0, 0, image.width, image.height))
            } else {
                image.dirty
            };
            image.dirty = None;
            let Some((x0, y0, x1, y1)) = dirty else {
                continue;
            };
            let (w, h) = (x1 - x0, y1 - y0);
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for y in y0..y1 {
                let at = ((y * image.width + x0) * 4) as usize;
                rgba.extend_from_slice(&image.rgba[at..at + (w * 4) as usize]);
            }
            let r = image.rect();
            out.push(CanvasUpload {
                x: r.x + x0,
                y: r.y + y0,
                width: w,
                height: h,
                rgba,
            });
        }
    }
}
impl Drop for CanvasState {
    fn drop(&mut self) {
        let cells: Vec<_> = self.images.values().map(|i| i.cells).collect();
        let mut page = self.page();
        for c in cells {
            page.free(c);
        }
    }
}

fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// Up to `limit` canvas shapes; texts and sprite names clipped as render's are.
fn clip_canvas(shapes: Vec<CanvasShape>, limit: usize) -> Vec<CanvasShape> {
    shapes
        .into_iter()
        .take(limit)
        .map(|shape| match shape {
            CanvasShape::Image(mut i) => {
                for r in [&mut i.src, &mut i.dst] {
                    r.x = finite(r.x);
                    r.y = finite(r.y);
                    r.w = finite(r.w);
                    r.h = finite(r.h);
                }
                CanvasShape::Image(i)
            }
            CanvasShape::Sprite(mut s) => {
                s.sprite = clip(s.sprite);
                CanvasShape::Sprite(s)
            }
            CanvasShape::Text(mut t) => {
                t.text = clip(t.text);
                t.color &= 0xFF_FFFF;
                CanvasShape::Text(t)
            }
            other => other,
        })
        .collect()
}

fn clip_items(items: Vec<ItemShape>) -> Vec<ItemShape> {
    items
        .into_iter()
        .take(CANVAS_ITEMS)
        .map(|mut item| {
            item.item = clip(item.item);
            item.at.x = finite(item.at.x);
            item.at.y = finite(item.at.y);
            item.scale = if item.scale.is_finite() {
                item.scale.clamp(0.0, 16.0)
            } else {
                1.0
            };
            item
        })
        .collect()
}

impl canvas::Host for State {
    fn set_screen_items(&mut self, items: Vec<ItemShape>) {
        if let Some(screen) = &mut self.canvas.screen {
            screen.items = clip_items(items);
        }
    }
    fn set_box_items(&mut self, items: Vec<ItemShape>) {
        if let Some(panel) = &mut self.canvas.panel {
            panel.items = clip_items(items);
        }
    }
    fn set_box_key(&mut self, name: Option<String>) -> Result<(), String> {
        if let Some(name) = &name
            && !self.input.keys.iter().any(|k| &k.name == name)
        {
            return Err(format!("no registered key {name}"));
        }
        self.canvas.box_key = name;
        Ok(())
    }
    fn create_image(&mut self, width: u32, height: u32) -> Result<ImageId, String> {
        let c = &mut self.canvas;
        let side = 1..=IMAGE_SIDE;
        if !side.contains(&width) || !side.contains(&height) {
            return Err(format!(
                "an image is 1 to {IMAGE_SIDE} texels a side, not {width} × {height}"
            ));
        }
        let texels = u64::from(width) * u64::from(height);
        if c.texels + texels > IMAGE_TEXELS {
            return Err(format!(
                "a mod's images hold at most {IMAGE_TEXELS} texels together"
            ));
        }
        let (cw, ch) = (
            width.div_ceil(CELL) as usize,
            height.div_ceil(CELL) as usize,
        );
        let Some((x, y)) = c.page().alloc(cw, ch) else {
            return Err("the image page is full".into());
        };
        let id = c.next;
        c.next = c.next.wrapping_add(1).max(1);
        c.texels += texels;
        c.images.insert(
            id,
            Image {
                cells: (x, y, cw, ch),
                width,
                height,
                rgba: vec![0; (texels * 4) as usize],
                // Whatever a stopped mod left there goes: it starts clear.
                dirty: Some((0, 0, width, height)),
            },
        );
        Ok(id)
    }
    fn update_image(
        &mut self,
        image: ImageId,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        let started = Instant::now();
        let result = self.put_image(image, (x, y, width, height), rgba);
        self.canvas.charge(started);
        result
    }
    fn drop_image(&mut self, image: ImageId) {
        let c = &mut self.canvas;
        if let Some(img) = c.images.remove(&image) {
            c.texels -= u64::from(img.width) * u64::from(img.height);
            c.page().free(img.cells);
        }
    }
    fn set_corner(
        &mut self,
        corner: RenderCorner,
        width: f32,
        height: f32,
        shapes: Vec<CanvasShape>,
    ) {
        let size = |v: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1024.0)
            } else {
                0.0
            }
        };
        self.canvas.panel = Some(CanvasPanel {
            corner,
            placed: None,
            width: size(width),
            height: size(height),
            shapes: clip_canvas(shapes, SHAPES),
            items: Vec::new(),
        });
    }
    fn set_placed(
        &mut self,
        anchor: RenderCorner,
        dx: f32,
        dy: f32,
        width: f32,
        height: f32,
        shapes: Vec<CanvasShape>,
    ) {
        self.set_corner(anchor, width, height, shapes);
        self.canvas.panel.as_mut().expect("just set").placed = Some((finite(dx), finite(dy)));
    }
    fn open_canvas_screen(&mut self, title: String, shapes: Vec<CanvasShape>) {
        self.ui.screen = None;
        self.canvas.set_screen(Some(CanvasScreen {
            title: clip(title),
            shapes: clip_canvas(shapes, SCREEN_SHAPES),
            items: Vec::new(),
        }));
        self.ui.open = true;
        self.ui.close = false;
    }
    fn set_canvas_screen(&mut self, shapes: Vec<CanvasShape>) {
        if let Some(screen) = &mut self.canvas.screen {
            screen.shapes = clip_canvas(shapes, SCREEN_SHAPES);
        }
    }
    fn close_canvas_screen(&mut self) {
        if self.canvas.screen.is_some() {
            self.canvas.set_screen(None);
            self.ui.close = true;
            self.ui.open = false;
        }
    }
    fn clear_corner(&mut self) {
        self.canvas.panel = None;
    }
    fn request_frame_budget(&mut self, micros: u32) -> u32 {
        self.canvas.budget.requested = micros;
        self.canvas.refresh_budget();
        self.canvas.budget.granted
    }
    fn work_left(&mut self) -> u64 {
        // The linker's own `work-left` reads the store's fuel
        // (`Mods::engine`); this one serves only a store without it.
        0
    }
}

impl State {
    /// `update-image`'s work.
    fn put_image(
        &mut self,
        image: ImageId,
        (x, y, width, height): (u32, u32, u32, u32),
        rgba: Vec<u8>,
    ) -> Result<(), String> {
        let c = &mut self.canvas;
        let Some(img) = c.images.get_mut(&image) else {
            return Err(format!("no image {image}"));
        };
        let (x1, y1) = (x.checked_add(width), y.checked_add(height));
        if x1.is_none_or(|x1| x1 > img.width) || y1.is_none_or(|y1| y1 > img.height) {
            return Err(format!(
                "{width} × {height} at {x}, {y} is outside the {} × {} image",
                img.width, img.height
            ));
        }
        let want = u64::from(width) * u64::from(height) * 4;
        if rgba.len() as u64 != want {
            return Err(format!(
                "{width} × {height} texels are {want} bytes, not {}",
                rgba.len()
            ));
        }
        if c.uploaded + rgba.len() > UPLOAD_BYTES {
            return Err(format!(
                "over {} KiB of images updated this frame",
                UPLOAD_BYTES >> 10
            ));
        }
        c.uploaded += rgba.len();
        let row = (width * 4) as usize;
        for (i, line) in rgba.chunks_exact(row.max(1)).enumerate() {
            let at = (((y + i as u32) * img.width + x) * 4) as usize;
            img.rgba[at..at + row].copy_from_slice(line);
        }
        if width > 0 && height > 0 {
            img.touch(x, y, x + width, y + height);
        }
        Ok(())
    }
}

impl Mods {
    /// Limit open canvas screens to the normal 500 µs maximum under thermal
    /// pressure (for example iOS Serious/Critical). Requests are retained;
    /// clearing the limit restores them. Applies to current and future mods.
    pub fn limit_canvas_screen_budget(&mut self, limited: bool) {
        self.canvas_screen_budget_limited = limited;
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().canvas.limit_screen_budget(limited);
        }
    }
    /// The registered key activated by touching this running mod's HUD box.
    pub fn canvas_box_key(&self, owner: usize) -> Option<&str> {
        self.entries
            .get(owner)?
            .run
            .as_ref()?
            .store
            .data()
            .canvas
            .box_key
            .as_deref()
    }
    /// The running mods' canvas boxes, in load order, with the mod's index.
    pub fn canvas_panels(&self) -> impl Iterator<Item = (usize, &CanvasPanel)> {
        self.entries.iter().enumerate().filter_map(|(i, e)| {
            let panel = e.run.as_ref()?.store.data().canvas.panel.as_ref()?;
            Some((i, panel))
        })
    }
    /// The canvas screen mod `owner` last opened, until it closes.
    pub fn canvas_screen(&self, owner: usize) -> Option<&CanvasScreen> {
        self.entries
            .get(owner)?
            .run
            .as_ref()?
            .store
            .data()
            .canvas
            .screen
            .as_ref()
    }
    /// Where mod `owner`'s image is on the page; none for one it doesn't have.
    pub fn canvas_image(&self, owner: usize, image: u32) -> Option<ImageRect> {
        let run = self.entries.get(owner)?.run.as_ref()?;
        Some(run.store.data().canvas.images.get(&image)?.rect())
    }
    /// Some running mod imports `canvas`: the game keeps the page in its atlas.
    pub fn uses_canvas(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.run.as_ref().is_some_and(|r| r.canvas))
    }
    /// The texels to write into the page before the next frame (`all`: every
    /// image whole, after the page was made again). Call it once a frame:
    /// each mod's upload allowance starts over.
    pub fn take_canvas_uploads(&mut self, all: bool) -> Vec<CanvasUpload> {
        let mut out = Vec::new();
        for run in self.entries.iter_mut().filter_map(|e| e.run.as_mut()) {
            run.store.data_mut().canvas.take_uploads(all, &mut out);
        }
        out
    }
    /// The frame budget mod `i` has, in µs, while it runs.
    pub fn frame_budget(&self, i: usize) -> Option<u32> {
        let run = self.entries.get(i)?.run.as_ref()?;
        Some(run.store.data().canvas.budget.granted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canvas::Host;

    #[test]
    fn larger_requests_follow_only_the_owners_canvas_screen() {
        let mut s = super::super::entity_tests::state();
        let mut other = super::super::entity_tests::state();
        assert_eq!(s.request_frame_budget(0), 1);
        assert_eq!(s.request_frame_budget(3500), 500);
        assert_eq!(other.request_frame_budget(3500), 500);
        s.open_canvas_screen("Map".into(), vec![]);
        assert_eq!(s.canvas.budget.granted, 3500);
        assert_eq!(other.canvas.budget.granted, 500);
        assert_eq!(s.request_frame_budget(u32::MAX), 4000);
        s.close_canvas_screen();
        assert_eq!(s.canvas.budget.granted, 500);
        s.open_canvas_screen("Map".into(), vec![]);
        assert_eq!(s.canvas.budget.granted, 4000);
        // The same path Esc/Done uses, before notifying the guest.
        s.canvas.set_screen(None);
        assert_eq!(s.canvas.budget.granted, 500);
        s.open_canvas_screen("Map".into(), vec![]);
        super::super::ui::Host::close_screen(&mut s);
        assert_eq!(s.canvas.budget.granted, 500);
        s.open_canvas_screen("Map".into(), vec![]);
        super::super::ui::Host::open_screen(
            &mut s,
            super::super::ui::Screen {
                title: "Widgets".into(),
                widgets: vec![],
            },
        );
        assert_eq!(s.canvas.budget.granted, 500);
        s.open_canvas_screen("Map".into(), vec![]);
        assert_eq!(s.request_frame_budget(300), 300);
        s.close_canvas_screen();
        s.open_canvas_screen("Map".into(), vec![]);
        assert_eq!(s.canvas.budget.granted, 300);
    }

    #[test]
    fn thermal_limit_retains_the_screen_request() {
        let mut s = super::super::entity_tests::state();
        s.canvas.limit_screen_budget(true);
        s.open_canvas_screen("Map".into(), vec![]);
        assert_eq!(s.request_frame_budget(4000), 500);
        s.canvas.limit_screen_budget(false);
        assert_eq!(s.canvas.budget.granted, 4000);
        s.canvas.limit_screen_budget(true);
        assert_eq!(s.canvas.budget.granted, 500);
        s.close_canvas_screen();
        s.canvas.limit_screen_budget(false);
        assert_eq!(s.canvas.budget.granted, 500);
        s.open_canvas_screen("Map".into(), vec![]);
        assert_eq!(s.canvas.budget.granted, 4000);
    }

    #[test]
    fn screen_work_leaves_fuel_to_return_before_the_hard_cap() {
        let mut s = super::super::entity_tests::state();
        s.open_canvas_screen("Map".into(), vec![]);
        s.request_frame_budget(4000);
        // 5000 fuel/µs would buy 20M fuel, beyond the frame hook's 10M.
        s.canvas.budget.observe(500_000, Duration::from_micros(100));
        let cap = super::super::FUEL_FRAME;
        let soft = s.canvas.budget.allowance(cap);
        assert_eq!(soft, 9_950_000);
        let work = Work {
            start: cap,
            soft,
            ..Work::default()
        };
        assert_eq!(work_left(work, 50_000), 0);
        // Even a hard cap below the floor must win.
        assert!(s.canvas.budget.allowance(1000) <= 1000);
    }

    #[test]
    fn the_page_hands_out_free_cells_and_takes_them_back() {
        let mut page = Page::default();
        // A 1024 × 1024 image is 64 × 64 cells: four fill the page.
        let quarters: Vec<_> = (0..4).map(|_| page.alloc(64, 64)).collect();
        assert_eq!(
            quarters,
            [Some((0, 0)), Some((64, 0)), Some((0, 64)), Some((64, 64))]
        );
        assert_eq!(page.alloc(1, 1), None);
        page.free((64, 0, 64, 64));
        assert_eq!(page.alloc(1, 1), Some((64, 0)));
        assert_eq!(page.alloc(64, 64), None);
        assert_eq!(page.alloc(63, 64), Some((65, 0)));
        assert_eq!(page.alloc(1, 63), Some((64, 1)));
        assert_eq!(page.alloc(1, 1), None);
        assert_eq!(page.alloc(0, 4), None);
        assert_eq!(page.alloc(129, 1), None);
        assert_eq!(bits(0, 128), u128::MAX);
    }

    #[test]
    fn frame_times_report_a_window_then_start_again() {
        let mut t = FrameTimes::default();
        let us = Duration::from_micros;
        assert_eq!(t.record(us(100), 4), None);
        assert_eq!(t.record(us(350), 4), None);
        assert_eq!(t.record(us(650), 4), None);
        assert_eq!(
            t.record(us(100), 4).as_deref(),
            Some("4 calls: avg 300 µs, max 650 µs, 2 over 300 µs, 1 over 500 µs")
        );
        assert_eq!(
            t.record(us(20), 1).as_deref(),
            Some("1 calls: avg 20 µs, max 20 µs, 0 over 300 µs, 0 over 500 µs")
        );
    }

    #[test]
    fn work_left_counts_down_the_soft_budget() {
        let work = Work {
            start: 10_000_000,
            soft: 200_000,
            ..Work::default()
        };
        assert_eq!(work_left(work, 10_000_000), WORK_SCALE);
        assert_eq!(work_left(work, 9_900_000), WORK_SCALE / 2);
        assert_eq!(work_left(work, 9_700_000), 0);
        assert_eq!(work_left(Work::default(), 5), 0);
        // Host calls' time counts as the fuel it would have bought.
        let charged = Work {
            host: 50_000,
            ..work
        };
        assert_eq!(work_left(charged, 10_000_000), WORK_SCALE * 3 / 4);
        assert_eq!(work_left(charged, 9_850_000), 0);
    }

    #[test]
    fn the_budget_follows_the_measured_rate() {
        let mut b = Budget {
            granted: 300,
            ..Budget::default()
        };
        let cap = 10_000_000;
        // Before any frame: the low first guess, at least the floor.
        let first = ((FIRST_RATE * 300.0) as u64).max(SOFT_MIN);
        assert_eq!(b.allowance(cap), first);
        // Too small a call to say anything.
        b.observe(1_000, Duration::from_micros(500));
        assert_eq!(b.allowance(cap), first);
        // 400 fuel a µs: 120 000 fuel for 300 µs, after one frame.
        b.observe(40_000, Duration::from_micros(100));
        assert_eq!(b.allowance(cap), 120_000);
        // A slower device moves it a quarter of the way each frame.
        b.observe(20_000, Duration::from_micros(100));
        assert_eq!(b.allowance(cap), 105_000);
        // Never past the hook's own fuel, nor under the floor.
        b.granted = 500;
        b.observe(u64::MAX / 4, Duration::from_micros(1));
        assert_eq!(b.allowance(cap), cap - SOFT_RESERVE);
        let slow = Budget {
            granted: 1,
            rate: Some(1.0),
            ..Budget::default()
        };
        assert_eq!(slow.allowance(cap), SOFT_MIN);
    }
}
