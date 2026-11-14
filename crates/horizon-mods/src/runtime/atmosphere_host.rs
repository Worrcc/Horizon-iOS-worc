//! Capability `atmosphere` (API level 33): a client mod's fog and sky
//! profiles and its mix of them, checked here and read by the game every
//! frame ([`Mods::atmosphere_mix`]); the sampling and the cross-fade are
//! [`crate::atmosphere`]'s.
use super::{Mods, State, Status};
use crate::atmosphere::{self as atmo, FogKeyIn, Profile, SkyKeyIn, WaterFogIn};
use crate::atmosphere_fx as fx;
use crate::atmosphere_sky as sky;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use wasmtime::component::{HasSelf, Linker};

mod bindings {
    wasmtime::component::bindgen!({
        path: "../horizon-mod-sdk/wit",
        world: "atmosphere-host",
    });
}
pub use atmosphere::{EyeMedium, Scene as AtmosphereScene};
use bindings::horizon::mods::atmosphere;

pub(super) fn add_to_linker(linker: &mut Linker<State>) -> wasmtime::Result<()> {
    atmosphere::add_to_linker::<State, HasSelf<State>>(linker, |s| s)
}

/// Every accepted `set-mix` gets a new serial, so the game sees a change
/// even when a mod sets the same weights again after another mod's.
static SERIAL: AtomicU64 = AtomicU64::new(1);

/// Every started emitter gets a new serial: the game keys its own
/// simulation by it, so a reused emitter id starts afresh.
static EMITTER_SERIAL: AtomicU64 = AtomicU64::new(1);

/// One live emitter as the mod set it.
#[derive(Clone, Debug)]
pub(super) struct EmitterSlot {
    serial: u64,
    effect: u32,
    anchor: fx::Anchor,
    density: f32,
    tint: u32,
}

/// A live emitter the game simulates: whose, its serial, the effect and how
/// the mod placed it.
#[derive(Clone, Copy, Debug)]
pub struct AtmosphereEmitter<'a> {
    pub mod_id: &'a str,
    pub serial: u64,
    pub effect: &'a Arc<fx::Effect>,
    pub anchor: fx::Anchor,
    pub density: f32,
    pub tint: u32,
}

/// One mod's profiles and current mix, effects and emitters.
#[derive(Default)]
pub(super) struct AtmosphereState {
    /// Namespaces whose content sprite sets the mod may name (its id and
    /// the manifest's `namespaces`).
    namespaces: Vec<String>,
    effects: Vec<Arc<fx::Effect>>,
    emitters: Vec<Option<EmitterSlot>>,
    /// Serials stopped with `fade`, until the game reads them; a serial that
    /// disappears without one has its particles removed.
    faded: Vec<u64>,
    profiles: Vec<Profile>,
    mix: Vec<(u32, f32)>,
    transition: f32,
    /// 0 until the first accepted `set-mix`.
    serial: u64,
    /// First tick of the current second and the calls in it.
    calls: (Option<u64>, usize),
    warned: bool,
    /// The sky sprites the mod set last, and its `set-sky-sprites` calls
    /// in the current second (first tick, count).
    sky: Vec<sky::SkySprite>,
    sky_calls: (Option<u64>, usize),
}

impl AtmosphereState {
    pub(super) fn new(namespaces: Vec<String>) -> Self {
        Self {
            namespaces,
            ..Self::default()
        }
    }
}

/// The mix the game shows: whose profiles, which weights (summing to 1, or
/// empty for vanilla), the cross-fade and a serial that changes with it.
#[derive(Clone, Copy, Debug)]
pub struct AtmosphereMix<'a> {
    pub mod_id: &'a str,
    pub serial: u64,
    pub weights: &'a [(u32, f32)],
    pub transition: f32,
}

impl atmosphere::Host for State {
    fn define_profile(&mut self, profile: atmosphere::Profile) -> Result<u32, String> {
        if self.server {
            return Err("atmosphere is client only".into());
        }
        if self.atmosphere.profiles.len() >= atmo::PROFILES_PER_MOD {
            return Err(format!(
                "at most {} profiles per mod",
                atmo::PROFILES_PER_MOD
            ));
        }
        let fog: Vec<FogKeyIn> = profile
            .fog_keys
            .iter()
            .map(|k| FogKeyIn {
                day_time: k.day_time,
                color: k.color,
                start: k.start,
                end: k.end,
                relative: k.relative,
            })
            .collect();
        let sky: Vec<SkyKeyIn> = profile
            .sky_keys
            .iter()
            .map(|k| SkyKeyIn {
                day_time: k.day_time,
                zenith: k.zenith,
                horizon: k.horizon,
                cloud: k.cloud,
                cloud_far: k.cloud_far,
            })
            .collect();
        let water = profile.water_fog.map(|w| WaterFogIn {
            color: w.color,
            start: w.start,
            end: w.end,
        });
        let profile = Profile::new(&fog, &sky, water)?;
        self.atmosphere.profiles.push(profile);
        Ok(self.atmosphere.profiles.len() as u32 - 1)
    }

    fn set_mix(&mut self, weights: Vec<(u32, f32)>, transition: f32) -> Result<(), String> {
        if self.server {
            return Err("atmosphere is client only".into());
        }
        let (mix, transition) =
            atmo::check_mix(&weights, transition, self.atmosphere.profiles.len())?;
        let tick = self.snapshot.tick;
        let state = &mut self.atmosphere;
        match state.calls.0 {
            Some(start) if tick.saturating_sub(start) < 20 => {}
            _ => state.calls = (Some(tick), 0),
        }
        if state.calls.1 >= atmo::MIX_CALLS_PER_SECOND {
            if !state.warned {
                state.warned = true;
                log::warn!(
                    "Horizon: mod {} calls atmosphere.set-mix more than {} times a second; the rest are ignored",
                    self.id,
                    atmo::MIX_CALLS_PER_SECOND
                );
            }
            return Ok(());
        }
        state.calls.1 += 1;
        state.mix = mix;
        state.transition = transition;
        state.serial = SERIAL.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn conditions(&mut self) -> Option<AtmosphereScene> {
        self.snapshot.atmosphere
    }

    fn define_effect(&mut self, effect: atmosphere::Effect) -> Result<u32, String> {
        if self.server {
            return Err("atmosphere is client only".into());
        }
        if self.atmosphere.effects.len() >= fx::EFFECTS_PER_MOD {
            return Err(format!("at most {} effects per mod", fx::EFFECTS_PER_MOD));
        }
        let effect = convert_effect(effect).checked()?;
        let sprite = &effect.particle.sprite;
        let ns = fx::sprite_namespace(sprite).unwrap_or_default();
        if ns != "minecraft" && ns != self.id && !self.atmosphere.namespaces.iter().any(|n| n == ns)
        {
            return Err(format!(
                "particle sprite {sprite}: namespace {ns} is not this mod's (use its own content or minecraft:<type>)"
            ));
        }
        if !self.snapshot.particle_sets.contains(sprite.as_str()) {
            return Err(format!(
                "particle sprite set {sprite} is not loaded (assets/{ns}/particles/<name>.json in the mod's content, or a vanilla particle type)"
            ));
        }
        self.atmosphere.effects.push(Arc::new(effect));
        Ok(self.atmosphere.effects.len() as u32 - 1)
    }

    fn start_emitter(
        &mut self,
        effect: u32,
        anchor: atmosphere::Anchor,
        density: f32,
        tint: u32,
    ) -> Result<u32, String> {
        if self.server {
            return Err("atmosphere is client only".into());
        }
        if effect as usize >= self.atmosphere.effects.len() {
            return Err(format!("no effect {effect}"));
        }
        if !density.is_finite() || !(0.0..=fx::DENSITY_MAX).contains(&density) {
            return Err(format!("density must be 0..{}", fx::DENSITY_MAX));
        }
        if tint > 0xFF_FFFF {
            return Err(format!("tint {tint:#x} is not 0xRRGGBB"));
        }
        let anchor = convert_anchor(anchor).checked()?;
        let state = &mut self.atmosphere;
        let slot = match state.emitters.iter().position(Option::is_none) {
            Some(i) => i,
            None if state.emitters.len() < fx::EMITTERS_PER_MOD => {
                state.emitters.push(None);
                state.emitters.len() - 1
            }
            None => {
                return Err(format!(
                    "at most {} live emitters per mod",
                    fx::EMITTERS_PER_MOD
                ));
            }
        };
        state.emitters[slot] = Some(EmitterSlot {
            serial: EMITTER_SERIAL.fetch_add(1, Ordering::Relaxed),
            effect,
            anchor,
            density,
            tint,
        });
        Ok(slot as u32)
    }

    fn move_emitter(&mut self, emitter: u32, anchor: atmosphere::Anchor) -> Result<(), String> {
        let anchor = convert_anchor(anchor).checked()?;
        let slot = self
            .atmosphere
            .emitters
            .get_mut(emitter as usize)
            .and_then(Option::as_mut)
            .ok_or_else(|| format!("no live emitter {emitter}"))?;
        slot.anchor = anchor;
        Ok(())
    }

    fn stop_emitter(&mut self, emitter: u32, fade: bool) -> Result<(), String> {
        let state = &mut self.atmosphere;
        let slot = state
            .emitters
            .get_mut(emitter as usize)
            .and_then(Option::take)
            .ok_or_else(|| format!("no live emitter {emitter}"))?;
        if fade {
            // Bounded: the game reads them every particle tick; an unread
            // one only costs its particles their fade.
            if state.faded.len() >= fx::EMITTERS_PER_MOD * 4 {
                state.faded.remove(0);
            }
            state.faded.push(slot.serial);
        }
        Ok(())
    }

    fn set_sky_sprites(&mut self, sprites: Vec<atmosphere::SkySprite>) -> Result<(), String> {
        if self.server {
            return Err("atmosphere is client only".into());
        }
        let tick = self.snapshot.tick;
        let calls = &mut self.atmosphere.sky_calls;
        match calls.0 {
            Some(start) if tick.saturating_sub(start) < 20 => {}
            _ => *calls = (Some(tick), 0),
        }
        if calls.1 >= sky::CALLS_PER_SECOND {
            return Err(format!(
                "at most {} set-sky-sprites calls a second",
                sky::CALLS_PER_SECOND
            ));
        }
        calls.1 += 1;
        let namespaces = &self.atmosphere.namespaces;
        let id = &self.id;
        let own = |ns: &str| ns == id || namespaces.iter().any(|n| n == ns);
        let list = sky::check_list(sprites.into_iter().map(convert_sky).collect(), &own)?;
        self.atmosphere.sky = list;
        Ok(())
    }
}

fn convert_sky(s: atmosphere::SkySprite) -> sky::SkySprite {
    sky::SkySprite {
        key: s.key,
        texture: s.texture,
        uv: [s.uv.0, s.uv.1, s.uv.2, s.uv.3],
        yaw: s.yaw,
        elevation: s.elevation,
        width: s.width,
        height: s.height,
        roll: s.roll,
        tint: s.tint,
        alpha: s.alpha,
        blend: match s.blend {
            atmosphere::SkyBlend::Translucent => sky::Blend::Translucent,
            atmosphere::SkyBlend::Emissive => sky::Blend::Emissive,
        },
        space: match s.space {
            atmosphere::SkySpace::Compass => sky::Space::Compass,
            atmosphere::SkySpace::Stars => sky::Space::Stars,
        },
        drift: [s.drift_yaw, s.drift_elevation, s.spin],
        lifetime: s.lifetime,
        fade_in: s.fade_in,
        fade_out: s.fade_out,
    }
}

fn span(s: atmosphere::Span) -> fx::Span {
    fx::Span { lo: s.lo, hi: s.hi }
}
fn span3(s: atmosphere::Span3) -> [fx::Span; 3] {
    [span(s.x), span(s.y), span(s.z)]
}
fn curve(c: atmosphere::Curve) -> fx::Curve {
    fx::Curve {
        base: span(c.base),
        keys: c.keys.iter().map(|k| (k.t, k.value)).collect(),
        ease: match c.ease {
            atmosphere::Ease::Linear => fx::Ease::Linear,
            atmosphere::Ease::Smooth => fx::Ease::Smooth,
            atmosphere::Ease::Step => fx::Ease::Step,
        },
        wave: c.wave.map(|w| fx::Wave {
            amplitude: span(w.amplitude),
            period: span(w.period),
            phase: span(w.phase),
        }),
    }
}
fn convert_anchor(a: atmosphere::Anchor) -> fx::Anchor {
    match a {
        atmosphere::Anchor::Camera => fx::Anchor::Camera,
        atmosphere::Anchor::Point((x, y, z)) => fx::Anchor::World([x, y, z]),
    }
}
fn convert_effect(e: atmosphere::Effect) -> fx::Effect {
    let (m, p) = (e.emitter, e.particle);
    fx::Effect {
        emitter: fx::Emitter {
            looping: m.looping,
            duration: m.duration,
            rate: m.rate,
            density_scaled: m.density_scaled,
            max: m.max,
            burst: m.burst,
            shape: match m.shape {
                atmosphere::EmitterShape::Point => fx::Shape::Point,
                atmosphere::EmitterShape::Box => fx::Shape::Box,
                atmosphere::EmitterShape::Disc => fx::Shape::Disc,
                atmosphere::EmitterShape::Sphere => fx::Shape::Sphere,
            },
            offset: span3(m.offset),
            half_size: [m.half_size.0, m.half_size.1, m.half_size.2],
            radius: span(m.radius),
            surface: m.surface,
            direction: m.direction.map(span3),
        },
        particle: fx::ParticleSpec {
            sprite: p.sprite,
            frame: match p.frame {
                atmosphere::FrameMode::First => fx::Frames::First,
                atmosphere::FrameMode::Random => fx::Frames::Random,
                atmosphere::FrameMode::OverLife => fx::Frames::OverLife,
                atmosphere::FrameMode::Loop => fx::Frames::Loop,
            },
            frame_rate: p.frame_rate,
            lifetime: span(p.lifetime),
            speed: span(p.speed),
            velocity: span3(p.velocity),
            drag: p.drag,
            gravity: p.gravity,
            acceleration: p.acceleration.map(|a| [curve(a.x), curve(a.y), curve(a.z)]),
            size: curve(p.size),
            alpha: curve(p.alpha),
            color: p.color,
            color_keys: p.color_keys.iter().map(|k| (k.t, k.color)).collect(),
            shade: p.shade,
            spin: span(p.spin),
            spin_rate: span(p.spin_rate),
            fade_near: p.fade_near.map(span),
            fade_far: p.fade_far.map(span),
            emissive: p.emissive,
            translucent: p.translucent,
            only_in: match p.only_in {
                atmosphere::Medium::Anywhere => fx::Medium::Anywhere,
                atmosphere::Medium::Air => fx::Medium::Air,
                atmosphere::Medium::Water => fx::Medium::Water,
            },
            die_nearer_than: p.die_nearer_than,
        },
    }
}

impl Mods {
    /// A running mod has capability `atmosphere`: the game fills
    /// [`super::Snapshot::atmosphere`] for it.
    pub fn wants_atmosphere(&self) -> bool {
        self.entries.iter().any(|e| {
            e.status == Status::Running
                && e.manifest
                    .as_ref()
                    .is_some_and(|m| m.capabilities.contains(&crate::Capability::Atmosphere))
        })
    }
    /// The mix to show: the first running mod (load order) with a non-empty
    /// mix, else the first that set one (so its return to vanilla fades),
    /// else none.
    pub fn atmosphere_mix(&self) -> Option<AtmosphereMix<'_>> {
        let states = || {
            self.entries
                .iter()
                .filter_map(|e| Some(e.run.as_ref()?.store.data()))
                .filter(|s| s.atmosphere.serial != 0)
        };
        let state = states()
            .find(|s| !s.atmosphere.mix.is_empty())
            .or_else(|| states().next())?;
        Some(AtmosphereMix {
            mod_id: &state.id,
            serial: state.atmosphere.serial,
            weights: &state.atmosphere.mix,
            transition: state.atmosphere.transition,
        })
    }
    /// Every running mod's live emitters, in load order.
    pub fn atmosphere_emitters(&self) -> impl Iterator<Item = AtmosphereEmitter<'_>> {
        self.entries
            .iter()
            .filter(|e| e.status == Status::Running)
            .filter_map(|e| Some(e.run.as_ref()?.store.data()))
            .flat_map(|s| {
                s.atmosphere.emitters.iter().flatten().filter_map(|slot| {
                    Some(AtmosphereEmitter {
                        mod_id: &s.id,
                        serial: slot.serial,
                        effect: s.atmosphere.effects.get(slot.effect as usize)?,
                        anchor: slot.anchor,
                        density: slot.density,
                        tint: slot.tint,
                    })
                })
            })
    }
    /// Every running mod's sky sprites, in load order (each mod's in its
    /// list's order), with the mod's id.
    pub fn atmosphere_sky_sprites(&self) -> impl Iterator<Item = (&str, &sky::SkySprite)> {
        self.entries
            .iter()
            .filter(|e| e.status == Status::Running)
            .filter_map(|e| Some(e.run.as_ref()?.store.data()))
            .flat_map(|s| {
                s.atmosphere
                    .sky
                    .iter()
                    .map(|sprite| (s.id.as_str(), sprite))
            })
    }
    /// The serials stopped with `fade` since the last call.
    pub fn take_atmosphere_fades(&mut self) -> Vec<u64> {
        let mut out = Vec::new();
        for e in &mut self.entries {
            if let Some(run) = e.run.as_mut() {
                out.append(&mut run.store.data_mut().atmosphere.faded);
            }
        }
        out
    }
    /// A one-shot emitter ran out: its id frees for the mod.
    pub fn end_atmosphere_emitter(&mut self, serial: u64) {
        for e in &mut self.entries {
            if let Some(run) = e.run.as_mut() {
                for slot in &mut run.store.data_mut().atmosphere.emitters {
                    if slot.as_ref().is_some_and(|s| s.serial == serial) {
                        *slot = None;
                        return;
                    }
                }
            }
        }
    }
    /// A running mod's profile by id.
    pub fn atmosphere_profile(&self, mod_id: &str, id: u32) -> Option<&Profile> {
        self.entries
            .iter()
            .filter_map(|e| Some(e.run.as_ref()?.store.data()))
            .find(|s| s.id == mod_id)?
            .atmosphere
            .profiles
            .get(id as usize)
    }
}
