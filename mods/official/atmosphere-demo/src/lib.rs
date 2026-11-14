//! Capability `atmosphere` (API level 33) in a few lines: three profiles
//! keyed by time of day, mixed by the weather twice a second, and `/atmo`
//! to pick one, define a plain fog colour, read the conditions or start a
//! particle emitter of glowing motes around the camera (`/atmo emit`).
use horizon_mod_sdk::{
    Mod, Registration,
    atmosphere::{self, EmitterShape, FogKey, FrameMode, Medium, Profile, SkyKey, WaterFog},
    atmosphere_fx::{constant, effect, emitter, fade, flat, particle, span, span3},
    atmosphere_mix::{clear, rgb, show},
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
        };
        demo.apply(0.0)?;
        Ok((demo, Registration::new().on_tick().command("atmo")))
    }
    fn tick(&mut self, tick: u64) {
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
                    "usage: /atmo auto|misty|storm|crimson|off|conditions|emit [DENSITY|stop]|fog RRGGBB START END"
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
