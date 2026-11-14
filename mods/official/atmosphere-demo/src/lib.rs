//! Capability `atmosphere` (API level 33) in a few lines: three profiles
//! keyed by time of day, mixed by the weather twice a second, and `/atmo`
//! to pick one, define a plain fog colour, read the conditions or start a
//! particle emitter of glowing motes around the camera (`/atmo emit`), or
//! put an aurora, meteors and two far bodies on the sky (`/atmo sky`).
use horizon_mod_sdk::{
    Mod, Registration,
    atmosphere::{
        self, EmitterShape, FogKey, FrameMode, Medium, Profile, SkyKey, SkySpace, SkySprite,
        WaterFog,
    },
    atmosphere_fx::{constant, effect, emitter, fade, flat, particle, span, span3},
    atmosphere_mix::{clear, rgb, show},
    atmosphere_sky::{DISC, STREAK, VEIL, emissive, sprite},
    chat,
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// Misty by day, a storm mix as it rains.
    Auto,
    /// One profile alone.
    Fixed(u32),
    Off,
}

struct Demo {
    misty: u32,
    storm: u32,
    crimson: u32,
    mode: Mode,
    /// The rain level the last mix was made for, in tenths.
    rain: Option<u8>,
    /// The motes effect, defined on the first `/atmo emit`, and its live emitter.
    motes: Option<u32>,
    emitter: Option<u32>,
    /// `/atmo sky`: the lasting sprites, the meteors in flight and when each burns
    /// out (tick), the next meteor's tick and key, and a small random state.
    sky: Option<Sky>,
    /// The last tick seen.
    tick: u64,
}

struct Sky {
    lasting: Vec<SkySprite>,
    meteors: Vec<(SkySprite, u64)>,
    next_meteor: u64,
    next_key: u32,
    seed: u32,
}

impl Sky {
    fn random(&mut self) -> f32 {
        // Numerical Recipes' LCG: plenty for where a meteor falls.
        self.seed = self
            .seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        (self.seed >> 8) as f32 / (1 << 24) as f32
    }
    /// A meteor streaking down-sky for a second: its head (the streak's bright right end)
    /// leads, turned along its path.
    fn meteor(&mut self, tick: u64) {
        let yaw = self.random() * 360.0;
        let elevation = 35.0 + self.random() * 40.0;
        let drift_yaw = (self.random() - 0.5) * 50.0;
        let drift_elevation = -18.0 - self.random() * 14.0;
        let mut m = emissive(sprite(self.next_key, STREAK, yaw, elevation, 14.0));
        m.height = 1.0;
        m.roll = drift_elevation
            .atan2(drift_yaw * elevation.to_radians().cos())
            .to_degrees();
        m.tint = rgb(0xFF, 0xF0, 0xD0);
        m.drift_yaw = drift_yaw;
        m.drift_elevation = drift_elevation;
        m.lifetime = 1.2;
        m.fade_in = 0.1;
        m.fade_out = 0.5;
        self.next_key = self.next_key.wrapping_add(1).max(1000);
        self.meteors.push((m, tick + 25));
    }
    fn show(&self) -> Result<(), String> {
        let mut all = self.lasting.clone();
        all.extend(self.meteors.iter().map(|(m, _)| m.clone()));
        atmosphere::set_sky_sprites(&all)
    }
}

/// An aurora of twelve green to violet veils low in the north, slowly drifting; a dark
/// second moon (alpha blended, not adding light); and vanilla's sun far off as a small
/// warm body turning with the stars.
fn lasting_sky() -> Vec<SkySprite> {
    let mut out = Vec::new();
    let tints = [
        rgb(0x40, 0xFF, 0x90),
        rgb(0x30, 0xE0, 0xC0),
        rgb(0x50, 0xFF, 0x70),
        rgb(0xA0, 0x60, 0xFF),
    ];
    for i in 0..12u32 {
        let mut v = emissive(sprite(i, VEIL, 140.0 + i as f32 * 7.0, 22.0, 12.0));
        v.height = 34.0;
        v.elevation = 20.0 + (i % 3) as f32 * 3.0;
        v.roll = (i as f32 * 1.7).sin() * 6.0;
        v.tint = tints[i as usize % tints.len()];
        v.alpha = 0.6;
        v.drift_yaw = 0.6;
        v.fade_in = 3.0;
        out.push(v);
    }
    let mut moon = sprite(100, DISC, 200.0, 50.0, 5.0);
    moon.tint = rgb(0x20, 0x28, 0x38);
    moon.alpha = 0.85;
    out.push(moon);
    let mut sun = emissive(sprite(
        101,
        "minecraft:environment/celestial/sun",
        60.0,
        20.0,
        6.0,
    ));
    sun.tint = rgb(0xFF, 0xB0, 0x80);
    sun.space = SkySpace::Stars;
    out.push(sun);
    out
}

/// Warm motes drifting up in a 12-block box around the camera, swaying, fading
/// in and out and away close to the camera. Vanilla's end rod sprites stand in
/// for the mod's own (a mod with capability `registries` can ship its own set).
fn define_motes() -> Result<u32, String> {
    let mut e = emitter(40.0);
    e.shape = EmitterShape::Box;
    e.half_size = (12.0, 4.0, 12.0);
    e.offset = span3(0.0, 0.0);
    e.offset.y = constant(2.0);
    e.max = 400;
    e.direction = Some(atmosphere::Span3 {
        x: span(-0.3, 0.3),
        y: constant(1.0),
        z: span(-0.3, 0.3),
    });
    let mut p = particle("minecraft:end_rod");
    p.frame = FrameMode::OverLife;
    p.lifetime = span(3.0, 6.0);
    p.speed = span(0.2, 0.6);
    p.drag = 0.4;
    p.gravity = -0.05;
    let sway = |period: f32| atmosphere::Curve {
        base: constant(0.0),
        keys: Vec::new(),
        ease: atmosphere::Ease::Linear,
        wave: Some(atmosphere::Wave {
            amplitude: constant(0.3),
            period: span(period, period * 1.5),
            phase: span(0.0, 1.0),
        }),
    };
    p.acceleration = Some(atmosphere::Curve3 {
        x: sway(3.0),
        y: flat(constant(0.0)),
        z: sway(3.5),
    });
    p.size = fade(span(0.08, 0.16), 0.2, 0.3);
    p.alpha = fade(constant(1.0), 0.2, 0.3);
    p.color_keys = vec![
        atmosphere::ColorKey {
            t: 0.0,
            color: rgb(0xFF, 0xE0, 0x90),
        },
        atmosphere::ColorKey {
            t: 1.0,
            color: rgb(0xFF, 0x90, 0x40),
        },
    ];
    p.shade = 0.3;
    p.fade_near = Some(span(0.5, 2.0));
    p.emissive = true;
    p.translucent = true;
    p.only_in = Medium::Air;
    atmosphere::define_effect(&effect(e, p))
}

fn fog(day_time: u32, color: u32, start: f32, end: f32) -> FogKey {
    FogKey {
        day_time,
        color,
        start,
        end,
        relative: false,
    }
}
fn sky(day_time: u32, zenith: u32, horizon: u32, cloud: u32) -> SkyKey {
    SkyKey {
        day_time,
        zenith,
        horizon,
        cloud,
        cloud_far: zenith,
    }
}

fn define() -> Result<(u32, u32, u32), String> {
    let misty = atmosphere::define_profile(&Profile {
        fog_keys: vec![
            fog(0, rgb(0xE8, 0xC8, 0xA0), 8.0, 96.0),
            fog(6000, rgb(0xC8, 0xD4, 0xE0), 24.0, 160.0),
            fog(12000, rgb(0xE0, 0x90, 0x70), 8.0, 96.0),
            fog(18000, rgb(0x10, 0x14, 0x28), 4.0, 64.0),
        ],
        sky_keys: vec![
            sky(
                0,
                rgb(0x70, 0x90, 0xC0),
                rgb(0xF0, 0xC0, 0x90),
                rgb(0xFF, 0xE0, 0xC0),
            ),
            sky(
                6000,
                rgb(0x60, 0x90, 0xE0),
                rgb(0xC0, 0xD8, 0xF0),
                rgb(0xFF, 0xFF, 0xFF),
            ),
            sky(
                12000,
                rgb(0x50, 0x50, 0x90),
                rgb(0xF0, 0x80, 0x50),
                rgb(0xFF, 0xA0, 0x80),
            ),
            sky(
                18000,
                rgb(0x02, 0x04, 0x10),
                rgb(0x08, 0x0C, 0x20),
                rgb(0x20, 0x20, 0x30),
            ),
        ],
        water_fog: Some(WaterFog {
            color: rgb(0x10, 0x40, 0x60),
            start: 2.0,
            end: 48.0,
        }),
    })?;
    let storm = atmosphere::define_profile(&Profile {
        fog_keys: vec![
            fog(6000, rgb(0x50, 0x58, 0x60), 2.0, 48.0),
            fog(18000, rgb(0x08, 0x0A, 0x10), 2.0, 32.0),
        ],
        sky_keys: vec![
            sky(
                6000,
                rgb(0x40, 0x48, 0x50),
                rgb(0x58, 0x60, 0x68),
                rgb(0x60, 0x64, 0x68),
            ),
            sky(
                18000,
                rgb(0x04, 0x04, 0x08),
                rgb(0x08, 0x08, 0x0C),
                rgb(0x10, 0x10, 0x14),
            ),
        ],
        water_fog: None,
    })?;
    let crimson = atmosphere::define_profile(&Profile {
        fog_keys: vec![fog(0, rgb(0xFF, 0x20, 0x20), 0.0, 24.0)],
        sky_keys: vec![sky(
            0,
            rgb(0x60, 0x00, 0x00),
            rgb(0xFF, 0x20, 0x20),
            rgb(0xFF, 0x80, 0x80),
        )],
        water_fog: None,
    })?;
    Ok((misty, storm, crimson))
}

impl Demo {
    fn apply(&mut self, seconds: f32) -> Result<(), String> {
        match self.mode {
            Mode::Off => clear(seconds),
            Mode::Fixed(id) => show(id, seconds),
            Mode::Auto => {
                let rain = atmosphere::conditions().map_or(0.0, |c| c.rain.max(c.thunder));
                self.rain = Some((rain * 10.0).round() as u8);
                if rain <= 0.0 {
                    show(self.misty, seconds)
                } else {
                    atmosphere::set_mix(&[(self.misty, 1.0 - rain), (self.storm, rain)], seconds)
                }
            }
        }
    }
}

impl Mod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        let (misty, storm, crimson) = define()?;
        let mut demo = Demo {
            misty,
            storm,
            crimson,
            mode: Mode::Auto,
            rain: None,
            motes: None,
            emitter: None,
            sky: None,
            tick: 0,
        };
        demo.apply(0.0)?;
        Ok((demo, Registration::new().on_tick().command("atmo")))
    }
    fn tick(&mut self, tick: u64) {
        self.tick = tick;
        // The sky's meteors: one every two to five seconds, gone after their second.
        if let Some(sky) = self.sky.as_mut() {
            let before = sky.meteors.len();
            sky.meteors.retain(|(_, end)| *end > tick);
            let mut changed = sky.meteors.len() != before;
            if tick >= sky.next_meteor {
                sky.meteor(tick);
                sky.next_meteor = tick + 40 + (sky.random() * 60.0) as u64;
                changed = true;
            }
            if changed {
                let _ = sky.show();
            }
        }
        // Twice a second, and only when the rain moved by a tenth.
        if self.mode != Mode::Auto || tick % 10 != 0 {
            return;
        }
        let rain = atmosphere::conditions().map(|c| (c.rain.max(c.thunder) * 10.0).round() as u8);
        if rain.is_some() && rain != self.rain {
            let _ = self.apply(2.0);
        }
    }
    fn command(&mut self, _name: &str, args: &str) -> Result<Option<String>, String> {
        let mut words = args.split_whitespace();
        let reply = match words.next().unwrap_or("") {
            "auto" => {
                self.mode = Mode::Auto;
                self.rain = None;
                "misty, stormy in rain"
            }
            "misty" => {
                self.mode = Mode::Fixed(self.misty);
                "misty"
            }
            "storm" => {
                self.mode = Mode::Fixed(self.storm);
                "storm"
            }
            "crimson" => {
                self.mode = Mode::Fixed(self.crimson);
                "crimson"
            }
            "off" => {
                self.mode = Mode::Off;
                "vanilla"
            }
            // `/atmo fog RRGGBB START END`: a new plain profile, shown at once.
            "fog" => {
                let color = words
                    .next()
                    .and_then(|w| u32::from_str_radix(w.trim_start_matches('#'), 16).ok())
                    .ok_or("usage: /atmo fog RRGGBB START END")?;
                let mut number = || words.next().and_then(|w| w.parse::<f32>().ok());
                let (start, end) = (number().unwrap_or(0.0), number().unwrap_or(32.0));
                let id = atmosphere::define_profile(&Profile {
                    fog_keys: vec![fog(0, color, start, end)],
                    sky_keys: vec![sky(0, color, color, color)],
                    water_fog: Some(WaterFog { color, start, end }),
                })?;
                self.mode = Mode::Fixed(id);
                show(id, 0.0)?;
                return Ok(Some(format!(
                    "profile {id}: fog #{color:06x} {start}..{end}"
                )));
            }
            // `/atmo emit [DENSITY]`: motes around the camera; `/atmo emit stop`.
            "emit" => {
                if let Some(id) = self.emitter.take() {
                    let _ = atmosphere::stop_emitter(id, true);
                }
                let arg = words.next().unwrap_or("1");
                if arg == "stop" {
                    return Ok(Some("emitter stopped".into()));
                }
                let density: f32 = arg
                    .parse()
                    .map_err(|_| "usage: /atmo emit [DENSITY|stop]")?;
                let motes = match self.motes {
                    Some(id) => id,
                    None => *self.motes.insert(define_motes()?),
                };
                let id = atmosphere::start_emitter(
                    motes,
                    atmosphere::Anchor::Camera,
                    density,
                    0xFFFFFF,
                )?;
                self.emitter = Some(id);
                return Ok(Some(format!("emitter {id}: motes at density {density}")));
            }
            // `/atmo sky`: aurora, meteors, a dark moon and a far sun; `/atmo sky off`.
            "sky" => {
                if words.next() == Some("off") {
                    self.sky = None;
                    atmosphere::set_sky_sprites(&[])?;
                    return Ok(Some("sky sprites off".into()));
                }
                let mut sky = Sky {
                    lasting: lasting_sky(),
                    meteors: Vec::new(),
                    next_meteor: self.tick + 40,
                    next_key: 1000,
                    seed: 0x5EED,
                };
                sky.meteor(self.tick);
                sky.meteor(self.tick);
                sky.show()?;
                let count = sky.lasting.len() + sky.meteors.len();
                self.sky = Some(sky);
                return Ok(Some(format!(
                    "sky: {count} sprites, meteors every few seconds"
                )));
            }
            "conditions" => {
                let text = match atmosphere::conditions() {
                    None => "no conditions".to_owned(),
                    Some(c) => format!(
                        "rain {:.2} thunder {:.2} day {} moon {} y {:.1} light {}/{} sky {}",
                        c.rain,
                        c.thunder,
                        c.day_time,
                        c.moon_phase,
                        c.camera_y,
                        c.sky_light,
                        c.block_light,
                        c.sky_visible
                    ),
                };
                chat::show(&text);
                return Ok(None);
            }
            _ => {
                return Err(
                    "usage: /atmo auto|misty|storm|crimson|off|conditions|emit [DENSITY|stop]|sky [off]|fog RRGGBB START END"
                        .into(),
                );
            }
        };
        let seconds = if args.split_whitespace().nth(1) == Some("now") {
            0.0
        } else {
            2.0
        };
        self.apply(seconds)?;
        Ok(Some(format!("atmosphere: {reply}")))
    }
}

horizon_mod_sdk::export_mod!(Demo);
