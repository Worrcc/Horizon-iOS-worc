//! Capability `atmosphere` (API level 33) in the frame: the running mods'
//! profile mix, cross-faded and sampled once per frame, blended into the
//! sky after vanilla weather and into the fog after `setup_fog`, before the
//! Fog option and native mods' fog hooks (`docs/api-expansion-d.md`).
use super::Game;
use horizon_client::environment::Sky;
use horizon_client::fog::{FogData, FogInputs, FogType};
use horizon_mods::atmosphere::{
    FogSample, Sample, SkySample, blend, linear_to_srgb, srgb_to_linear,
};

/// This frame's mod atmosphere: each profile's weight and sample; vanilla
/// takes what the weights leave. Empty: the frame is vanilla.
#[derive(Default)]
pub(super) struct AtmosphereBlend {
    parts: Vec<(f32, Sample)>,
}

fn to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(srgb_to_linear)
}
fn to_srgb(c: [f32; 3]) -> [f32; 3] {
    c.map(linear_to_srgb)
}

impl AtmosphereBlend {
    #[cfg_attr(not(feature = "mods"), allow(dead_code))]
    fn new(parts: Vec<(f32, Sample)>) -> Self {
        Self { parts }
    }
    /// Zenith, horizon and cloud colours over the weathered vanilla sky.
    pub(super) fn apply_sky(&self, sky: &mut Sky) {
        if !self.parts.iter().any(|(_, s)| s.sky.is_some()) {
            return;
        }
        let cloud = [sky.cloud_color[0], sky.cloud_color[1], sky.cloud_color[2]];
        let vanilla = SkySample {
            zenith: to_linear(sky.sky_color),
            horizon: to_linear(sky.fog_color),
            cloud: to_linear(cloud),
            cloud_far: to_linear(cloud),
        };
        let out = SkySample::from_array(blend(
            vanilla.to_array(),
            self.parts
                .iter()
                .map(|(w, s)| (*w, s.sky.map(SkySample::to_array))),
        ));
        sky.sky_color = to_srgb(out.zenith);
        sky.fog_color = to_srgb(out.horizon);
        let [r, g, b] = to_srgb(out.cloud);
        sky.cloud_color = [r, g, b, sky.cloud_color[3]];
    }
    /// The fog's colour and environmental distances: the profiles' air fog
    /// in the open, their water fog under water. Lava, powder snow,
    /// blindness, darkness and a boss's world fog keep vanilla's.
    pub(super) fn apply_fog(&self, fog: &mut FogData, input: &FogInputs) {
        let effects = input.blindness.is_some() || input.darkness.is_some() || input.boss_world_fog;
        self.apply_fog_in(fog, input.fog_type, effects);
    }
    /// [`Self::apply_fog`] for the camera in `fog_type`; `effects`: blindness,
    /// darkness or a boss's world fog close the view in.
    fn apply_fog_in(&self, fog: &mut FogData, fog_type: FogType, effects: bool) {
        if self.parts.is_empty() || effects {
            return;
        }
        let pick: fn(&Sample) -> Option<FogSample> = match fog_type {
            FogType::Atmospheric => |s| s.fog,
            FogType::Water => |s| s.water,
            FogType::Lava | FogType::PowderSnow => return,
        };
        if !self.parts.iter().any(|(_, s)| pick(s).is_some()) {
            return;
        }
        let vanilla = FogSample {
            color: to_linear([fog.color[0], fog.color[1], fog.color[2]]),
            start: fog.environmental_start,
            end: fog.environmental_end,
        };
        let out = FogSample::from_array(blend(
            vanilla.to_array(),
            self.parts
                .iter()
                .map(|(w, s)| (*w, pick(s).map(FogSample::to_array))),
        ));
        let [r, g, b] = to_srgb(out.color);
        fog.color = [r, g, b, fog.color[3]];
        fog.environmental_start = out.start;
        // The fog shader divides by end - start.
        fog.environmental_end = out.end.max(out.start + 0.01);
    }
}

/// The cross-fade between the mods' mixes, kept across frames.
#[cfg(feature = "mods")]
#[derive(Default)]
pub(super) struct AtmosphereFade {
    fader: horizon_mods::atmosphere::Fader<(String, u32)>,
    last: Option<std::time::Instant>,
}

impl Game<'_> {
    /// Moves the cross-fade on and samples the mixed profiles for this frame.
    pub(super) fn atmosphere_blend(&mut self) -> AtmosphereBlend {
        #[cfg(feature = "mods")]
        {
            let day_time = self.view_day_time() as f64;
            let render_distance = self.terrain_distance() as f32 * 16.0;
            let title = self.title.is_some();
            let state = &mut self.mods;
            let fade = &mut state.atmosphere;
            let now = std::time::Instant::now();
            let seconds = fade
                .last
                .replace(now)
                .map_or(0.0, |last| (now - last).as_secs_f32());
            match state.mods.atmosphere_mix() {
                Some(mix) if fade.fader.target() != Some(mix.serial) => fade.fader.retarget(
                    Some(mix.serial),
                    mix.weights
                        .iter()
                        .map(|&(id, w)| ((mix.mod_id.to_owned(), id), w))
                        .collect(),
                    mix.transition,
                ),
                Some(_) => {}
                None => fade.fader.retarget(None, Vec::new(), 0.0),
            }
            fade.fader.advance(seconds);
            if title || fade.fader.is_vanilla() {
                return AtmosphereBlend::default();
            }
            let parts = fade
                .fader
                .weights()
                .into_iter()
                .filter_map(|((id, profile), w)| {
                    let profile = state.mods.atmosphere_profile(&id, profile)?;
                    Some((w, profile.sample(day_time, render_distance)))
                })
                .collect();
            AtmosphereBlend::new(parts)
        }
        #[cfg(not(feature = "mods"))]
        AtmosphereBlend::default()
    }

    /// What capability `atmosphere`'s `conditions` reads this tick; none on
    /// the title, with no player or when no running mod has the capability.
    #[cfg(feature = "mods")]
    pub(super) fn atmosphere_scene(&self) -> Option<horizon_mods::AtmosphereScene> {
        use horizon_mods::EyeMedium;
        if self.title.is_some() || self.player.is_none() || !self.mods.mods.wants_atmosphere() {
            return None;
        }
        let at = self.view.eye;
        let [yaw, pitch] = self.view.eye_rotation;
        let (rain, thunder) = self.view_weather(self.clock.fraction() as f32);
        let day = self.view_day_time();
        let [x, y, z] = at.map(|v| v.floor() as i32);
        let light = self
            .world
            .chunks
            .get(&(x >> 4, z >> 4))
            .and_then(|chunk| {
                if y >= chunk.max_y() {
                    Some(0xF0)
                } else {
                    (y >= chunk.min_y()).then(|| chunk.light(x & 15, y, z & 15))
                }
            })
            .unwrap_or(0);
        let eye = match horizon_client::fog::fog_type(
            &self.world,
            self.res.facts(),
            at,
            &self.near_plane(),
        ) {
            FogType::Atmospheric => EyeMedium::Air,
            FogType::Water => EyeMedium::Water,
            FogType::Lava => EyeMedium::Lava,
            FogType::PowderSnow => EyeMedium::PowderSnow,
        };
        let sky_visible = super::render_tick::motion_blocking_height(
            &self.world,
            &self.pack.assets.properties,
            x,
            z,
        ) <= y;
        Some(horizon_mods::AtmosphereScene {
            rain,
            thunder,
            day_time: day.rem_euclid(24_000) as u32,
            moon_phase: day.div_euclid(24_000).rem_euclid(8) as u8,
            camera_x: at[0],
            camera_y: at[1],
            camera_z: at[2],
            yaw,
            pitch,
            eye,
            sky_light: light >> 4,
            block_light: light & 15,
            sky_visible,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_mods::atmosphere::{FogKeyIn, Profile, WaterFogIn};

    #[track_caller]
    fn near<const N: usize>(a: [f32; N], b: [f32; N]) {
        assert!(
            a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-5),
            "{a:?} != {b:?}"
        );
    }

    fn vanilla_fog() -> FogData {
        FogData {
            color: [1.0, 1.0, 1.0, 1.0],
            environmental_start: 100.0,
            environmental_end: 200.0,
            render_distance_start: 0.0,
            render_distance_end: 256.0,
            sky_end: 256.0,
            cloud_end: 2048.0,
        }
    }
    fn red_profile() -> Sample {
        Profile::new(
            &[FogKeyIn {
                day_time: 0,
                color: 0xFF0000,
                start: 0.0,
                end: 40.0,
                relative: false,
            }],
            &[],
            Some(WaterFogIn {
                color: 0x0000FF,
                start: 1.0,
                end: 8.0,
            }),
        )
        .unwrap()
        .sample(0.0, 256.0)
    }

    #[test]
    fn a_full_weight_profile_replaces_the_air_and_water_fog() {
        let blend = AtmosphereBlend::new(vec![(1.0, red_profile())]);
        let mut fog = vanilla_fog();
        blend.apply_fog_in(&mut fog, FogType::Atmospheric, false);
        near(fog.color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            (fog.environmental_start, fog.environmental_end),
            (0.0, 40.0)
        );
        assert_eq!(fog.render_distance_end, 256.0);
        let mut water = vanilla_fog();
        blend.apply_fog_in(&mut water, FogType::Water, false);
        near(water.color, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(water.environmental_end, 8.0);
    }

    #[test]
    fn half_weight_mixes_with_vanilla_and_lava_blindness_stay_vanilla() {
        let blend = AtmosphereBlend::new(vec![(0.5, red_profile())]);
        let mut fog = vanilla_fog();
        blend.apply_fog_in(&mut fog, FogType::Atmospheric, false);
        assert_eq!(
            (fog.environmental_start, fog.environmental_end),
            (50.0, 120.0)
        );
        // Half of linear white and red, back in sRGB.
        near([fog.color[0]], [1.0]);
        assert!((fog.color[1] - linear_to_srgb(0.5)).abs() < 1e-5);
        for (fog_type, effects) in [
            (FogType::Lava, false),
            (FogType::PowderSnow, false),
            (FogType::Atmospheric, true),
            (FogType::Water, true),
        ] {
            let mut fog = vanilla_fog();
            blend.apply_fog_in(&mut fog, fog_type, effects);
            assert_eq!(fog, vanilla_fog());
        }
    }

    #[test]
    fn sky_parts_set_zenith_horizon_and_clouds() {
        let sky_profile = Profile::new(
            &[],
            &[horizon_mods::atmosphere::SkyKeyIn {
                day_time: 0,
                zenith: 0x0000FF,
                horizon: 0x00FF00,
                cloud: 0xFF0000,
                cloud_far: 0,
            }],
            None,
        )
        .unwrap()
        .sample(0.0, 1.0);
        let blend = AtmosphereBlend::new(vec![(1.0, sky_profile)]);
        let mut sky = Sky::default();
        sky.cloud_color = [1.0, 1.0, 1.0, 0.8];
        blend.apply_sky(&mut sky);
        near(sky.sky_color, [0.0, 0.0, 1.0]);
        near(sky.fog_color, [0.0, 1.0, 0.0]);
        near(sky.cloud_color, [1.0, 0.0, 0.0, 0.8]);
        // A fog-only profile leaves the sky alone.
        let mut sky = Sky::default();
        AtmosphereBlend::new(vec![(1.0, red_profile())]).apply_sky(&mut sky);
        assert_eq!(sky.sky_color, Sky::default().sky_color);
    }
}
