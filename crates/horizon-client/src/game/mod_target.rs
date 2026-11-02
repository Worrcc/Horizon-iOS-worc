//! Client mods' read-only living entity raycast. Content mobs and remote players
//! live in the same mob list as vanilla mobs; health comes from those instances.
use super::{Game, ray_box, view_vector};
use horizon_client::mobs::{Mob, finalize};
use horizon_mods::{EntityHit, RAYCAST_MAX};

fn mob_info(m: &Mob, player_name: Option<String>) -> EntityHit {
    let key = format!("entity.{}", m.kind.id.replace(':', "."));
    EntityHit {
        kind: m.kind.id.clone(),
        name: player_name
            .or_else(|| m.custom_name_text.as_ref().map(|t| t.plain()))
            .or_else(|| m.custom_name.clone())
            .unwrap_or_else(|| horizon_client::lang::get(&key)),
        health: m.health,
        max_health: finalize::max_health(m),
        x: m.position.x,
        y: m.position.y,
        z: m.position.z,
    }
}

fn nearest_mob(mobs: &[Mob], eye: [f64; 3], dir: [f64; 3]) -> Option<(f64, &Mob)> {
    mobs.iter()
        .filter(|m| m.alive() && !m.removed)
        .filter_map(|m| Some((ray_box(eye, dir, f64::from(RAYCAST_MAX), m.bounds())?, m)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

impl Game<'_> {
    pub(super) fn mod_view_entity(&mut self) -> Option<(f32, EntityHit)> {
        if !self.mods.mods.wants_entity_raycast() {
            return None;
        }
        let eye = self.eye_position()?;
        let dir = view_vector(self.yaw, self.pitch);
        let mut best = nearest_mob(&self.mobs, eye, dir).map(|(d, m)| {
            let player_name = (m.kind.id == "minecraft:player")
                .then(|| {
                    self.remote_player_names()
                        .into_iter()
                        .find(|(id, _)| *id == m.id)
                })
                .flatten()
                .map(|(_, name)| name);
            (d, mob_info(m, player_name))
        });
        // In-frame hosts keep their guests outside the mob list.
        if let Some(host) = self.lan.as_mut() {
            for g in &mut host.hub.guests {
                if !g.placed
                    || g.dimension != host.hub.config.dimension
                    || !g.loaded
                    || g.dead
                    || g.spectator()
                {
                    continue;
                }
                if let Some(d) = ray_box(
                    eye,
                    dir,
                    f64::from(RAYCAST_MAX),
                    super::lan_mobs::guest_bounds(g),
                ) && best.as_ref().is_none_or(|b| d < b.0)
                {
                    let Some(max_health) = g
                        .motion
                        .as_mut()
                        .and_then(|p| p.attribute("minecraft:max_health").ok())
                    else {
                        continue;
                    };
                    best = Some((
                        d,
                        EntityHit {
                            kind: "minecraft:player".into(),
                            name: g.name.clone(),
                            health: g.health,
                            max_health: max_health as f32,
                            x: g.position[0],
                            y: g.position[1],
                            z: g.position[2],
                        },
                    ));
                }
            }
        }
        // The dragon uses multipart boxes and its own health state.
        if let Some(dragon) = self.dragon.as_ref().filter(|d| d.alive()) {
            for part in 0..8 {
                if let Some(d) = ray_box(eye, dir, f64::from(RAYCAST_MAX), dragon.part_box(part))
                    && best.as_ref().is_none_or(|b| d < b.0)
                {
                    best = Some((
                        d,
                        EntityHit {
                            kind: "minecraft:ender_dragon".into(),
                            name: horizon_client::lang::get("entity.minecraft.ender_dragon"),
                            health: dragon.health,
                            max_health: super::dragon::MAX_HEALTH,
                            x: dragon.position.x,
                            y: dragon.position.y,
                            z: dragon.position.z,
                        },
                    ));
                }
            }
        }
        best.map(|(d, hit)| (d as f32, hit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::mobs::MobTypes;
    use horizon_core::geometry::Vec3;
    use std::sync::Arc;

    #[test]
    fn entity_target_uses_content_mob_health_and_nearest_live_box() {
        let root = std::env::temp_dir().join(format!("horizon-mod-target-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("entity-types.json"),
            r#"{
            "minecraft:cow": {"width": 1, "height": 2, "attributes": {"minecraft:max_health": 10}},
            "ruby:golem": {"horizon": {"template": "minecraft:cow"},
                "attributes": {"minecraft:max_health": 60}}
        }"#,
        )
        .unwrap();
        std::fs::write(
            root.join("entity-models.json"),
            r#"{"minecraft:cow#main": {}}"#,
        )
        .unwrap();
        let types = MobTypes::load(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        let kind = Arc::new(types.types["ruby:golem"].clone());
        let mut mob = Mob::new(1, kind, Vec3::new(0.0, 0.0, 4.0), 0.0);
        assert_eq!((mob.health, mob_info(&mob, None).max_health), (60.0, 60.0));
        mob.health = 37.5;
        finalize::set_base(&mut mob, finalize::Attr::MaxHealth, 80.0);
        mob.custom_name = Some("Ruby Guardian".into());
        let info = mob_info(&mob, None);
        assert_eq!((info.health, info.max_health), (37.5, 80.0));
        assert_eq!(
            (info.kind.as_str(), info.name.as_str()),
            ("ruby:golem", "Ruby Guardian")
        );
        assert_eq!(mob_info(&mob, Some("Alex".into())).name, "Alex");
        let mut near = mob.clone();
        near.id = 2;
        near.position.z = 2.0;
        let mut mobs = vec![mob, near];
        let pick =
            |mobs: &[Mob]| nearest_mob(mobs, [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]).map(|(_, m)| m.id);
        assert_eq!(pick(&mobs), Some(2));
        mobs[1].health = 0.0;
        assert_eq!(pick(&mobs), Some(1));
        mobs[0].removed = true;
        assert_eq!(pick(&mobs), None);
    }
}
