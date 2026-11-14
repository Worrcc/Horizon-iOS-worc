//! Capability `atmosphere`'s sky sprites (API level 33) in the frame: the
//! running mods' `set-sky-sprites` lists placed on the sky dome at each
//! sprite's age (drift, spin, lifetime and fades), culled to the view and
//! handed to the renderer (`gfx/sky_sprites.rs`), which draws them after the
//! stars and before clouds and terrain (`docs/api-expansion-d.md`).
use super::Game;
use horizon_client::gfx::sky_sprites::Sprite;

/// When each mod's sprite key first showed, in game ticks; keys a mod no
/// longer lists are forgotten, so the key shown again starts afresh.
#[cfg(feature = "mods")]
#[derive(Default)]
pub(super) struct SkyAges {
    born: std::collections::HashMap<(String, u32), f64>,
}

/// What the frame needs to place sky sprites.
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) struct SkyView {
    pub(super) yaw: f32,
    pub(super) pitch: f32,
    pub(super) fov_degrees: f32,
    pub(super) aspect: f32,
    /// The stars' frame (`celestial_geometry`): rotate Y -90, then X by this angle.
    pub(super) star_angle: f32,
    /// Game ticks plus the partial tick.
    pub(super) now: f64,
}

impl Game<'_> {
    /// This frame's sky sprites; none on the title screen, without a sky
    /// (the Nether, Horizon's sky option off) or under blindness or darkness.
    #[cfg(feature = "mods")]
    pub(super) fn mod_sky_sprites(&mut self, title: bool, view: SkyView) -> Vec<Sprite> {
        use horizon_mods::atmosphere_sky::{Blend, Space, forward, half_diagonal, in_view};
        let state = &mut self.mods;
        let shown = !title
            && self.settings.horizon.sky
            && self.dim != super::Dim::Nether
            && self.potions.effects.get("darkness").is_none()
            && self.potions.effects.get("blindness").is_none();
        let mut sprites = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let stars = glam::Mat3::from_rotation_y((-90.0f32).to_radians())
            * glam::Mat3::from_rotation_x(view.star_angle.to_radians());
        let facing = forward(view.yaw, view.pitch);
        let half = half_diagonal(view.fov_degrees, view.aspect);
        for (mod_id, sprite) in state.mods.atmosphere_sky_sprites() {
            let key = (mod_id.to_owned(), sprite.key);
            let born = *state.sky.born.entry(key.clone()).or_insert(view.now);
            seen.insert(key);
            if !shown {
                continue;
            }
            let Some(placed) = sprite.place(((view.now - born) / 20.0) as f32) else {
                continue;
            };
            let turn = |p: [f32; 3]| match sprite.space {
                Space::Compass => p,
                Space::Stars => (stars * glam::Vec3::from_array(p)).to_array(),
            };
            if !in_view(turn(placed.direction), placed.reach_degrees, facing, half) {
                continue;
            }
            sprites.push(Sprite {
                texture: sprite.texture.clone(),
                uv: sprite.uv,
                corners: placed.corners.map(turn),
                rgba: placed.rgba,
                emissive: sprite.blend == Blend::Emissive,
            });
        }
        state.sky.born.retain(|key, _| seen.contains(key));
        sprites
    }
    #[cfg(not(feature = "mods"))]
    pub(super) fn mod_sky_sprites(&mut self, _title: bool, _view: SkyView) -> Vec<Sprite> {
        Vec::new()
    }
}
