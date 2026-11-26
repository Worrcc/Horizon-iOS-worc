//! Capability `atmosphere`'s sky sprites (API level 33) without the runtime:
//! the checks on a mod's `set-sky-sprites` list, and where a sprite is and
//! how strongly it shows at an age, which the game works out every frame
//! (`docs/api-expansion-d.md`). The mod sets the list when it changes; the
//! drift, spin, lifetime and fades are the game's.

/// Sprites one mod may show at once.
pub const SPRITES_PER_MOD: usize = 256;
/// `set-sky-sprites` calls a mod may make in 20 ticks; later ones fail.
pub const CALLS_PER_SECOND: usize = 20;
/// The widest and tallest sprite, in degrees.
pub const ANGLE_MAX: f32 = 120.0;
/// The fastest drift or spin, in degrees per second.
pub const RATE_MAX: f32 = 360.0;
/// The shortest and longest lifetime, in seconds (0 is forever).
pub const LIFETIME_MIN: f32 = 0.05;
pub const LIFETIME_MAX: f32 = 600.0;
/// The longest fade-in of a sprite without a lifetime, in seconds.
pub const FADE_IN_MAX: f32 = 60.0;
/// The longest texture name.
pub const TEXTURE_NAME_MAX: usize = 128;
/// The sky dome's radius, as the stars' (`SkyRenderer.buildStars`).
pub const RADIUS: f32 = 100.0;
/// Horizon's own sky textures, made by the game: any mod may name them.
pub const BUILT_IN: [&str; 4] = [
    "horizon:sky/glow",
    "horizon:sky/disc",
    "horizon:sky/streak",
    "horizon:sky/veil",
];

/// How a sprite goes over the sky (WIT `sky-blend`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    Translucent,
    Emissive,
}

/// What a sprite's direction is fixed to (WIT `sky-space`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    Compass,
    Stars,
}

/// One sky sprite as the mod sent it (WIT `sky-sprite`); [`check_list`]
/// keeps only valid ones.
#[derive(Clone, Debug, PartialEq)]
pub struct SkySprite {
    pub key: u32,
    pub texture: String,
    pub uv: [f32; 4],
    pub yaw: f32,
    pub elevation: f32,
    pub width: f32,
    pub height: f32,
    pub roll: f32,
    pub tint: u32,
    pub alpha: f32,
    pub blend: Blend,
    pub space: Space,
    /// Degrees per second added to yaw, elevation and roll.
    pub drift: [f32; 3],
    pub lifetime: f32,
    pub fade_in: f32,
    pub fade_out: f32,
}

/// A sprite at one age: its direction and corners on the dome (radius
/// [`RADIUS`], world axes, or the stars' frame for [`Space::Stars`]), the
/// colour it multiplies its texels by, and the angle from its direction to
/// its farthest corner (for culling).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub direction: [f32; 3],
    /// Bottom left, bottom right, top right, top left as seen from inside
    /// the dome, for the texture's (u0, v1), (u1, v1), (u1, v0), (u0, v0).
    pub corners: [[f32; 3]; 4],
    /// The tint (0..1, as the frame's colours) and the faded alpha.
    pub rgba: [f32; 4],
    pub reach_degrees: f32,
}

/// The texture name's namespace, when it is `<namespace>:<path>` with
/// vanilla's resource-location characters.
pub fn texture_namespace(name: &str) -> Option<&str> {
    let (ns, path) = name.split_once(':')?;
    let ok_ns = !ns.is_empty()
        && ns
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-'));
    let ok_path = !path.is_empty()
        && !path.starts_with('/')
        && !path.ends_with('/')
        && !path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
        && path
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'/'));
    (ok_ns && ok_path).then_some(ns)
}

/// The resource path a texture name stands for (`assets/<ns>/textures/<path>.png`).
pub fn texture_path(name: &str) -> Option<String> {
    let (ns, path) = name.split_once(':')?;
    texture_namespace(name)?;
    Some(format!("assets/{ns}/textures/{path}.png"))
}

fn finite_in(name: &str, v: f32, lo: f32, hi: f32) -> Result<(), String> {
    if v.is_finite() && (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(format!("{name} must be {lo}..{hi}, not {v}"))
    }
}

impl SkySprite {
    /// The sprite when every field is in range and its texture is one the
    /// mod may name (`own` says whether a namespace is the mod's).
    pub fn checked(self, own: &dyn Fn(&str) -> bool) -> Result<Self, String> {
        let t = &self.texture;
        if t.len() > TEXTURE_NAME_MAX {
            return Err(format!("texture name longer than {TEXTURE_NAME_MAX}"));
        }
        let ns = texture_namespace(t)
            .ok_or_else(|| format!("texture {t:?} is not <namespace>:<path>"))?;
        if ns == "horizon" {
            if !BUILT_IN.contains(&t.as_str()) {
                return Err(format!(
                    "texture {t}: Horizon's own sky textures are {}",
                    BUILT_IN.join(", ")
                ));
            }
        } else if ns != "minecraft" && !own(ns) {
            return Err(format!(
                "texture {t}: namespace {ns} is not this mod's (use its own content, minecraft: or horizon:sky/...)"
            ));
        }
        for (name, v) in ["u0", "v0", "u1", "v1"].into_iter().zip(self.uv) {
            finite_in(name, v, 0.0, 1.0)?;
        }
        if !self.yaw.is_finite() || !self.roll.is_finite() {
            return Err("yaw and roll must be finite".into());
        }
        finite_in("elevation", self.elevation, -90.0, 90.0)?;
        for (name, v) in [("width", self.width), ("height", self.height)] {
            finite_in(name, v, 0.0, ANGLE_MAX)?;
            if v <= 0.0 {
                return Err(format!("{name} must be above 0"));
            }
        }
        if self.tint > 0xFF_FFFF {
            return Err(format!("tint {:#x} is not 0xRRGGBB", self.tint));
        }
        finite_in("alpha", self.alpha, 0.0, 1.0)?;
        for (name, v) in ["drift-yaw", "drift-elevation", "spin"]
            .into_iter()
            .zip(self.drift)
        {
            finite_in(name, v, -RATE_MAX, RATE_MAX)?;
        }
        if self.lifetime == 0.0 {
            finite_in("fade-in", self.fade_in, 0.0, FADE_IN_MAX)?;
            if self.fade_out != 0.0 {
                return Err("fade-out needs a lifetime".into());
            }
        } else {
            finite_in("lifetime", self.lifetime, LIFETIME_MIN, LIFETIME_MAX)?;
            finite_in("fade-in", self.fade_in, 0.0, self.lifetime)?;
            finite_in("fade-out", self.fade_out, 0.0, self.lifetime)?;
        }
        Ok(self)
    }

    /// How much of its alpha the sprite shows `age` seconds after its key
    /// first showed; none once its lifetime is over.
    pub fn fade(&self, age: f32) -> Option<f32> {
        let age = age.max(0.0);
        if self.lifetime > 0.0 && age >= self.lifetime {
            return None;
        }
        let fade_in = if self.fade_in > 0.0 {
            (age / self.fade_in).min(1.0)
        } else {
            1.0
        };
        let fade_out = if self.lifetime > 0.0 && self.fade_out > 0.0 {
            ((self.lifetime - age) / self.fade_out).min(1.0)
        } else {
            1.0
        };
        Some(fade_in.min(fade_out))
    }

    /// Where the sprite is and how it shows `age` seconds after its key
    /// first showed; none when it is gone or fully transparent.
    pub fn place(&self, age: f32) -> Option<Placed> {
        let alpha = self.alpha * self.fade(age)?;
        if alpha <= 0.0 {
            return None;
        }
        let age = age.max(0.0);
        let yaw = (self.yaw + self.drift[0] * age).to_radians();
        let elevation = (self.elevation + self.drift[1] * age).to_radians();
        let roll = (self.roll + self.drift[2] * age).to_radians();
        let (sy, cy) = yaw.sin_cos();
        let (se, ce) = elevation.sin_cos();
        // The player's facing at this yaw and an elevation (Entity.calculateViewVector,
        // pitch up positive): south at 0, west at 90.
        let direction = [-sy * ce, se, cy * ce];
        // The viewer's right and up: right stays level, so the zenith has a frame too.
        let right0 = [-cy, 0.0, -sy];
        let up0 = cross(right0, direction);
        let (sr, cr) = roll.sin_cos();
        let right = add(scale(right0, cr), scale(up0, sr));
        let up = add(scale(up0, cr), scale(right0, -sr));
        let half_w = (self.width * 0.5).to_radians().tan() * RADIUS;
        let half_h = (self.height * 0.5).to_radians().tan() * RADIUS;
        let center = scale(direction, RADIUS);
        let corner =
            |a: f32, b: f32| add(center, add(scale(right, a * half_w), scale(up, b * half_h)));
        let channel = |shift: u32| ((self.tint >> shift) & 0xFF) as f32 / 255.0;
        Some(Placed {
            direction,
            corners: [
                corner(-1.0, -1.0),
                corner(1.0, -1.0),
                corner(1.0, 1.0),
                corner(-1.0, 1.0),
            ],
            rgba: [channel(16), channel(8), channel(0), alpha],
            reach_degrees: (half_w.hypot(half_h) / RADIUS).atan().to_degrees(),
        })
    }
}

/// The list when it fits the caps and every sprite is valid (errors name the
/// sprite's index); no two sprites may share a key.
pub fn check_list(
    sprites: Vec<SkySprite>,
    own: &dyn Fn(&str) -> bool,
) -> Result<Vec<SkySprite>, String> {
    if sprites.len() > SPRITES_PER_MOD {
        return Err(format!("at most {SPRITES_PER_MOD} sky sprites per mod"));
    }
    let mut keys = std::collections::HashSet::with_capacity(sprites.len());
    sprites
        .into_iter()
        .enumerate()
        .map(|(i, s)| {
            if !keys.insert(s.key) {
                return Err(format!("sky sprite {i}: key {} used twice", s.key));
            }
            s.checked(own).map_err(|e| format!("sky sprite {i}: {e}"))
        })
        .collect()
}

/// The camera's facing from the player's yaw and pitch (pitch down positive).
pub fn forward(yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = yaw.to_radians().sin_cos();
    let (sp, cp) = pitch.to_radians().sin_cos();
    [-sy * cp, -sp, cy * cp]
}

/// Whether a sprite reaching `reach` degrees from `direction` can be on a
/// screen facing `forward` whose corners are `half_diagonal` degrees off its
/// centre.
pub fn in_view(direction: [f32; 3], reach: f32, forward: [f32; 3], half_diagonal: f32) -> bool {
    let limit = reach + half_diagonal;
    if limit >= 180.0 {
        return true;
    }
    dot(direction, forward) >= limit.to_radians().cos()
}

/// The angle from the screen's centre to its corner, for [`in_view`].
pub fn half_diagonal(fov_degrees: f32, aspect: f32) -> f32 {
    let t = (fov_degrees * 0.5).to_radians().tan();
    (t * t * (1.0 + aspect * aspect)).sqrt().atan().to_degrees()
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    a.map(|v| v * s)
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sprite() -> SkySprite {
        SkySprite {
            key: 1,
            texture: "horizon:sky/glow".into(),
            uv: [0.0, 0.0, 1.0, 1.0],
            yaw: 0.0,
            elevation: 0.0,
            width: 10.0,
            height: 10.0,
            roll: 0.0,
            tint: 0xFFFFFF,
            alpha: 1.0,
            blend: Blend::Emissive,
            space: Space::Compass,
            drift: [0.0; 3],
            lifetime: 0.0,
            fade_in: 0.0,
            fade_out: 0.0,
        }
    }
    fn mine(ns: &str) -> bool {
        ns == "mymod"
    }
    fn near(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn directions_follow_the_players_compass_and_elevation() {
        let at = |yaw: f32, elevation: f32| {
            SkySprite {
                yaw,
                elevation,
                ..sprite()
            }
            .place(0.0)
            .unwrap()
            .direction
        };
        assert!(near(at(0.0, 0.0), [0.0, 0.0, 1.0]), "south");
        assert!(near(at(90.0, 0.0), [-1.0, 0.0, 0.0]), "west");
        assert!(near(at(180.0, 0.0), [0.0, 0.0, -1.0]), "north");
        assert!(near(at(-90.0, 0.0), [1.0, 0.0, 0.0]), "east");
        assert!(near(at(37.0, 90.0), [0.0, 1.0, 0.0]), "zenith");
        assert!(near(at(0.0, 30.0), [0.0, 0.5, 0.866_025]));
        // The camera's own facing agrees (player pitch is down positive).
        assert!(near(forward(90.0, -30.0), at(90.0, 30.0)));
    }

    #[test]
    fn corners_span_the_angular_size_unmirrored_and_roll_turns_them() {
        let s = SkySprite {
            width: 20.0,
            height: 10.0,
            ..sprite()
        };
        let p = s.place(0.0).unwrap();
        // Facing south, the viewer's right is west (-x) and up is +y.
        let [bl, br, tr, tl] = p.corners;
        let half_w = 10f32.to_radians().tan() * RADIUS;
        let half_h = 5f32.to_radians().tan() * RADIUS;
        assert!(near(bl, [half_w, -half_h, RADIUS]));
        assert!(near(br, [-half_w, -half_h, RADIUS]));
        assert!(near(tr, [-half_w, half_h, RADIUS]));
        assert!(near(tl, [half_w, half_h, RADIUS]));
        let reach = (half_w.hypot(half_h) / RADIUS).atan().to_degrees();
        assert!((p.reach_degrees - reach).abs() < 1e-4);
        // A quarter turn: the bottom-left corner goes to where the bottom right was
        // as a tall sprite would put it.
        let rolled = SkySprite { roll: 90.0, ..s }.place(0.0).unwrap();
        assert!(
            near(rolled.corners[0], [-half_h, -half_w, RADIUS]),
            "{:?}",
            rolled.corners
        );
        // The zenith keeps a frame: its corners are distinct and level.
        let top = SkySprite {
            elevation: 90.0,
            ..sprite()
        }
        .place(0.0)
        .unwrap();
        assert!(top.corners.iter().all(|c| (c[1] - RADIUS).abs() < 1e-3));
        assert!(!near(top.corners[0], top.corners[2]));
    }

    #[test]
    fn drift_and_spin_move_the_sprite_with_its_age() {
        let s = SkySprite {
            drift: [90.0, 15.0, 45.0],
            ..sprite()
        };
        let later = s.place(2.0).unwrap();
        let expected = SkySprite {
            yaw: 180.0,
            elevation: 30.0,
            roll: 90.0,
            drift: [0.0; 3],
            ..s.clone()
        }
        .place(0.0)
        .unwrap();
        assert!(near(later.direction, expected.direction));
        assert!(near(later.corners[0], expected.corners[0]));
        // Over the zenith it goes on down the far side.
        let over = SkySprite {
            elevation: 80.0,
            drift: [0.0, 20.0, 0.0],
            ..sprite()
        }
        .place(1.0)
        .unwrap();
        assert!(near(over.direction, [0.0, 0.984_808, -0.173_648]));
    }

    #[test]
    fn lifetime_and_fades_shape_the_alpha() {
        let s = SkySprite {
            alpha: 0.8,
            lifetime: 4.0,
            fade_in: 1.0,
            fade_out: 2.0,
            tint: 0xFF8000,
            ..sprite()
        };
        let alpha = |age: f32| s.place(age).map(|p| p.rgba[3]);
        assert_eq!(alpha(0.0), None, "fully transparent at its first instant");
        assert!((alpha(0.5).unwrap() - 0.4).abs() < 1e-6);
        assert!((alpha(1.5).unwrap() - 0.8).abs() < 1e-6);
        assert!((alpha(3.0).unwrap() - 0.4).abs() < 1e-6);
        assert_eq!(alpha(4.0), None);
        assert_eq!(alpha(100.0), None);
        let rgba = s.place(2.0).unwrap().rgba;
        assert!(near([rgba[0], rgba[1], rgba[2]], [1.0, 128.0 / 255.0, 0.0]));
        // Forever, with a fade-in only.
        let forever = SkySprite {
            fade_in: 2.0,
            ..sprite()
        };
        assert_eq!(forever.fade(1.0), Some(0.5));
        assert_eq!(forever.fade(1e6), Some(1.0));
    }

    #[test]
    fn checks_name_the_field_and_the_texture_rules() {
        assert!(sprite().checked(&mine).is_ok());
        for texture in [
            "minecraft:environment/celestial/sun",
            "mymod:sky/aurora",
            "horizon:sky/veil",
        ] {
            let s = SkySprite {
                texture: texture.into(),
                ..sprite()
            };
            assert!(s.checked(&mine).is_ok(), "{texture}");
        }
        let cases: Vec<(SkySprite, &str)> = vec![
            (
                SkySprite {
                    texture: "other:sky/a".into(),
                    ..sprite()
                },
                "not this mod's",
            ),
            (
                SkySprite {
                    texture: "horizon:sky/moon".into(),
                    ..sprite()
                },
                "Horizon's own",
            ),
            (
                SkySprite {
                    texture: "noname".into(),
                    ..sprite()
                },
                "<namespace>:<path>",
            ),
            (
                SkySprite {
                    texture: "minecraft:../x".into(),
                    ..sprite()
                },
                "<namespace>:<path>",
            ),
            (
                SkySprite {
                    texture: "minecraft:Sun".into(),
                    ..sprite()
                },
                "<namespace>:<path>",
            ),
            (
                SkySprite {
                    texture: format!("minecraft:{}", "a".repeat(130)),
                    ..sprite()
                },
                "longer",
            ),
            (
                SkySprite {
                    uv: [0.0, 0.0, 1.5, 1.0],
                    ..sprite()
                },
                "u1",
            ),
            (
                SkySprite {
                    yaw: f32::NAN,
                    ..sprite()
                },
                "finite",
            ),
            (
                SkySprite {
                    elevation: 91.0,
                    ..sprite()
                },
                "elevation",
            ),
            (
                SkySprite {
                    width: 0.0,
                    ..sprite()
                },
                "width",
            ),
            (
                SkySprite {
                    height: 121.0,
                    ..sprite()
                },
                "height",
            ),
            (
                SkySprite {
                    tint: 0x1000000,
                    ..sprite()
                },
                "tint",
            ),
            (
                SkySprite {
                    alpha: f32::INFINITY,
                    ..sprite()
                },
                "alpha",
            ),
            (
                SkySprite {
                    drift: [0.0, 400.0, 0.0],
                    ..sprite()
                },
                "drift-elevation",
            ),
            (
                SkySprite {
                    drift: [0.0, 0.0, -361.0],
                    ..sprite()
                },
                "spin",
            ),
            (
                SkySprite {
                    lifetime: 0.01,
                    ..sprite()
                },
                "lifetime",
            ),
            (
                SkySprite {
                    lifetime: 700.0,
                    ..sprite()
                },
                "lifetime",
            ),
            (
                SkySprite {
                    lifetime: 2.0,
                    fade_in: 3.0,
                    ..sprite()
                },
                "fade-in",
            ),
            (
                SkySprite {
                    fade_out: 1.0,
                    ..sprite()
                },
                "fade-out needs",
            ),
            (
                SkySprite {
                    fade_in: 61.0,
                    ..sprite()
                },
                "fade-in",
            ),
        ];
        for (s, why) in cases {
            let e = s.clone().checked(&mine).unwrap_err();
            assert!(e.contains(why), "{s:?}: {e}");
        }
    }

    #[test]
    fn lists_are_capped_and_keys_unique() {
        let list = |n: u32| {
            (0..n)
                .map(|key| SkySprite { key, ..sprite() })
                .collect::<Vec<_>>()
        };
        assert_eq!(check_list(list(256), &mine).unwrap().len(), 256);
        assert!(check_list(list(257), &mine).unwrap_err().contains("256"));
        let mut dup = list(3);
        dup[2].key = 0;
        assert!(
            check_list(dup, &mine)
                .unwrap_err()
                .contains("sky sprite 2: key 0")
        );
        let mut bad = list(3);
        bad[1].alpha = 2.0;
        assert!(
            check_list(bad, &mine)
                .unwrap_err()
                .starts_with("sky sprite 1: alpha")
        );
        assert!(check_list(Vec::new(), &mine).unwrap().is_empty());
    }

    #[test]
    fn culling_keeps_what_can_touch_the_screen() {
        let half = half_diagonal(70.0, 16.0 / 9.0);
        // tan(35°) = 0.700; the corner is at atan(0.700 * sqrt(1 + 3.16)) = 55.0°.
        assert!((half - 55.0).abs() < 0.1, "{half}");
        let south = forward(0.0, 0.0);
        let at = |yaw: f32| SkySprite { yaw, ..sprite() }.place(0.0).unwrap();
        let ahead = at(0.0);
        assert!(in_view(ahead.direction, ahead.reach_degrees, south, half));
        let behind = at(180.0);
        assert!(!in_view(
            behind.direction,
            behind.reach_degrees,
            south,
            half
        ));
        // 60° off: just past the corner but its own reach brings it in.
        let side = at(60.0);
        assert!(in_view(side.direction, side.reach_degrees, south, half));
        assert!(!in_view(side.direction, 0.0, south, half));
        assert!(in_view(behind.direction, 130.0, south, half));
    }

    #[test]
    fn texture_paths_are_resource_paths() {
        assert_eq!(
            texture_path("minecraft:environment/celestial/sun").as_deref(),
            Some("assets/minecraft/textures/environment/celestial/sun.png")
        );
        assert_eq!(texture_path("bad"), None);
    }
}
