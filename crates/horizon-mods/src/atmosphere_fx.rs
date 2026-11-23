//! Capability `atmosphere`'s particle effects (API level 33) without the
//! runtime: the checks on a mod's effect, curve sampling, the emitter's spawn
//! clock and shapes, and one particle's motion and look. The game runs these
//! host-side through its particle engine (`game/mod_particles.rs`); the mod
//! only defines effects and starts, moves and stops emitters
//! (`docs/api-expansion-d.md`). Everything is deterministic for a seed.

/// Effects one mod may define.
pub const EFFECTS_PER_MOD: usize = 256;
/// Live emitters per mod.
pub const EMITTERS_PER_MOD: usize = 48;
/// Live particles per mod (inside the engine's own 16384).
pub const PARTICLES_PER_MOD: usize = 3072;
/// Particles one mod may spawn per particle tick (20 per second).
pub const SPAWNS_PER_TICK: usize = 512;
/// One emitter's `max` and `burst`.
pub const EMITTER_MAX: u32 = 512;
/// Keys in one curve or colour list.
pub const CURVE_KEYS: usize = 8;
/// The farthest a particle may spawn from the camera, in blocks.
pub const SPAWN_RADIUS: f64 = 64.0;
/// The largest emitter density.
pub const DENSITY_MAX: f32 = 4.0;
/// Blocks: offsets, sizes, radii, speeds and accelerations stay within this.
const BLOCKS: f32 = 64.0;

/// A value range drawn uniformly (WIT `span`).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Span {
    pub lo: f32,
    pub hi: f32,
}
impl Span {
    pub const fn new(lo: f32, hi: f32) -> Self {
        Self { lo, hi }
    }
    pub const fn constant(v: f32) -> Self {
        Self { lo: v, hi: v }
    }
    pub fn draw(self, rng: &mut Rng) -> f32 {
        self.lo + (self.hi - self.lo) * rng.unit()
    }
    fn check(self, what: &str, min: f32, max: f32) -> Result<(), String> {
        if !(self.lo.is_finite() && self.hi.is_finite()) {
            return Err(format!("{what} is not finite"));
        }
        if self.lo > self.hi {
            return Err(format!("{what}: lo {} is above hi {}", self.lo, self.hi));
        }
        if self.lo < min || self.hi > max {
            return Err(format!("{what} must be within {min}..{max}"));
        }
        Ok(())
    }
}

/// `amplitude * sin(2 pi (age / period + phase))` (WIT `wave`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    pub amplitude: Span,
    pub period: Span,
    pub phase: Span,
}
impl Wave {
    fn check(&self, what: &str, amplitude: f32) -> Result<(), String> {
        self.amplitude
            .check(&format!("{what} wave amplitude"), -amplitude, amplitude)?;
        self.period
            .check(&format!("{what} wave period"), 0.01, 600.0)?;
        self.phase
            .check(&format!("{what} wave phase"), -1000.0, 1000.0)
    }
    fn draw(&self, rng: &mut Rng) -> DrawnWave {
        DrawnWave {
            amplitude: self.amplitude.draw(rng),
            period: self.period.draw(rng),
            phase: self.phase.draw(rng),
        }
    }
}

/// One particle's wave.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawnWave {
    pub amplitude: f32,
    pub period: f32,
    pub phase: f32,
}
impl DrawnWave {
    pub fn at(self, age: f32) -> f32 {
        if self.amplitude == 0.0 || self.period <= 0.0 {
            return 0.0;
        }
        self.amplitude * (std::f32::consts::TAU * (age / self.period + self.phase)).sin()
    }
}

/// How a curve eases between keys (WIT `ease`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ease {
    #[default]
    Linear,
    /// Smoothstep between the two keys.
    Smooth,
    /// Holds a key's value until the next.
    Step,
}

/// `base * keys(life) + wave(age)` (WIT `curve`). Keys are `(t, value)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Curve {
    pub base: Span,
    pub keys: Vec<(f32, f32)>,
    pub ease: Ease,
    pub wave: Option<Wave>,
}
impl Curve {
    /// A constant value.
    pub fn constant(v: f32) -> Self {
        Self {
            base: Span::constant(v),
            ..Self::default()
        }
    }
    /// The keys' value at life fraction `t`: 1 with no keys, the first key's
    /// before it and the last key's after it.
    pub fn shape(&self, t: f32) -> f32 {
        let keys = &self.keys;
        let Some(&(t0, v0)) = keys.first() else {
            return 1.0;
        };
        if t <= t0 {
            return v0;
        }
        for pair in keys.windows(2) {
            let ((ta, va), (tb, vb)) = (pair[0], pair[1]);
            if t < tb {
                let f = if tb > ta { (t - ta) / (tb - ta) } else { 1.0 };
                let f = match self.ease {
                    Ease::Linear => f,
                    Ease::Smooth => f * f * (3.0 - 2.0 * f),
                    Ease::Step => 0.0,
                };
                return va + (vb - va) * f;
            }
        }
        keys[keys.len() - 1].1
    }
    /// Checks and sorts the keys; `base` within `min..max`, waves and key
    /// values within `limit` either way.
    fn checked(mut self, what: &str, min: f32, max: f32, limit: f32) -> Result<Self, String> {
        self.base.check(what, min, max)?;
        if self.keys.len() > CURVE_KEYS {
            return Err(format!("{what}: at most {CURVE_KEYS} keys"));
        }
        for &(t, v) in &self.keys {
            if !(0.0..=1.0).contains(&t) || !v.is_finite() || v.abs() > limit {
                return Err(format!(
                    "{what}: key t must be 0..1 and its value finite within -{limit}..{limit}"
                ));
            }
        }
        self.keys.sort_by(|a, b| a.0.total_cmp(&b.0));
        if let Some(wave) = &self.wave {
            wave.check(what, limit)?;
        }
        Ok(self)
    }
    fn draw(&self, rng: &mut Rng) -> DrawnCurve {
        DrawnCurve {
            base: self.base.draw(rng),
            wave: self.wave.map(|w| w.draw(rng)).unwrap_or_default(),
        }
    }
}

/// One particle's draw of a curve.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawnCurve {
    pub base: f32,
    pub wave: DrawnWave,
}
impl DrawnCurve {
    pub fn at(self, curve: &Curve, t: f32, age: f32) -> f32 {
        self.base * curve.shape(t) + self.wave.at(age)
    }
}

/// Where an emitter spawns (WIT `emitter-shape`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    #[default]
    Point,
    Box,
    Disc,
    Sphere,
}

/// How an effect spawns (WIT `emitter`).
#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    pub looping: bool,
    pub duration: f32,
    pub rate: f32,
    pub density_scaled: bool,
    pub max: u32,
    pub burst: u32,
    pub shape: Shape,
    pub offset: [Span; 3],
    pub half_size: [f32; 3],
    pub radius: Span,
    pub surface: bool,
    pub direction: Option<[Span; 3]>,
}
impl Default for Emitter {
    fn default() -> Self {
        Self {
            looping: true,
            duration: 1.0,
            rate: 0.0,
            density_scaled: false,
            max: EMITTER_MAX,
            burst: 0,
            shape: Shape::Point,
            offset: [Span::default(); 3],
            half_size: [0.0; 3],
            radius: Span::default(),
            surface: false,
            direction: None,
        }
    }
}

/// Where a particle may live (WIT `medium`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Medium {
    #[default]
    Anywhere,
    Air,
    Water,
}

/// Which sprite a particle shows (WIT `frame-mode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Frames {
    #[default]
    First,
    Random,
    OverLife,
    Loop,
}

/// How each particle moves and looks (WIT `particle`).
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleSpec {
    pub sprite: String,
    pub frame: Frames,
    pub frame_rate: f32,
    pub lifetime: Span,
    pub speed: Span,
    pub velocity: [Span; 3],
    pub drag: f32,
    pub gravity: f32,
    pub acceleration: Option<[Curve; 3]>,
    pub size: Curve,
    pub alpha: Curve,
    pub color: u32,
    /// `(t, 0xRRGGBB)`.
    pub color_keys: Vec<(f32, u32)>,
    pub shade: f32,
    pub spin: Span,
    pub spin_rate: Span,
    pub fade_near: Option<Span>,
    pub fade_far: Option<Span>,
    pub emissive: bool,
    pub translucent: bool,
    pub only_in: Medium,
    pub die_nearer_than: f32,
}
impl Default for ParticleSpec {
    fn default() -> Self {
        Self {
            sprite: String::new(),
            frame: Frames::First,
            frame_rate: 0.0,
            lifetime: Span::constant(1.0),
            speed: Span::default(),
            velocity: [Span::default(); 3],
            drag: 0.0,
            gravity: 0.0,
            acceleration: None,
            size: Curve::constant(0.1),
            alpha: Curve::constant(1.0),
            color: 0xFFFFFF,
            color_keys: Vec::new(),
            shade: 0.0,
            spin: Span::default(),
            spin_rate: Span::default(),
            fade_near: None,
            fade_far: None,
            emissive: false,
            translucent: false,
            only_in: Medium::Anywhere,
            die_nearer_than: 0.0,
        }
    }
}

/// A checked effect (WIT `effect`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Effect {
    pub emitter: Emitter,
    pub particle: ParticleSpec,
}

fn finite_within(what: &str, v: f32, min: f32, max: f32) -> Result<(), String> {
    if !v.is_finite() || v < min || v > max {
        return Err(format!("{what} must be finite within {min}..{max}"));
    }
    Ok(())
}
fn check_color(what: &str, c: u32) -> Result<(), String> {
    if c > 0xFF_FFFF {
        return Err(format!("{what} {c:#x} is not 0xRRGGBB"));
    }
    Ok(())
}
fn check3(what: &str, spans: &[Span; 3], min: f32, max: f32) -> Result<(), String> {
    for (axis, s) in ["x", "y", "z"].iter().zip(spans) {
        s.check(&format!("{what} {axis}"), min, max)?;
    }
    Ok(())
}

/// The namespace of a sprite set name `<namespace>:<name>`, when well formed
/// (vanilla's resource-location characters).
pub fn sprite_namespace(name: &str) -> Option<&str> {
    let (ns, path) = name.split_once(':')?;
    let ok = |s: &str, extra: &[u8]| {
        !s.is_empty()
            && s.len() <= 128
            && s.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b"_.-".contains(&b)
                    || extra.contains(&b)
            })
    };
    (ok(ns, b"") && ok(path, b"/")).then_some(ns)
}

impl Effect {
    /// Checks every field and sorts the curves' keys; the error names the
    /// first field out of range.
    pub fn checked(mut self) -> Result<Self, String> {
        let e = &self.emitter;
        finite_within("emitter duration", e.duration, 0.05, 600.0)?;
        finite_within("emitter rate", e.rate, 0.0, 2048.0)?;
        if !(1..=EMITTER_MAX).contains(&e.max) {
            return Err(format!("emitter max must be 1..={EMITTER_MAX}"));
        }
        if e.burst > EMITTER_MAX {
            return Err(format!("emitter burst must be 0..={EMITTER_MAX}"));
        }
        check3("emitter offset", &e.offset, -BLOCKS, BLOCKS)?;
        for v in e.half_size {
            finite_within("emitter half-size", v, 0.0, BLOCKS)?;
        }
        e.radius.check("emitter radius", 0.0, BLOCKS)?;
        if let Some(d) = &e.direction {
            check3("emitter direction", d, -BLOCKS, BLOCKS)?;
        }
        let p = &mut self.particle;
        if sprite_namespace(&p.sprite).is_none() {
            return Err(format!(
                "particle sprite {:?} is not <namespace>:<name>",
                p.sprite
            ));
        }
        finite_within("particle frame-rate", p.frame_rate, 0.0, 60.0)?;
        p.lifetime.check("particle lifetime", 0.05, 60.0)?;
        p.speed.check("particle speed", -BLOCKS, BLOCKS)?;
        check3("particle velocity", &p.velocity, -BLOCKS, BLOCKS)?;
        finite_within("particle drag", p.drag, 0.0, 20.0)?;
        finite_within("particle gravity", p.gravity, -40.0, 40.0)?;
        if let Some(acceleration) = p.acceleration.take() {
            let [x, y, z] = acceleration;
            p.acceleration = Some([
                x.checked("particle acceleration x", -BLOCKS, BLOCKS, BLOCKS)?,
                y.checked("particle acceleration y", -BLOCKS, BLOCKS, BLOCKS)?,
                z.checked("particle acceleration z", -BLOCKS, BLOCKS, BLOCKS)?,
            ]);
        }
        p.size = std::mem::take(&mut p.size).checked("particle size", 0.0, 16.0, 16.0)?;
        p.alpha = std::mem::take(&mut p.alpha).checked("particle alpha", 0.0, 1.0, 16.0)?;
        check_color("particle color", p.color)?;
        if p.color_keys.len() > CURVE_KEYS {
            return Err(format!("particle color-keys: at most {CURVE_KEYS}"));
        }
        for &(t, c) in &p.color_keys {
            if !(0.0..=1.0).contains(&t) {
                return Err("particle color-keys: t must be 0..1".into());
            }
            check_color("particle color-key", c)?;
        }
        p.color_keys.sort_by(|a, b| a.0.total_cmp(&b.0));
        finite_within("particle shade", p.shade, 0.0, 1.0)?;
        p.spin.check("particle spin", -3600.0, 3600.0)?;
        p.spin_rate.check("particle spin-rate", -3600.0, 3600.0)?;
        if let Some(f) = p.fade_near {
            f.check("particle fade-near", 0.0, 128.0)?;
        }
        if let Some(f) = p.fade_far {
            f.check("particle fade-far", 0.0, 128.0)?;
        }
        finite_within("particle die-nearer-than", p.die_nearer_than, 0.0, 16.0)?;
        Ok(self)
    }
}

/// Where an emitter is (WIT `anchor`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// Follows the camera, on world axes.
    Camera,
    World([f64; 3]),
}
impl Anchor {
    /// Checks a world point is finite.
    pub fn checked(self) -> Result<Self, String> {
        match self {
            Anchor::World(p) if !p.iter().all(|c| c.is_finite() && c.abs() < 3.0e7) => {
                Err("emitter anchor must be a finite point inside the world".into())
            }
            a => Ok(a),
        }
    }
    /// The point, for a camera at `camera`.
    pub fn at(self, camera: [f64; 3]) -> [f64; 3] {
        match self {
            Anchor::Camera => camera,
            Anchor::World(p) => p,
        }
    }
}

/// A small deterministic generator (SplitMix64).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    /// Uniform in 0..1.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// An emitter's spawn clock: how many particles each step spawns.
#[derive(Clone, Debug)]
pub struct EmitterClock {
    /// Seconds since the start.
    pub age: f32,
    carry: f32,
    started: bool,
}
impl Default for EmitterClock {
    fn default() -> Self {
        Self::new()
    }
}
impl EmitterClock {
    pub fn new() -> Self {
        Self {
            age: 0.0,
            carry: 0.0,
            started: false,
        }
    }
    /// Moves the clock on by `dt` seconds with the rate multiplied by
    /// `rate_scale`: the particles due (the burst on the first step) and
    /// whether a one-shot emitter has ended.
    pub fn step(&mut self, e: &Emitter, dt: f32, rate_scale: f32) -> (u32, bool) {
        let mut due = 0;
        if !self.started {
            self.started = true;
            due += e.burst;
        }
        // Float steps of 0.05 s drift by ulps: within 1e-4 counts as whole.
        let remaining = e.duration - self.age;
        let active = if e.looping || remaining >= dt - 1e-4 {
            dt
        } else if remaining <= 1e-4 {
            0.0
        } else {
            remaining
        };
        self.age += dt;
        self.carry += e.rate * rate_scale.max(0.0) * active;
        let whole = (self.carry + 1e-4).floor();
        self.carry -= whole;
        due = due.saturating_add(whole as u32);
        // A hair early, so float steps of 0.05 s end a 0.5 s emitter on its tenth.
        (due, !e.looping && self.age >= e.duration - 1e-4)
    }
}

/// Where a spawned particle starts and its first velocity, relative to the
/// anchor; blocks and blocks per second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spawn {
    pub offset: [f64; 3],
    pub velocity: [f32; 3],
}

fn normalize(v: [f32; 3]) -> Option<[f32; 3]> {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    (len > 1e-6 && len.is_finite()).then(|| v.map(|c| c / len))
}
fn random_direction(rng: &mut Rng) -> [f32; 3] {
    // Uniform on the sphere: z uniform in -1..1, angle uniform.
    let z = rng.unit() * 2.0 - 1.0;
    let a = rng.unit() * std::f32::consts::TAU;
    let r = (1.0 - z * z).max(0.0).sqrt();
    [r * a.cos(), z, r * a.sin()]
}

/// Draws one spawn of `e`.
pub fn spawn(effect: &Effect, rng: &mut Rng) -> Spawn {
    let e = &effect.emitter;
    let o = e.offset.map(|s| s.draw(rng));
    let local: [f32; 3] = match e.shape {
        Shape::Point => [0.0; 3],
        Shape::Box => {
            let mut p = e.half_size.map(|h| (rng.unit() * 2.0 - 1.0) * h);
            if e.surface {
                let axis = (rng.unit() * 3.0).min(2.0) as usize;
                p[axis] = if rng.unit() < 0.5 {
                    -e.half_size[axis]
                } else {
                    e.half_size[axis]
                };
            }
            p
        }
        Shape::Disc => {
            let r = e.radius.draw(rng);
            let r = if e.surface { r } else { r * rng.unit().sqrt() };
            let a = rng.unit() * std::f32::consts::TAU;
            [r * a.cos(), 0.0, r * a.sin()]
        }
        Shape::Sphere => {
            let r = e.radius.draw(rng);
            let r = if e.surface { r } else { r * rng.unit().cbrt() };
            random_direction(rng).map(|c| c * r)
        }
    };
    let direction = match &e.direction {
        Some(d) => normalize(d.map(|s| s.draw(rng))).unwrap_or([0.0; 3]),
        None => normalize(local).unwrap_or_else(|| random_direction(rng)),
    };
    let p = &effect.particle;
    let speed = p.speed.draw(rng);
    let extra = p.velocity.map(|s| s.draw(rng));
    Spawn {
        offset: [0, 1, 2].map(|i| f64::from(o[i] + local[i])),
        velocity: [0, 1, 2].map(|i| direction[i] * speed + extra[i]),
    }
}

/// One live particle's own state: its draws, age and velocity.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleSim {
    /// Seconds lived and to live.
    pub age: f32,
    pub life: f32,
    pub velocity: [f32; 3],
    size: DrawnCurve,
    alpha: DrawnCurve,
    acceleration: [DrawnCurve; 3],
    shade: f32,
    spin: f32,
    spin_rate: f32,
    /// A random sprite pick in 0..1.
    pick: f32,
}

/// What a particle shows this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// Quad size in blocks.
    pub size: f32,
    /// Linear 0..1 channels, tint applied; alpha after the camera fades.
    pub rgba: [f32; 4],
    /// Radians.
    pub roll: f32,
    /// Index in a set of `frames` sprites.
    pub frame: usize,
}

fn channel(c: u32, shift: u32) -> f32 {
    ((c >> shift) & 0xFF) as f32 / 255.0
}
fn rgb(c: u32) -> [f32; 3] {
    [channel(c, 16), channel(c, 8), channel(c, 0)]
}
/// 0 at `lo`, 1 at `hi`, smooth between (a step at `lo` when they meet).
fn ramp(x: f32, s: Span) -> f32 {
    if s.hi <= s.lo {
        return if x >= s.lo { 1.0 } else { 0.0 };
    }
    let f = ((x - s.lo) / (s.hi - s.lo)).clamp(0.0, 1.0);
    f * f * (3.0 - 2.0 * f)
}

impl ParticleSim {
    /// A new particle of `effect` with its start velocity.
    pub fn new(effect: &Effect, velocity: [f32; 3], rng: &mut Rng) -> Self {
        let p = &effect.particle;
        let zero = DrawnCurve::default();
        let acceleration = p.acceleration.as_ref().map_or([zero; 3], |a| {
            [a[0].draw(rng), a[1].draw(rng), a[2].draw(rng)]
        });
        Self {
            age: 0.0,
            life: p.lifetime.draw(rng).max(0.05),
            velocity,
            size: p.size.draw(rng),
            alpha: p.alpha.draw(rng),
            acceleration,
            shade: p.shade * rng.unit(),
            spin: p.spin.draw(rng),
            spin_rate: p.spin_rate.draw(rng),
            pick: rng.unit(),
        }
    }
    /// Life fraction 0..1.
    pub fn t(&self) -> f32 {
        (self.age / self.life).clamp(0.0, 1.0)
    }
    /// Moves on by `dt` seconds: the displacement, or none once it expires.
    pub fn step(&mut self, effect: &Effect, dt: f32) -> Option<[f64; 3]> {
        if self.age >= self.life - 1e-4 {
            return None;
        }
        let p = &effect.particle;
        let t = self.t();
        let mut a = [0.0f32; 3];
        if let Some(curves) = &p.acceleration {
            for i in 0..3 {
                a[i] = self.acceleration[i].at(&curves[i], t, self.age);
            }
        }
        a[1] -= p.gravity;
        let keep = (-p.drag * dt).exp();
        let mut d = [0.0f64; 3];
        for i in 0..3 {
            self.velocity[i] = (self.velocity[i] + a[i] * dt) * keep;
            d[i] = f64::from(self.velocity[i] * dt);
        }
        self.age += dt;
        Some(d)
    }
    /// What it shows `camera_distance` blocks from the camera, tinted by
    /// `tint` (0xRRGGBB), from a set of `frames` sprites.
    pub fn look(&self, effect: &Effect, camera_distance: f32, tint: u32, frames: usize) -> Look {
        let p = &effect.particle;
        let t = self.t();
        let size = self.size.at(&p.size, t, self.age).clamp(0.0, 16.0);
        let mut alpha = self.alpha.at(&p.alpha, t, self.age).clamp(0.0, 1.0);
        if let Some(near) = p.fade_near {
            alpha *= ramp(camera_distance, near);
        }
        if let Some(far) = p.fade_far {
            alpha *= 1.0 - ramp(camera_distance, far);
        }
        let base = match p.color_keys.as_slice() {
            [] => rgb(p.color),
            [(_, c)] => rgb(*c),
            keys => {
                let mut out = rgb(keys[keys.len() - 1].1);
                if t <= keys[0].0 {
                    out = rgb(keys[0].1);
                } else {
                    for pair in keys.windows(2) {
                        let ((ta, ca), (tb, cb)) = (pair[0], pair[1]);
                        if t < tb {
                            let f = if tb > ta { (t - ta) / (tb - ta) } else { 1.0 };
                            let (a, b) = (rgb(ca), rgb(cb));
                            out = [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * f);
                            break;
                        }
                    }
                }
                out
            }
        };
        let tint = rgb(tint);
        let dark = 1.0 - self.shade;
        let frames = frames.max(1);
        let frame = match p.frame {
            Frames::First => 0,
            Frames::Random => (self.pick * frames as f32) as usize,
            Frames::OverLife => (t * frames as f32) as usize,
            Frames::Loop => (self.age * p.frame_rate) as usize % frames,
        }
        .min(frames - 1);
        Look {
            size,
            rgba: [
                base[0] * tint[0] * dark,
                base[1] * tint[1] * dark,
                base[2] * tint[2] * dark,
                alpha,
            ],
            roll: (self.spin + self.spin_rate * self.age).to_radians(),
            frame,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(keys: &[(f32, f32)], ease: Ease) -> Curve {
        Curve {
            base: Span::constant(1.0),
            keys: keys.to_vec(),
            ease,
            wave: None,
        }
    }
    fn effect() -> Effect {
        Effect {
            emitter: Emitter {
                rate: 10.0,
                ..Emitter::default()
            },
            particle: ParticleSpec {
                sprite: "demo:mote".into(),
                ..ParticleSpec::default()
            },
        }
    }

    #[test]
    fn curves_ease_between_sorted_keys_and_hold_past_the_ends() {
        let fade = [(0.0, 0.0), (0.25, 1.0), (0.75, 1.0), (1.0, 0.0)];
        let linear = curve(&fade, Ease::Linear);
        assert_eq!(linear.shape(0.125), 0.5);
        assert_eq!(linear.shape(0.5), 1.0);
        assert_eq!(linear.shape(0.875), 0.5);
        assert_eq!(linear.shape(1.0), 0.0);
        // Smoothstep at a quarter of the way: 3/16 - 2/64 = 0.15625.
        let smooth = curve(&fade, Ease::Smooth);
        assert!((smooth.shape(0.0625) - 0.15625).abs() < 1e-6);
        assert_eq!(smooth.shape(0.125), 0.5);
        let step = curve(&[(0.0, 2.0), (0.5, 4.0)], Ease::Step);
        assert_eq!(
            (step.shape(0.49), step.shape(0.5), step.shape(1.0)),
            (2.0, 4.0, 4.0)
        );
        assert_eq!(curve(&[(0.5, 3.0)], Ease::Linear).shape(0.0), 3.0);
        assert_eq!(curve(&[], Ease::Linear).shape(0.3), 1.0);
        // Keys arrive in any order; the check sorts them.
        let mut e = effect();
        e.particle.size = Curve {
            base: Span::constant(2.0),
            keys: vec![(1.0, 0.0), (0.0, 0.0), (0.5, 1.0)],
            ..Curve::default()
        };
        let e = e.checked().unwrap();
        assert_eq!(e.particle.size.keys, [(0.0, 0.0), (0.5, 1.0), (1.0, 0.0)]);
        assert_eq!(e.particle.size.shape(0.25), 0.5);
    }

    #[test]
    fn waves_add_a_drawn_sine_by_age() {
        let w = DrawnWave {
            amplitude: 0.5,
            period: 2.0,
            phase: 0.25,
        };
        assert!((w.at(0.0) - 0.5).abs() < 1e-6);
        assert!((w.at(1.0) + 0.5).abs() < 1e-6);
        let c = Curve {
            base: Span::constant(0.75),
            wave: Some(Wave {
                amplitude: Span::constant(0.25),
                period: Span::constant(1.0),
                phase: Span::constant(0.25),
            }),
            ..Curve::default()
        };
        let drawn = c.draw(&mut Rng::new(1));
        assert!((drawn.at(&c, 0.0, 0.0) - 1.0).abs() < 1e-6);
        assert!((drawn.at(&c, 0.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn effects_out_of_range_are_refused_with_the_field_named() {
        let cases: Vec<(fn(&mut Effect), &str)> = vec![
            (|e| e.emitter.rate = 5000.0, "rate"),
            (|e| e.emitter.rate = f32::NAN, "rate"),
            (|e| e.emitter.max = 0, "max"),
            (|e| e.emitter.max = 513, "max"),
            (|e| e.emitter.burst = 600, "burst"),
            (|e| e.emitter.duration = 0.0, "duration"),
            (|e| e.emitter.offset[1] = Span::new(0.0, 80.0), "offset y"),
            (|e| e.emitter.radius = Span::new(2.0, 1.0), "radius"),
            (
                |e| e.emitter.half_size = [1.0, f32::INFINITY, 1.0],
                "half-size",
            ),
            (|e| e.particle.sprite = "nonamespace".into(), "sprite"),
            (|e| e.particle.sprite = "Demo:Mote".into(), "sprite"),
            (|e| e.particle.lifetime = Span::new(0.0, 1.0), "lifetime"),
            (|e| e.particle.drag = -1.0, "drag"),
            (|e| e.particle.size.base = Span::constant(17.0), "size"),
            (|e| e.particle.alpha.base = Span::constant(1.5), "alpha"),
            (|e| e.particle.size.keys = vec![(0.0, 1.0); 9], "at most 8"),
            (|e| e.particle.alpha.keys = vec![(1.5, 1.0)], "key t"),
            (|e| e.particle.color = 0x1000000, "color"),
            (
                |e| e.particle.color_keys = vec![(0.0, 0xFFFFFF); 9],
                "color-keys",
            ),
            (|e| e.particle.shade = 2.0, "shade"),
            (
                |e| e.particle.fade_near = Some(Span::new(0.0, 200.0)),
                "fade-near",
            ),
            (|e| e.particle.die_nearer_than = 20.0, "die-nearer-than"),
            (
                |e| {
                    e.particle.alpha.wave = Some(Wave {
                        amplitude: Span::constant(0.1),
                        period: Span::constant(0.0),
                        phase: Span::default(),
                    })
                },
                "period",
            ),
            (
                |e| {
                    let mut c = Curve::constant(0.0);
                    c.base = Span::new(f32::NAN, 0.0);
                    e.particle.acceleration = Some([Curve::constant(0.0), c, Curve::constant(0.0)]);
                },
                "acceleration y",
            ),
        ];
        assert!(effect().checked().is_ok());
        for (change, field) in cases {
            let mut e = effect();
            change(&mut e);
            let error = e.checked().unwrap_err();
            assert!(error.contains(field), "{field}: {error}");
        }
    }

    #[test]
    fn the_clock_spawns_the_rate_over_time_bursts_once_and_ends_one_shots() {
        let mut e = Emitter {
            rate: 10.0,
            burst: 5,
            ..Emitter::default()
        };
        let mut clock = EmitterClock::new();
        let total: u32 = (0..20).map(|_| clock.step(&e, 0.05, 1.0).0).sum();
        assert_eq!(total, 5 + 10);
        // Half density over two seconds.
        let mut clock = EmitterClock::new();
        let total: u32 = (0..40).map(|_| clock.step(&e, 0.05, 0.5).0).sum();
        assert_eq!(total, 5 + 10);
        e.looping = false;
        e.duration = 0.5;
        e.burst = 0;
        let mut clock = EmitterClock::new();
        let steps: Vec<(u32, bool)> = (0..12).map(|_| clock.step(&e, 0.05, 1.0)).collect();
        assert_eq!(steps.iter().map(|s| s.0).sum::<u32>(), 5);
        let ended = steps.iter().position(|s| s.1).unwrap();
        assert_eq!(ended, 9, "{steps:?}");
    }

    #[test]
    fn spawns_stay_in_their_shape_and_repeat_for_a_seed() {
        let mut e = effect();
        e.emitter.offset = [
            Span::constant(0.0),
            Span::constant(5.0),
            Span::constant(0.0),
        ];
        e.emitter.shape = Shape::Box;
        e.emitter.half_size = [2.0, 0.0, 3.0];
        e.particle.speed = Span::constant(2.0);
        e.emitter.direction = Some([
            Span::constant(0.0),
            Span::constant(-4.0),
            Span::constant(0.0),
        ]);
        let mut rng = Rng::new(7);
        for _ in 0..200 {
            let s = spawn(&e, &mut rng);
            assert!(s.offset[0].abs() <= 2.0 && s.offset[1] == 5.0 && s.offset[2].abs() <= 3.0);
            assert_eq!(s.velocity, [0.0, -2.0, 0.0]);
        }
        e.emitter.shape = Shape::Disc;
        e.emitter.radius = Span::constant(4.0);
        e.emitter.surface = true;
        e.emitter.direction = None;
        for _ in 0..200 {
            let s = spawn(&e, &mut rng);
            let r = (s.offset[0].powi(2) + s.offset[2].powi(2)).sqrt();
            assert!((r - 4.0).abs() < 1e-4 && s.offset[1] == 5.0, "{s:?}");
            // Outwards along the rim, at the speed.
            let v = s.velocity;
            assert!(v[1] == 0.0 && ((v[0] * v[0] + v[2] * v[2]).sqrt() - 2.0).abs() < 1e-4);
        }
        e.emitter.shape = Shape::Sphere;
        e.emitter.surface = false;
        for _ in 0..200 {
            let s = spawn(&e, &mut rng);
            let d =
                (s.offset[0].powi(2) + (s.offset[1] - 5.0).powi(2) + s.offset[2].powi(2)).sqrt();
            assert!(d <= 4.0 + 1e-4);
        }
        let run = |seed| {
            let mut rng = Rng::new(seed);
            (0..16).map(|_| spawn(&e, &mut rng)).collect::<Vec<_>>()
        };
        assert_eq!(run(42), run(42));
        assert_ne!(run(42), run(43));
    }

    #[test]
    fn particles_fall_drag_and_expire_on_time() {
        let mut e = effect();
        e.particle.lifetime = Span::constant(1.0);
        e.particle.gravity = 10.0;
        let mut sim = ParticleSim::new(&e, [0.0; 3], &mut Rng::new(1));
        let mut y = 0.0;
        let mut steps = 0;
        while let Some(d) = sim.step(&e, 0.05) {
            y += d[1];
            steps += 1;
        }
        assert_eq!(steps, 20);
        // Semi-implicit Euler: -g dt^2 (1 + 2 + ... + 20) = -10 * 0.0025 * 210.
        assert!((y + 5.25).abs() < 1e-3, "{y}");
        // Drag 1/s halves the speed in ln 2 seconds.
        e.particle.gravity = 0.0;
        e.particle.drag = 1.0;
        let mut sim = ParticleSim::new(&e, [8.0, 0.0, 0.0], &mut Rng::new(1));
        for _ in 0..20 {
            sim.step(&e, 0.05);
        }
        assert!((sim.velocity[0] - 8.0 * (-1.0f32).exp()).abs() < 1e-3);
    }

    #[test]
    fn looks_follow_curves_colour_keys_camera_fades_and_frames() {
        let mut e = effect();
        e.particle.lifetime = Span::constant(2.0);
        e.particle.size = Curve {
            base: Span::constant(0.5),
            keys: vec![(0.0, 0.0), (0.5, 1.0)],
            ..Curve::default()
        };
        e.particle.color_keys = vec![(0.0, 0xFF0000), (1.0, 0x0000FF)];
        e.particle.fade_near = Some(Span::new(2.0, 4.0));
        e.particle.fade_far = Some(Span::new(10.0, 20.0));
        e.particle.frame = Frames::OverLife;
        let mut sim = ParticleSim::new(&e, [0.0; 3], &mut Rng::new(3));
        for _ in 0..10 {
            sim.step(&e, 0.05);
        }
        // Half a second of two: t = 0.25.
        let look = sim.look(&e, 3.0, 0xFFFFFF, 6);
        assert!((look.size - 0.25).abs() < 1e-5);
        assert!((look.rgba[0] - 0.75).abs() < 1e-5 && (look.rgba[2] - 0.25).abs() < 1e-5);
        assert!((look.rgba[3] - 0.5).abs() < 1e-5);
        assert_eq!(look.frame, 1); // 0.25 of 6 sprites
        assert_eq!(sim.look(&e, 1.0, 0xFFFFFF, 4).rgba[3], 0.0);
        assert_eq!(sim.look(&e, 25.0, 0xFFFFFF, 4).rgba[3], 0.0);
        assert!((sim.look(&e, 15.0, 0xFFFFFF, 4).rgba[3] - 0.5).abs() < 1e-5);
        // The tint multiplies.
        assert_eq!(sim.look(&e, 5.0, 0x000000, 4).rgba[0], 0.0);
        e.particle.frame = Frames::Loop;
        e.particle.frame_rate = 7.0; // 3.5 sprites in, of 4
        assert_eq!(sim.look(&e, 5.0, 0xFFFFFF, 4).frame, 3);
        assert_eq!(sim.look(&e, 5.0, 0xFFFFFF, 1).frame, 0);
    }
}
