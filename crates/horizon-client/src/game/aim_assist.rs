//! Client-only aim assistance. Nothing is collected until a mod enables the device.
use super::Game;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LookDevice {
    Mouse,
    Controller,
    Touch,
}

pub(super) fn accumulated_device(touch_frame: Option<u64>, frame: u64, mouse: bool) -> LookDevice {
    if touch_frame == Some(frame) && !mouse {
        LookDevice::Touch
    } else {
        LookDevice::Mouse
    }
}

impl Game<'_> {
    pub(super) fn touch_assist_enabled(&self) -> bool {
        #[cfg(feature = "mods")]
        {
            let mut configs = self.mods.mods.aim_assists();
            configs
                .next()
                .is_some_and(|c| c.touch && !c.targets.is_empty())
                && configs.all(|c| c.touch && !c.targets.is_empty())
        }
        #[cfg(not(feature = "mods"))]
        {
            false
        }
    }

    pub(super) fn device_look_turn(&mut self, x: f64, y: f64, device: LookDevice) {
        let before = (self.yaw, self.pitch);
        self.look_turn(x, y);
        if (self.yaw, self.pitch) != before {
            self.look_device = device;
        }
    }

    #[cfg(not(feature = "mods"))]
    pub(super) fn assisted_turn(
        &mut self,
        turn: (f64, f64),
        _seconds: f32,
        _controller: bool,
    ) -> (f64, f64) {
        turn
    }

    #[cfg(feature = "mods")]
    pub(super) fn assisted_turn(
        &mut self,
        turn: (f64, f64),
        seconds: f32,
        controller: bool,
    ) -> (f64, f64) {
        use horizon_client_input::aim_assist::{Config, Target, assist};
        if turn == (0.0, 0.0) || self.aim_assist_targets_left == 0 {
            return turn;
        }
        let mut configs = self.mods.mods.aim_assists();
        let Some(first) = configs.next() else {
            return turn;
        };
        let enabled = |c: &horizon_mods::AimAssist| {
            if controller { c.controller } else { c.touch }
        };
        let mut cfg = Config {
            range: first.range,
            cone: first.cone,
            slowdown: first.slowdown,
            pull: first.pull,
        };
        if !enabled(first) || first.targets.is_empty() {
            return turn;
        }
        for c in configs {
            if !enabled(c) || c.targets.is_empty() {
                return turn;
            }
            cfg.range = cfg.range.min(c.range);
            cfg.cone = cfg.cone.min(c.cone);
            cfg.slowdown = cfg.slowdown.min(c.slowdown);
            cfg.pull = cfg.pull.min(c.pull);
        }
        let Some(eye) = self.eye_position() else {
            return turn;
        };
        let mut targets = [Target {
            yaw: 0.0,
            pitch: 0.0,
            distance: 0.0,
        }; 32];
        let mut count = 0;
        let mut considered = 0;
        // The dragon has its own simulation; use its entity box, like a normal mob.
        let dragon = self.dragon.as_ref().filter(|d| d.alive()).map(|d| {
            use horizon_core::geometry::{Aabb, Vec3};
            let p = d.position;
            (
                "minecraft:ender_dragon",
                "monster",
                Aabb::new(
                    Vec3::new(p.x - 8.0, p.y, p.z - 8.0),
                    Vec3::new(p.x + 8.0, p.y + 8.0, p.z + 8.0),
                ),
            )
        });
        let mobs = self
            .mobs
            .iter()
            .filter(|m| m.alive() && !m.removed && m.kind.id != "minecraft:player")
            .map(|m| (m.kind.id.as_str(), m.kind.category.as_str(), m.bounds()))
            .chain(dragon);
        for (id, category, bounds) in mobs {
            let hostile = is_hostile(id, category);
            // Intersect predicates, rather than ids: '*' and explicit ids overlap too.
            if !self.mods.mods.aim_assists().all(|c| {
                c.targets
                    .iter()
                    .any(|t| t == "*" || t == id || (t == "#hostile" && hostile))
            }) {
                continue;
            }
            let lo = bounds.min_position();
            let hi = bounds.max_position();
            let point = [
                eye[0].clamp(lo.x, hi.x),
                eye[1].clamp(lo.y, hi.y),
                eye[2].clamp(lo.z, hi.z),
            ];
            let d = [point[0] - eye[0], point[1] - eye[1], point[2] - eye[2]];
            let distance = d[0].hypot(d[2]).hypot(d[1]);
            if !distance.is_finite() || distance > f64::from(cfg.range) {
                continue;
            }
            let target = if distance == 0.0 {
                Target {
                    yaw: self.yaw,
                    pitch: self.pitch,
                    distance: 0.0,
                }
            } else {
                Target {
                    yaw: (-d[0]).atan2(d[2]).to_degrees() as f32,
                    pitch: (-d[1]).atan2(d[0].hypot(d[2])).to_degrees() as f32,
                    distance: distance as f32,
                }
            };
            let yaw = (target.yaw - self.yaw + 180.0).rem_euclid(360.0) - 180.0;
            if yaw.hypot(target.pitch - self.pitch) > cfg.cone {
                continue;
            }
            // Bound both block raycasts and the maths' target list to 32 per frame.
            considered += 1;
            if distance == 0.0
                || self
                    .clip_ray(eye, d.map(|v| v / distance), distance)
                    .is_none()
            {
                targets[count] = target;
                count += 1;
            }
            if considered == self.aim_assist_targets_left {
                break;
            }
        }
        self.aim_assist_targets_left -= considered;
        if count == 0 {
            return turn;
        }
        let adjusted = assist(
            (self.yaw, self.pitch),
            ((turn.0 * 0.15) as f32, (turn.1 * 0.15) as f32),
            seconds,
            &targets[..count],
            &cfg,
        );
        (f64::from(adjusted.0) / 0.15, f64::from(adjusted.1) / 0.15)
    }
}

#[cfg(feature = "mods")]
fn is_hostile(id: &str, category: &str) -> bool {
    use horizon_client::mobs::spawn_selection::Category;
    if Category::ALL.iter().any(|c| c.name() == category) {
        return category == Category::Monster.name();
    }
    // Remote metadata can lack categories. Explicit mod ids still match independently.
    matches!(
        id.strip_prefix("minecraft:"),
        Some(
            "zombie"
                | "skeleton"
                | "creeper"
                | "spider"
                | "cave_spider"
                | "enderman"
                | "witch"
                | "slime"
                | "phantom"
                | "drowned"
                | "husk"
                | "stray"
                | "blaze"
                | "ghast"
                | "magma_cube"
                | "wither_skeleton"
                | "pillager"
                | "ravager"
                | "vindicator"
                | "evoker"
                | "vex"
                | "guardian"
                | "elder_guardian"
                | "shulker"
                | "silverfish"
                | "endermite"
                | "zombie_villager"
                | "piglin_brute"
                | "hoglin"
                | "zoglin"
                | "warden"
                | "breeze"
                | "bogged"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::{LookDevice, accumulated_device};

    #[test]
    fn mixed_mouse_and_touch_are_mouse_and_stale_touch_is_not_reused() {
        assert_eq!(accumulated_device(None, 0, false), LookDevice::Mouse);
        assert_eq!(accumulated_device(Some(7), 7, false), LookDevice::Touch);
        // Even if opposite physical mouse events cancel, the flag stays true.
        assert_eq!(accumulated_device(Some(7), 7, true), LookDevice::Mouse);
        assert_eq!(accumulated_device(Some(7), 8, false), LookDevice::Mouse);
        assert_eq!(accumulated_device(None, 8, true), LookDevice::Mouse);
    }
}
