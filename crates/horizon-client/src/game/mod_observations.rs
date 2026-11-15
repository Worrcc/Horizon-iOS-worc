//! API 35: bounded client observations and the vanilla HUD's actual draw bounds.
use super::Game;
use horizon_client::{hud_layout::GuiDraw, mobs::Species};
use horizon_mods::{
    BarKind, EntityCategory, EntityDot, FoodState, HudBar, RenderRect,
    client_queries::{self, Category},
};
use std::sync::Arc;

fn category(c: Category) -> EntityCategory {
    match c {
        Category::Hostile => EntityCategory::Hostile,
        Category::Passive => EntityCategory::Passive,
        Category::Neutral => EntityCategory::Neutral,
        Category::Water => EntityCategory::Water,
        Category::Ambient => EntityCategory::Ambient,
        Category::Player => EntityCategory::Player,
        Category::OwnTamed => EntityCategory::OwnTamed,
        Category::OtherTamed => EntityCategory::OtherTamed,
        Category::Other => EntityCategory::Other,
    }
}

/// Species templates give content mobs the same neutral trait as their vanilla template.
fn neutral(species: Species) -> bool {
    matches!(
        species,
        Species::Bee
            | Species::Wolf
            | Species::PolarBear
            | Species::IronGolem
            | Species::Enderman
            | Species::ZombifiedPiglin
            | Species::Piglin
            | Species::Dolphin
            | Species::Llama
            | Species::TraderLlama
            | Species::Panda
            | Species::Goat
            | Species::Spider
            | Species::CaveSpider
            | Species::Pufferfish
    )
}

/// Groups real sprite bounds instead of duplicating the HUD's desktop/mobile formulas.
fn bars(draws: &[GuiDraw]) -> Result<Vec<HudBar>, String> {
    let mut groups: [Vec<[f32; 4]>; 5] = std::array::from_fn(|_| Vec::new());
    for draw in draws {
        let GuiDraw::Sprite {
            sprite, x, y, w, h, ..
        } = draw
        else {
            continue;
        };
        let index = if sprite.starts_with("hud/heart/vehicle_") {
            4
        } else if sprite.starts_with("hud/heart/") {
            0
        } else if sprite.starts_with("hud/food_") {
            1
        } else if sprite.starts_with("hud/armor_") {
            2
        } else if sprite.as_ref() == "hud/air" || sprite.starts_with("hud/air_") {
            3
        } else {
            continue;
        };
        groups[index].push([*x as f32, *y as f32, *w as f32, *h as f32]);
    }
    let mut out = Vec::new();
    for (rects, kind) in groups.into_iter().zip([
        BarKind::Health,
        BarKind::Food,
        BarKind::Armor,
        BarKind::Air,
        BarKind::MountHealth,
    ]) {
        if let Some([x, y, w, h]) = client_queries::union_bounds(rects)? {
            out.push(HudBar {
                kind,
                bounds: RenderRect { x, y, w, h },
            });
        }
    }
    Ok(out)
}

impl Game<'_> {
    pub(super) fn capture_mod_hud(&mut self, draws: &[GuiDraw]) {
        let Ok(bars) = bars(draws) else {
            self.mods.hud_bars = None;
            return;
        };
        if let Some((_, current)) = &mut self.mods.hud_bars {
            current.extend(bars);
        }
    }

    pub(super) fn mod_food_stats(&mut self) -> Option<FoodState> {
        if self.title.is_some() {
            return None;
        }
        let p = self.player.as_mut()?;
        let exhaustion = match &self.integrated {
            Some(server) => {
                let uuid = self
                    .remote
                    .as_ref()
                    .map(|r| r.profile())
                    .or(self
                        .level
                        .player_uuid
                        .map(horizon_client::lan::playerdata::ints_uuid))
                    .unwrap_or_else(|| {
                        horizon_protocol::crypto::offline_uuid(&self.options.player_name())
                    });
                server.food_exhaustion(uuid)
            }
            None if self.remote.is_none() => Some(p.health.food.exhaustion),
            None => None,
        };
        Some(FoodState {
            food: p.health.food.food.max(0) as u32,
            saturation: p.health.food.saturation,
            exhaustion,
            health: p.health.health(),
            max_health: p.attribute("minecraft:max_health").ok()? as f32,
            absorption: p.health.absorption(),
        })
    }

    pub(super) fn mod_client_entities(&mut self) -> (Arc<[EntityDot]>, Arc<[String]>) {
        if !self.mods.mods.wants_client_entities() || self.title.is_some() {
            return Default::default();
        }
        let Some(player) = &self.player else {
            return Default::default();
        };
        let kinds = self
            .mods
            .entity_kinds
            .get_or_insert_with(|| {
                let mut ids: Vec<String> = self.res.mob_types().keys().cloned().collect();
                ids.extend(["minecraft:player".into(), "minecraft:ender_dragon".into()]);
                ids.sort();
                ids.dedup();
                ids.into()
            })
            .clone();
        let center = player.position;
        let owner = self
            .remote
            .as_ref()
            .map(|r| horizon_client::lan::playerdata::uuid_ints(r.profile()))
            .or(self.level.player_uuid)
            .or_else(|| {
                Some(horizon_client::lan::playerdata::uuid_ints(
                    horizon_protocol::crypto::offline_uuid(&self.options.player_name()),
                ))
            });
        let local_id = self.remote.as_ref().map(|r| r.entity_id as u32);
        let mut dots: Vec<(f64, EntityDot)> = Vec::new();
        let mut add = |kind: &str, position: [f64; 3], yaw: f32, category: EntityCategory| {
            let [x, y, z] = position;
            let distance = (x - center.x).powi(2) + (y - center.y).powi(2) + (z - center.z).powi(2);
            if !distance.is_finite()
                || distance > f64::from(client_queries::ENTITY_RADIUS).powi(2)
                || !yaw.is_finite()
            {
                return;
            }
            let Ok(kind) = kinds.binary_search_by(|id| id.as_str().cmp(kind)) else {
                return;
            };
            dots.push((
                distance,
                EntityDot {
                    x,
                    y,
                    z,
                    yaw,
                    kind: kind as u32,
                    category,
                },
            ));
        };
        for m in self
            .mobs
            .iter()
            .filter(|m| m.alive() && !m.removed && Some(m.id) != local_id)
        {
            let cat = client_queries::category(
                m.category(),
                neutral(m.species()),
                m.extra.tame,
                m.extra.owner.is_some() && m.extra.owner == owner,
                m.kind.id == "minecraft:player",
            );
            add(
                &m.kind.id,
                [m.position.x, m.position.y, m.position.z],
                m.yaw,
                category(cat),
            );
        }
        if let Some(dragon) = self.dragon.as_ref().filter(|d| d.alive()) {
            add(
                "minecraft:ender_dragon",
                [dragon.position.x, dragon.position.y, dragon.position.z],
                dragon.yaw,
                EntityCategory::Hostile,
            );
        }
        // Legacy in-frame hosts keep guests separately; normal clients see player mobs above.
        if let Some(host) = &self.lan {
            for g in host
                .hub
                .guests
                .iter()
                .filter(|g| g.placed && g.loaded && !g.dead && g.dimension == self.dim.id())
            {
                if owner == Some(horizon_client::lan::playerdata::uuid_ints(g.uuid)) {
                    continue;
                }
                add(
                    "minecraft:player",
                    g.position,
                    g.yaw,
                    EntityCategory::Player,
                );
            }
        }
        (client_queries::nearest(dots).into(), kinds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use horizon_client::hud_layout::{self, HealthHud, HealthInputs, HeartKind};
    #[test]
    fn api35_hud_uses_vanilla_draws_and_mount_replaces_food() {
        let mut input = HealthInputs {
            tick: 1,
            millis: 1,
            health: 20.0,
            max_health: 20.0,
            absorption: 0.0,
            armor: 10,
            food: 20,
            saturation: 5.0,
            kind: HeartKind::Normal,
            regeneration: false,
            hunger: false,
            hardcore: false,
            damage_cooldown: false,
            air: 300,
            max_air: 300,
            under_water: false,
            vehicle_hearts: 0,
        };
        let draws = HealthHud::default().layout(&input, 320, 240);
        let found = bars(&draws).unwrap();
        assert_eq!(found.len(), 3);
        assert_eq!(
            (
                found[0].bounds.x,
                found[0].bounds.y,
                found[0].bounds.w,
                found[0].bounds.h
            ),
            (69.0, 201.0, 81.0, 9.0)
        );
        assert_eq!((found[1].bounds.x, found[1].bounds.y), (170.0, 201.0));
        assert_eq!(found[2].bounds.y, 191.0);
        input.max_health = 40.0;
        input.absorption = 8.0;
        let extra_rows = bars(&HealthHud::default().layout(&input, 320, 240)).unwrap();
        let health = extra_rows
            .iter()
            .find(|b| matches!(b.kind, BarKind::Health))
            .unwrap();
        assert_eq!((health.bounds.y, health.bounds.h), (183.0, 27.0));
        assert_eq!(
            extra_rows
                .iter()
                .find(|b| matches!(b.kind, BarKind::Armor))
                .unwrap()
                .bounds
                .y,
            173.0
        );
        input.max_health = 20.0;
        input.absorption = 0.0;
        input.vehicle_hearts = 15;
        input.under_water = true;
        let mut draws = HealthHud::default().layout(&input, 320, 240);
        draws.extend(hud_layout::vehicle_health(320, 240, 15, 30.0));
        let found = bars(&draws).unwrap();
        assert!(!found.iter().any(|b| matches!(b.kind, BarKind::Food)));
        let mount = found
            .iter()
            .find(|b| matches!(b.kind, BarKind::MountHealth))
            .unwrap();
        assert_eq!(
            (
                mount.bounds.x,
                mount.bounds.y,
                mount.bounds.w,
                mount.bounds.h
            ),
            (170.0, 191.0, 81.0, 19.0)
        );
        assert_eq!(
            found
                .iter()
                .find(|b| matches!(b.kind, BarKind::Air))
                .unwrap()
                .bounds
                .y,
            181.0
        );
    }
}
