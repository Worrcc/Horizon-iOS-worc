//! Capability `atmosphere` (API level 33): a client mod's fog and sky
//! profiles and its mix of them, checked here and read by the game every
//! frame ([`Mods::atmosphere_mix`]); the sampling and the cross-fade are
//! [`crate::atmosphere`]'s.
use super::{Mods, State, Status};
use crate::atmosphere::{self as atmo, FogKeyIn, Profile, SkyKeyIn, WaterFogIn};
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

/// One mod's profiles and current mix.
#[derive(Default)]
pub(super) struct AtmosphereState {
    profiles: Vec<Profile>,
    mix: Vec<(u32, f32)>,
    transition: f32,
    /// 0 until the first accepted `set-mix`.
    serial: u64,
    /// First tick of the current second and the calls in it.
    calls: (Option<u64>, usize),
    warned: bool,
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
