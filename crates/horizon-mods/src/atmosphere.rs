//! Capability `atmosphere` (API level 33) without the runtime: the checks on
//! a mod's fog and sky profiles, sampling a profile at a time of day,
//! blending samples with vanilla, and the cross-fade between mixes. The
//! game samples every frame (`docs/api-expansion-d.md`); the mod only picks
//! the mix.

/// Profiles one mod may define.
pub const PROFILES_PER_MOD: usize = 64;
/// Keys in one profile list (fog or sky).
pub const KEYS_PER_LIST: usize = 32;
/// Profiles in one mix.
pub const MIX_PROFILES: usize = 4;
/// The longest cross-fade, in seconds.
pub const TRANSITION_MAX: f32 = 30.0;
/// `set-mix` calls a mod may make per second (20 ticks); more are ignored.
pub const MIX_CALLS_PER_SECOND: usize = 8;
/// Ticks in a day: key times wrap here.
pub const DAY: u32 = 24000;
/// The farthest absolute fog distance, in blocks.
pub const DISTANCE_MAX: f32 = 4096.0;
/// The largest relative fog distance, in render distances.
pub const RELATIVE_MAX: f32 = 2.0;

/// A fog key as the mod sends it (WIT `fog-key`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogKeyIn {
    pub day_time: u32,
    pub color: u32,
    pub start: f32,
    pub end: f32,
    pub relative: bool,
}
/// A sky key as the mod sends it (WIT `sky-key`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkyKeyIn {
    pub day_time: u32,
    pub zenith: u32,
    pub horizon: u32,
    pub cloud: u32,
    pub cloud_far: u32,
}
/// Water fog as the mod sends it (WIT `water-fog`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterFogIn {
    pub color: u32,
    pub start: f32,
    pub end: f32,
}

/// A colour in linear RGB.
pub type Linear = [f32; 3];

#[derive(Clone, Copy, Debug, PartialEq)]
struct FogKey {
    day_time: u32,
    color: Linear,
    start: f32,
    end: f32,
    relative: bool,
}
#[derive(Clone, Copy, Debug, PartialEq)]
struct SkyKey {
    day_time: u32,
    colors: [Linear; 4],
}

/// A checked profile: keys sorted by time, colours in linear RGB.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    fog: Vec<FogKey>,
    sky: Vec<SkyKey>,
    water: Option<FogSample>,
}

/// Fog at one moment: colour (linear RGB) and absolute distances in blocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogSample {
    pub color: Linear,
    pub start: f32,
    pub end: f32,
}
/// The sky at one moment, linear RGB.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkySample {
    pub zenith: Linear,
    pub horizon: Linear,
    pub cloud: Linear,
    pub cloud_far: Linear,
}
/// A profile sampled at one moment; None where it leaves vanilla.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub fog: Option<FogSample>,
    pub sky: Option<SkySample>,
    pub water: Option<FogSample>,
}

/// sRGB `0xRRGGBB` to linear RGB.
pub fn linear(rgb: u32) -> Linear {
    [16, 8, 0].map(|shift| srgb_to_linear(((rgb >> shift) & 0xFF) as f32 / 255.0))
}
/// One sRGB channel (0..1) to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
/// One linear channel to sRGB (0..1).
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn check_color(what: &str, rgb: u32) -> Result<(), String> {
    if rgb > 0xFF_FFFF {
        return Err(format!("{what} {rgb:#x} is not 0xRRGGBB"));
    }
    Ok(())
}
fn check_distances(what: &str, start: f32, end: f32, max: f32) -> Result<(), String> {
    if !(start.is_finite() && end.is_finite()) {
        return Err(format!("{what} distances must be finite"));
    }
    if !(0.0..=max).contains(&start) || !(0.0..=max).contains(&end) {
        return Err(format!("{what} distances must be 0..{max}"));
    }
    if start > end {
        return Err(format!("{what} starts after it ends"));
    }
    Ok(())
}
fn check_times(what: &str, times: impl Iterator<Item = u32>) -> Result<Vec<u32>, String> {
    let mut seen: Vec<u32> = Vec::new();
    for t in times {
        if t > DAY {
            return Err(format!("{what} day-time {t} is past {DAY}"));
        }
        let t = t % DAY;
        if seen.contains(&t) {
            return Err(format!("two {what}s at day-time {t}"));
        }
        seen.push(t);
    }
    Ok(seen)
}

impl Profile {
    /// Checks a profile as the mod sent it.
    pub fn new(
        fog: &[FogKeyIn],
        sky: &[SkyKeyIn],
        water: Option<WaterFogIn>,
    ) -> Result<Self, String> {
        if fog.is_empty() && sky.is_empty() && water.is_none() {
            return Err("a profile needs fog keys, sky keys or water fog".into());
        }
        if fog.len() > KEYS_PER_LIST || sky.len() > KEYS_PER_LIST {
            return Err(format!("at most {KEYS_PER_LIST} keys per list"));
        }
        let fog_times = check_times("fog key", fog.iter().map(|k| k.day_time))?;
        let sky_times = check_times("sky key", sky.iter().map(|k| k.day_time))?;
        let mut fog_keys = Vec::with_capacity(fog.len());
        for (k, day_time) in fog.iter().zip(fog_times) {
            check_color("fog colour", k.color)?;
            let max = if k.relative {
                RELATIVE_MAX
            } else {
                DISTANCE_MAX
            };
            check_distances("fog", k.start, k.end, max)?;
            fog_keys.push(FogKey {
                day_time,
                color: linear(k.color),
                start: k.start,
                end: k.end,
                relative: k.relative,
            });
        }
        let mut sky_keys = Vec::with_capacity(sky.len());
        for (k, day_time) in sky.iter().zip(sky_times) {
            for c in [k.zenith, k.horizon, k.cloud, k.cloud_far] {
                check_color("sky colour", c)?;
            }
            sky_keys.push(SkyKey {
                day_time,
                colors: [k.zenith, k.horizon, k.cloud, k.cloud_far].map(linear),
            });
        }
        let water = match water {
            Some(w) => {
                check_color("water fog colour", w.color)?;
                check_distances("water fog", w.start, w.end, DISTANCE_MAX)?;
                Some(FogSample {
                    color: linear(w.color),
                    start: w.start,
                    end: w.end,
                })
            }
            None => None,
        };
        fog_keys.sort_by_key(|k| k.day_time);
        sky_keys.sort_by_key(|k| k.day_time);
        Ok(Self {
            fog: fog_keys,
            sky: sky_keys,
            water,
        })
    }

    /// The profile at `day_time` (ticks, any value: wrapped to a day), with
    /// relative fog distances against `render_distance` blocks.
    pub fn sample(&self, day_time: f64, render_distance: f32) -> Sample {
        let t = day_time.rem_euclid(f64::from(DAY)) as f32;
        let fog = segment(self.fog.len(), |i| self.fog[i].day_time, t).map(|(a, b, f)| {
            let (a, b) = (&self.fog[a], &self.fog[b]);
            let distance = |k: &FogKey| {
                let scale = if k.relative { render_distance } else { 1.0 };
                (k.start * scale, k.end * scale)
            };
            let ((sa, ea), (sb, eb)) = (distance(a), distance(b));
            FogSample {
                color: lerp3(a.color, b.color, f),
                start: lerp(sa, sb, f),
                end: lerp(ea, eb, f),
            }
        });
        let sky = segment(self.sky.len(), |i| self.sky[i].day_time, t).map(|(a, b, f)| {
            let (a, b) = (&self.sky[a].colors, &self.sky[b].colors);
            let c: [Linear; 4] = std::array::from_fn(|i| lerp3(a[i], b[i], f));
            SkySample {
                zenith: c[0],
                horizon: c[1],
                cloud: c[2],
                cloud_far: c[3],
            }
        });
        Sample {
            fog,
            sky,
            water: self.water,
        }
    }
}

/// The two keys around `t` and how far `t` is from the first to the second;
/// keys are sorted and wrap from the last to the first over midnight.
fn segment(len: usize, time: impl Fn(usize) -> u32, t: f32) -> Option<(usize, usize, f32)> {
    match len {
        0 => None,
        1 => Some((0, 0, 0.0)),
        _ => {
            // The last key at or before t; before the first key, the last one (yesterday).
            let a = (0..len)
                .rev()
                .find(|&i| time(i) as f32 <= t)
                .unwrap_or(len - 1);
            let b = (a + 1) % len;
            let (ta, mut tb, mut t) = (time(a) as f32, time(b) as f32, t);
            if tb <= ta {
                tb += DAY as f32;
            }
            if t < ta {
                t += DAY as f32;
            }
            Some((a, b, ((t - ta) / (tb - ta)).clamp(0.0, 1.0)))
        }
    }
}
fn lerp(a: f32, b: f32, f: f32) -> f32 {
    a + (b - a) * f
}
fn lerp3(a: Linear, b: Linear, f: f32) -> Linear {
    std::array::from_fn(|i| lerp(a[i], b[i], f))
}

/// Weighted parts over vanilla: each part's weight (they sum to at most 1)
/// and its value, None where it leaves vanilla; vanilla takes the rest.
pub fn blend<const N: usize>(
    vanilla: [f32; N],
    parts: impl IntoIterator<Item = (f32, Option<[f32; N]>)>,
) -> [f32; N] {
    let mut out = [0.0; N];
    let mut rest = 1.0f32;
    for (w, value) in parts {
        let value = value.unwrap_or(vanilla);
        for i in 0..N {
            out[i] += value[i] * w;
        }
        rest -= w;
    }
    let rest = rest.max(0.0);
    for i in 0..N {
        out[i] += vanilla[i] * rest;
    }
    out
}

impl FogSample {
    /// Colour then distances, for [`blend`].
    pub fn to_array(self) -> [f32; 5] {
        let [r, g, b] = self.color;
        [r, g, b, self.start, self.end]
    }
    pub fn from_array(a: [f32; 5]) -> Self {
        Self {
            color: [a[0], a[1], a[2]],
            start: a[3],
            end: a[4],
        }
    }
}
impl SkySample {
    /// The four colours one after another, for [`blend`].
    pub fn to_array(self) -> [f32; 12] {
        let c = [self.zenith, self.horizon, self.cloud, self.cloud_far];
        std::array::from_fn(|i| c[i / 3][i % 3])
    }
    pub fn from_array(a: [f32; 12]) -> Self {
        let c = |i: usize| [a[i * 3], a[i * 3 + 1], a[i * 3 + 2]];
        Self {
            zenith: c(0),
            horizon: c(1),
            cloud: c(2),
            cloud_far: c(3),
        }
    }
}

/// Checks a mix as the mod sent it: up to [`MIX_PROFILES`] distinct ids
/// below `defined`, finite weights >= 0 with a positive sum. Returns the
/// weights normalised to sum 1 (zero weights dropped) and the transition
/// clamped to 0..[`TRANSITION_MAX`].
pub fn check_mix(
    weights: &[(u32, f32)],
    transition: f32,
    defined: usize,
) -> Result<(Vec<(u32, f32)>, f32), String> {
    if !transition.is_finite() {
        return Err("transition-seconds must be finite".into());
    }
    let transition = transition.clamp(0.0, TRANSITION_MAX);
    if weights.len() > MIX_PROFILES {
        return Err(format!("at most {MIX_PROFILES} profiles in a mix"));
    }
    for (i, &(id, w)) in weights.iter().enumerate() {
        if id as usize >= defined {
            return Err(format!("no profile {id}"));
        }
        if !w.is_finite() || w < 0.0 {
            return Err("mix weights must be finite and >= 0".into());
        }
        if weights[..i].iter().any(|&(other, _)| other == id) {
            return Err(format!("profile {id} twice in the mix"));
        }
    }
    if weights.is_empty() {
        return Ok((Vec::new(), transition));
    }
    let sum: f32 = weights.iter().map(|&(_, w)| w).sum();
    if !(sum > 0.0 && sum.is_finite()) {
        return Err("mix weights must add up to more than 0".into());
    }
    let mix = weights
        .iter()
        .filter(|&&(_, w)| w > 0.0)
        .map(|&(id, w)| (id, w / sum))
        .collect();
    Ok((mix, transition))
}

/// The cross-fade between mixes: weights of profiles keyed by `K`, vanilla
/// taking what they leave. A new target fades from the weights shown now,
/// so a mix changed mid-fade does not jump.
#[derive(Clone, Debug)]
pub struct Fader<K> {
    from: Vec<(K, f32)>,
    to: Vec<(K, f32)>,
    elapsed: f32,
    duration: f32,
    target: Option<u64>,
}
impl<K> Default for Fader<K> {
    fn default() -> Self {
        Self {
            from: Vec::new(),
            to: Vec::new(),
            elapsed: 0.0,
            duration: 0.0,
            target: None,
        }
    }
}
impl<K: Clone + PartialEq> Fader<K> {
    /// The target's serial now (None: no target, vanilla).
    pub fn target(&self) -> Option<u64> {
        self.target
    }
    /// Fades to `to` (weights summing to at most 1) over `seconds`, unless
    /// `serial` is the target already.
    pub fn retarget(&mut self, serial: Option<u64>, to: Vec<(K, f32)>, seconds: f32) {
        if serial == self.target {
            return;
        }
        self.from = self.weights();
        self.to = to;
        self.elapsed = 0.0;
        self.duration = if seconds.is_finite() {
            seconds.clamp(0.0, TRANSITION_MAX)
        } else {
            0.0
        };
        self.target = serial;
    }
    /// Moves the fade on by `seconds` of real time.
    pub fn advance(&mut self, seconds: f32) {
        if seconds.is_finite() && seconds > 0.0 {
            self.elapsed = (self.elapsed + seconds).min(self.duration);
        }
        if self.elapsed >= self.duration {
            self.from.clear();
        }
    }
    /// How far the fade is, 0..1.
    pub fn progress(&self) -> f32 {
        if self.duration <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.duration).clamp(0.0, 1.0)
        }
    }
    /// The weights shown now; vanilla takes 1 minus their sum.
    pub fn weights(&self) -> Vec<(K, f32)> {
        let f = self.progress();
        let mut out: Vec<(K, f32)> = Vec::with_capacity(self.from.len() + self.to.len());
        let mut add = |k: &K, w: f32| {
            if w <= 0.0 {
                return;
            }
            match out.iter_mut().find(|(o, _)| o == k) {
                Some((_, v)) => *v += w,
                None => out.push((k.clone(), w)),
            }
        };
        if f < 1.0 {
            for (k, w) in &self.from {
                add(k, w * (1.0 - f));
            }
        }
        for (k, w) in &self.to {
            add(k, w * f);
        }
        out
    }
    /// Nothing shown and nothing coming: the frame is vanilla.
    pub fn is_vanilla(&self) -> bool {
        self.to.is_empty() && (self.from.is_empty() || self.progress() >= 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fog(day_time: u32, color: u32, start: f32, end: f32) -> FogKeyIn {
        FogKeyIn {
            day_time,
            color,
            start,
            end,
            relative: false,
        }
    }
    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn colours_round_trip_through_linear() {
        for c in [0u32, 1, 0x7f, 0xbc, 0xff] {
            let l = srgb_to_linear(c as f32 / 255.0);
            assert!(close(linear_to_srgb(l) * 255.0, c as f32), "{c}");
        }
        assert_eq!(linear(0xFF0000), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn keys_interpolate_and_wrap_over_midnight() {
        let p = Profile::new(
            &[
                fog(18000, 0x000000, 10.0, 50.0),
                fog(6000, 0xFFFFFF, 30.0, 150.0),
            ],
            &[],
            None,
        )
        .unwrap();
        // On a key.
        let s = p.sample(6000.0, 256.0).fog.unwrap();
        assert_eq!((s.start, s.end, s.color), (30.0, 150.0, [1.0; 3]));
        // Halfway from 6000 to 18000.
        let s = p.sample(12000.0, 256.0).fog.unwrap();
        assert!(close(s.start, 20.0) && close(s.end, 100.0) && close(s.color[0], 0.5));
        // From 18000 over midnight (24000 = 0) to 6000: 0 is halfway.
        let s = p.sample(0.0, 256.0).fog.unwrap();
        assert!(close(s.start, 20.0), "{s:?}");
        let s = p.sample(24000.0 * 3.0 + 21000.0, 256.0).fog.unwrap();
        assert!(close(s.start, 15.0), "{s:?}");
        let s = p.sample(-3000.0, 256.0).fog.unwrap();
        assert!(close(s.start, 15.0), "{s:?}");
        // 24000 is accepted as 0.
        assert!(Profile::new(&[fog(24000, 0, 1.0, 2.0), fog(0, 0, 1.0, 2.0)], &[], None).is_err());
        let one = Profile::new(&[fog(24000, 0x123456, 1.0, 2.0)], &[], None).unwrap();
        assert_eq!(one.sample(9999.0, 1.0).fog.unwrap().start, 1.0);
    }

    #[test]
    fn relative_fog_scales_with_the_render_distance() {
        let mut k = fog(0, 0, 0.25, 1.5);
        k.relative = true;
        let p = Profile::new(&[k], &[], None).unwrap();
        let s = p.sample(500.0, 200.0).fog.unwrap();
        assert_eq!((s.start, s.end), (50.0, 300.0));
    }

    #[test]
    fn sky_keys_and_missing_parts() {
        let sky = |t, c| SkyKeyIn {
            day_time: t,
            zenith: c,
            horizon: c,
            cloud: c,
            cloud_far: c,
        };
        let p = Profile::new(&[], &[sky(0, 0), sky(12000, 0xFFFFFF)], None).unwrap();
        let s = p.sample(6000.0, 100.0);
        assert!(s.fog.is_none() && s.water.is_none());
        assert!(close(s.sky.unwrap().cloud_far[2], 0.5));
        let w = Profile::new(
            &[],
            &[],
            Some(WaterFogIn {
                color: 0x0000FF,
                start: 2.0,
                end: 64.0,
            }),
        )
        .unwrap();
        assert_eq!(w.sample(0.0, 1.0).water.unwrap().end, 64.0);
    }

    #[test]
    fn profiles_are_checked() {
        let bad: [&[FogKeyIn]; 9] = [
            &[fog(0, 0x1000000, 1.0, 2.0)],
            &[fog(0, 0, f32::NAN, 2.0)],
            &[fog(0, 0, 1.0, f32::INFINITY)],
            &[fog(0, 0, -1.0, 2.0)],
            &[fog(0, 0, 1.0, 5000.0)],
            &[fog(0, 0, 3.0, 2.0)],
            &[fog(24001, 0, 1.0, 2.0)],
            &[fog(5, 0, 1.0, 2.0), fog(5, 0, 1.0, 2.0)],
            &[FogKeyIn {
                relative: true,
                ..fog(0, 0, 1.0, 2.5)
            }],
        ];
        for keys in bad {
            assert!(Profile::new(keys, &[], None).is_err(), "{keys:?}");
        }
        assert!(Profile::new(&[], &[], None).is_err());
        let many: Vec<_> = (0..33).map(|i| fog(i * 10, 0, 1.0, 2.0)).collect();
        assert!(Profile::new(&many, &[], None).is_err());
        assert!(Profile::new(&many[..32], &[], None).is_ok());
        let water = |start, end| {
            Some(WaterFogIn {
                color: 0,
                start,
                end,
            })
        };
        assert!(Profile::new(&[], &[], water(5.0, 1.0)).is_err());
        assert!(Profile::new(&[], &[], water(f32::NAN, 1.0)).is_err());
    }

    #[test]
    fn mixes_are_checked_and_normalised() {
        let (mix, t) = check_mix(&[(0, 1.0), (2, 3.0), (1, 0.0)], 99.0, 3).unwrap();
        assert_eq!(mix, [(0, 0.25), (2, 0.75)]);
        assert_eq!(t, TRANSITION_MAX);
        assert_eq!(check_mix(&[], -1.0, 0).unwrap(), (vec![], 0.0));
        for (weights, defined) in [
            (&[(3, 1.0)][..], 3),
            (&[(0, -1.0)][..], 3),
            (&[(0, f32::NAN)][..], 3),
            (&[(0, 0.0)][..], 3),
            (&[(0, 1.0), (0, 1.0)][..], 3),
            (&[(0, 1.0), (1, 1.0), (2, 1.0), (3, 1.0), (4, 1.0)][..], 9),
            (&[(0, f32::MAX), (1, f32::MAX)][..], 3),
        ] {
            assert!(check_mix(weights, 1.0, defined).is_err(), "{weights:?}");
        }
        assert!(check_mix(&[(0, 1.0)], f32::NAN, 1).is_err());
    }

    #[test]
    fn blending_leaves_vanilla_the_rest() {
        let out = blend([1.0, 1.0], [(0.25, Some([0.0, 0.0])), (0.25, None)]);
        assert_eq!(out, [0.75, 0.75]);
        assert_eq!(blend([0.5], []), [0.5]);
        let s = SkySample {
            zenith: [0.1, 0.2, 0.3],
            horizon: [0.4, 0.5, 0.6],
            cloud: [0.7, 0.8, 0.9],
            cloud_far: [1.0, 0.0, 0.5],
        };
        assert_eq!(SkySample::from_array(s.to_array()), s);
        let f = FogSample {
            color: [0.1, 0.2, 0.3],
            start: 4.0,
            end: 9.0,
        };
        assert_eq!(FogSample::from_array(f.to_array()), f);
    }

    #[test]
    fn the_fader_cross_fades_and_retargets_from_what_it_shows() {
        let mut f = Fader::<u32>::default();
        assert!(f.is_vanilla());
        f.retarget(Some(1), vec![(7, 1.0)], 2.0);
        assert!(f.weights().is_empty());
        f.advance(1.0);
        assert_eq!(f.weights(), [(7, 0.5)]);
        // The same serial again changes nothing.
        f.retarget(Some(1), vec![(8, 1.0)], 0.0);
        assert_eq!(f.weights(), [(7, 0.5)]);
        // Mid-fade to another mix: starts from 0.5 of 7.
        f.retarget(Some(2), vec![(8, 0.5), (7, 0.5)], 4.0);
        assert_eq!(f.weights(), [(7, 0.5)]);
        f.advance(2.0);
        let w = f.weights();
        assert!(w.iter().any(|&(k, v)| k == 7 && close(v, 0.5)), "{w:?}");
        assert!(w.iter().any(|&(k, v)| k == 8 && close(v, 0.25)), "{w:?}");
        f.advance(100.0);
        assert_eq!(f.weights(), [(8, 0.5), (7, 0.5)]);
        assert!(!f.is_vanilla());
        // Back to vanilla at once.
        f.retarget(None, vec![], 0.0);
        f.advance(0.0);
        assert!(f.weights().is_empty() && f.is_vanilla());
        // Bad times count as no fade.
        f.retarget(Some(3), vec![(1, 1.0)], f32::NAN);
        assert_eq!(f.weights(), [(1, 1.0)]);
        f.advance(f32::NAN);
        assert_eq!(f.weights(), [(1, 1.0)]);
    }
}
