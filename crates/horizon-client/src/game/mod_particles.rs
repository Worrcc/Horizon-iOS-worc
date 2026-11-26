//! Capability `atmosphere`'s particle emitters (API level 33) in the particle
//! tick: the running mods' emitters spawn into the client particle engine,
//! and each particle moves and fades by its effect's curves through a
//! [`CurveController`]. The mod only starts, moves and stops emitters; the
//! curves, clock and shapes are `horizon_mods::atmosphere_fx`'s
//! (`docs/api-expansion-d.md`).
use super::Game;
use horizon_client::particles::{Particle, ParticleController, ParticleLevel, ParticleUpdate};
use horizon_mods::atmosphere_fx::{
    self as fx, Anchor, Effect, EmitterClock, Medium, ParticleSim, Rng,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// The particle engine's step, in seconds.
const DT: f32 = 0.05;

/// The camera as the last particle tick saw it, shared with the controllers.
#[derive(Debug, Default)]
struct CameraCell([AtomicU64; 3]);
impl CameraCell {
    fn set(&self, at: [f64; 3]) {
        for (cell, v) in self.0.iter().zip(at) {
            cell.store(v.to_bits(), Ordering::Relaxed);
        }
    }
    fn get(&self) -> [f64; 3] {
        [0, 1, 2].map(|i| f64::from_bits(self.0[i].load(Ordering::Relaxed)))
    }
}

/// One emitter's live particles, and whether they must go at once.
#[derive(Debug, Default)]
struct EmitterShared {
    live: AtomicUsize,
    killed: AtomicBool,
}

/// Counts a live particle for its mod and its emitter: every controller
/// instance holds one (a cloned particle counts again, a dropped one, by
/// expiry, the engine's cap, a reload or a clear, stops counting).
#[derive(Debug)]
struct Counted {
    mod_live: Arc<AtomicUsize>,
    emitter: Arc<EmitterShared>,
}
impl Counted {
    fn new(mod_live: Arc<AtomicUsize>, emitter: Arc<EmitterShared>) -> Self {
        mod_live.fetch_add(1, Ordering::Relaxed);
        emitter.live.fetch_add(1, Ordering::Relaxed);
        Self { mod_live, emitter }
    }
}
impl Clone for Counted {
    fn clone(&self) -> Self {
        Self::new(self.mod_live.clone(), self.emitter.clone())
    }
}
impl Drop for Counted {
    fn drop(&mut self) {
        self.mod_live.fetch_sub(1, Ordering::Relaxed);
        self.emitter.live.fetch_sub(1, Ordering::Relaxed);
    }
}

/// A mod effect's particle: its own draws and age, moved and coloured by
/// the effect's curves every particle tick.
#[derive(Clone, Debug)]
pub(super) struct CurveController {
    effect: Arc<Effect>,
    sim: ParticleSim,
    tint: u32,
    frames: usize,
    camera: Arc<CameraCell>,
    counted: Counted,
}

fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

impl CurveController {
    /// Size, colour, roll and light for this tick; none when it must die.
    fn show(&self, p: &mut Particle) -> Option<usize> {
        let spec = &self.effect.particle;
        let d = distance(p.position, self.camera.get()) as f32;
        if spec.die_nearer_than > 0.0 && d < spec.die_nearer_than {
            return None;
        }
        let look = self.sim.look(&self.effect, d, self.tint, self.frames);
        p.size = look.size;
        p.rgba = look.rgba;
        p.set_roll(look.roll);
        Some(look.frame)
    }
}

impl ParticleController for CurveController {
    fn tick(&mut self, p: &mut Particle, level: &ParticleLevel<'_>) -> ParticleUpdate {
        let none = ParticleUpdate::default();
        if self.counted.emitter.killed.load(Ordering::Relaxed) || !p.advance_age() {
            p.remove();
            return none;
        }
        let Some(d) = self.sim.step(&self.effect, DT) else {
            p.remove();
            return none;
        };
        for (c, d) in p.position.iter_mut().zip(d) {
            *c += d;
        }
        let stays = match self.effect.particle.only_in {
            Medium::Anywhere => true,
            Medium::Air => level.in_air(p.position),
            Medium::Water => level.in_water(p.position),
        };
        match self.show(p).filter(|_| stays) {
            Some(frame) => ParticleUpdate {
                frame: Some(frame),
                ..none
            },
            None => {
                p.remove();
                none
            }
        }
    }
}

/// One emitter's spawn clock and generator, kept by its serial.
struct EmitterRun {
    clock: EmitterClock,
    rng: Rng,
    shared: Arc<EmitterShared>,
    /// A one-shot ran out: its particles live on when the mod forgets it.
    ended: bool,
}

/// The emitters' runs and every mod's live particles, across ticks.
#[derive(Default)]
pub(super) struct ModParticles {
    runs: HashMap<u64, EmitterRun>,
    live: HashMap<String, Arc<AtomicUsize>>,
    camera: Arc<CameraCell>,
    /// The particle engine's sprite set names for the mods' snapshot; remade
    /// after a resource reload.
    pub(super) sets: Option<Arc<HashSet<String>>>,
}

impl ModParticles {
    /// Live particles of each mod that has any, for `horizon-ctl status`.
    pub(super) fn live(&self) -> std::collections::BTreeMap<&str, usize> {
        self.live
            .iter()
            .map(|(id, n)| (id.as_str(), n.load(Ordering::Relaxed)))
            .filter(|&(_, n)| n > 0)
            .collect()
    }
}

impl Game<'_> {
    /// The sprite sets a mod effect may name (`Snapshot::particle_sets`).
    pub(super) fn mod_particle_sets(&mut self) -> Arc<HashSet<String>> {
        if let Some(sets) = &self.mods.particles.sets {
            return sets.clone();
        }
        let sets = Arc::new(self.particles.sprite_set_names());
        self.mods.particles.sets = Some(sets.clone());
        sets
    }

    /// Once per particle tick, before the engine ticks: forget stopped
    /// emitters (removing their particles unless they fade), move the
    /// clocks and spawn what is due within the caps. Nothing spawns on the
    /// title screen (the tick does not run while paused), with Particles:
    /// Minimal, or beyond 64 blocks of the camera.
    pub(super) fn tick_mod_emitters(&mut self) {
        let state = &mut self.mods;
        let fades = state.mods.take_atmosphere_fades();
        let emitters: Vec<(String, u64, Arc<Effect>, Anchor, f32, u32)> = state
            .mods
            .atmosphere_emitters()
            .map(|e| {
                let (id, effect) = (e.mod_id.to_owned(), e.effect.clone());
                (id, e.serial, effect, e.anchor, e.density, e.tint)
            })
            .collect();
        let particles = &mut state.particles;
        particles.runs.retain(|serial, run| {
            let alive = emitters.iter().any(|e| e.1 == *serial);
            if !alive && !run.ended && !fades.contains(serial) {
                run.shared.killed.store(true, Ordering::Relaxed);
            }
            alive
        });
        if emitters.is_empty() {
            return;
        }
        let camera = self.particles.camera;
        particles.camera.set(camera);
        let setting = match self.particles.particle_level {
            0 => 1.0,
            1 => 0.5,
            _ => 0.0,
        };
        let spawning = self.title.is_none() && setting > 0.0;
        let mut spawned: HashMap<String, usize> = HashMap::new();
        let mut ended = Vec::new();
        for (mod_id, serial, effect, anchor, density, tint) in emitters {
            let run = particles.runs.entry(serial).or_insert_with(|| EmitterRun {
                clock: EmitterClock::new(),
                rng: Rng::new(serial.wrapping_mul(0x9E37_79B9_7F4A_7C15)),
                shared: Arc::default(),
                ended: false,
            });
            if run.ended {
                continue;
            }
            let scale = setting
                * if effect.emitter.density_scaled {
                    density
                } else {
                    1.0
                };
            let (due, done) = run.clock.step(&effect.emitter, DT, scale);
            if done {
                run.ended = true;
                ended.push(serial);
            }
            let at = anchor.at(camera);
            // The farthest a spawn reaches is 64 blocks of offset plus 64 of shape.
            if !spawning || due == 0 || distance(at, camera) > fx::SPAWN_RADIUS * 3.0 {
                continue;
            }
            let mod_live = particles.live.entry(mod_id.clone()).or_default().clone();
            let count = spawned.entry(mod_id).or_default();
            let room = [
                due as usize,
                fx::SPAWNS_PER_TICK.saturating_sub(*count),
                fx::PARTICLES_PER_MOD.saturating_sub(mod_live.load(Ordering::Relaxed)),
                (effect.emitter.max as usize)
                    .saturating_sub(run.shared.live.load(Ordering::Relaxed)),
            ]
            .into_iter()
            .min()
            .unwrap_or(0);
            *count += room;
            let spec = &effect.particle;
            let frames = self.particles.sprite_set_len(&spec.sprite).unwrap_or(1);
            for _ in 0..room {
                let spawn = fx::spawn(&effect, &mut run.rng);
                let sim = ParticleSim::new(&effect, spawn.velocity, &mut run.rng);
                let position = [0, 1, 2].map(|i| at[i] + spawn.offset[i]);
                let ticks = (sim.life / DT).ceil() as i32 + 1;
                let controller = CurveController {
                    effect: effect.clone(),
                    sim,
                    tint,
                    frames,
                    camera: particles.camera.clone(),
                    counted: Counted::new(mod_live.clone(), run.shared.clone()),
                };
                let Some(mut p) = self.particles.curve_particle(
                    &spec.sprite,
                    position,
                    spec.translucent,
                    fx::SPAWN_RADIUS,
                    controller.clone(),
                ) else {
                    continue;
                };
                p.lifetime = ticks;
                p.set_full_bright(spec.emissive);
                if let Some(frame) = controller.show(&mut p) {
                    self.particles.set_sprite_frame(&mut p, frame);
                    self.particles.push(p);
                }
            }
        }
        for serial in ended {
            self.mods.mods.end_atmosphere_emitter(serial);
        }
    }
}
