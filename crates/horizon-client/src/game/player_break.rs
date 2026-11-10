//! Queued mod mining shares the exact player break path, after runtime loans return.
use super::Game;

#[derive(Clone, Copy)]
pub(super) struct PlayerBreakOptions {
    pub drops_at: Option<[f64; 3]>,
    pub exhaustion: f32,
}

#[cfg(feature = "mods")]
impl Game<'_> {
    pub(super) fn apply_mod_breaks(
        &mut self,
        host: Option<&mut super::lan::LanHost>,
        caller: &str,
        player: &str,
        blocks: Vec<horizon_mods::BlockChangePos>,
        options: horizon_mods::BreakOptions,
    ) {
        let scoped = PlayerBreakOptions {
            drops_at: options.drops_at.map(|p| [p.x, p.y, p.z]),
            exhaustion: options.exhaustion,
        };
        let apply = |game: &mut Self, host: Option<&super::lan::LanHost>| {
            let previous_caller = game.mods.break_caller.replace(caller.to_owned());
            for pos in blocks {
                let pos = [pos.x, pos.y, pos.z];
                if host.is_some_and(|h| {
                    game.lan_acting
                        .and_then(|conn| h.hub.guests.iter().position(|g| g.conn == conn))
                        .is_some_and(|gi| game.under_spawn_protection(h, gi, pos))
                }) {
                    continue;
                }
                if !game.border.contains(f64::from(pos[0]), f64::from(pos[2])) {
                    continue;
                }
                let Some(p) = game.player.as_ref() else { break };
                let distance: f64 = pos
                    .iter()
                    .zip(p.position.components())
                    .map(|(v, p)| (f64::from(*v) + 0.5 - p).powi(2))
                    .sum();
                if distance > 80.0 * 80.0
                    || !game.world.chunks.contains_key(&(pos[0] >> 4, pos[2] >> 4))
                {
                    continue;
                }
                let state = game.world.block(pos[0], pos[1], pos[2]);
                let facts = game.res.facts().get(state);
                if game.res.facts().is_air(state)
                    || matches!(&*facts.name, "minecraft:water" | "minecraft:lava")
                    || (!game.creative && facts.hardness < 0.0)
                {
                    continue;
                }
                if options.keep_tool
                    && !game.creative
                    && game.break_would_use_last_durability(&facts.name, facts.hardness)
                {
                    break;
                }
                let previous = game.player_break_options.replace(scoped);
                let result = game.break_block(pos);
                game.player_break_options = previous;
                if let Err(error) = result {
                    log::warn!("Horizon: queued mod break: {error}");
                    break;
                }
            }
            game.mods.break_caller = previous_caller;
        };
        if self.player.is_some()
            && ((self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player_uuid_string() == player)
                || self
                    .lan_acting_uuid
                    .is_some_and(|id| super::server_mods::run::uuid_string(id) == player))
        {
            apply(self, None);
        } else if let Some(host) = host {
            self.with_mod_guest_host(host, player, |g, h| apply(g, Some(h)));
        } else if let Some(mut host) = self.lan.take() {
            self.with_mod_guest_host(&mut host, player, |g, h| apply(g, Some(h)));
            self.lan = Some(host);
        }
    }
    fn break_would_use_last_durability(&self, name: &str, hardness: f32) -> bool {
        let Some(stack) = self.held() else {
            return false;
        };
        let Some(info) = self.res.items().get(&stack.item) else {
            return false;
        };
        let wears = if &*stack.item == "minecraft:shears" {
            !self.items().block_in_tag("minecraft:fire", name)
        } else {
            hardness != 0.0
        };
        let damage = info.tool_damage_per_block.unwrap_or(0);
        // Conservative before Unbreaking RNG: never consume the last possible point.
        wears
            && damage > 0
            && info.max_damage > 0
            && stack.damage.saturating_add(damage) >= info.max_damage
    }
}

#[cfg(all(test, feature = "mods"))]
mod tests {
    use super::*;
    use crate::game::{Audio, Options, Progress, Settings, load_resources};
    use horizon_client::{chunk::Chunk, items::ItemStack, motion::PlayerMotion};
    use horizon_core::geometry::Vec3;
    use horizon_mods::{BlockChangePos, BreakOptions};
    use std::sync::Arc;

    #[test]
    #[ignore = "needs prepared resources in HORIZON_TEST_RESOURCES"]
    fn queued_breaks_drop_wear_keep_the_tool_and_creative_drops_nothing() {
        let root = std::path::PathBuf::from(std::env::var_os("HORIZON_TEST_RESOURCES").unwrap());
        let settings = Settings::default();
        let loaded = load_resources(
            root.clone(),
            None,
            &root,
            &settings,
            std::time::Instant::now(),
            false,
            true,
            &Progress::new(),
            &mut |_, _| unreachable!(),
        )
        .unwrap();
        let mut options = Options::parse(std::iter::empty()).unwrap().unwrap();
        options.resources = root.clone();
        let mut game = Game::new(
            &loaded.resources,
            loaded.pack,
            loaded.gui.unwrap(),
            options,
            settings,
            Audio::silent(&root),
            None,
        )
        .unwrap();
        let scratch =
            std::env::temp_dir().join(format!("horizon-api20-break-{}", std::process::id()));
        std::fs::create_dir_all(&scratch).unwrap();
        game.world_root = scratch.clone();
        game.title = None;
        game.player = Some(PlayerMotion::new(game.motion, Vec3::new(0.5, 64.0, 0.5)).unwrap());
        game.creative = false;
        game.mode = horizon_client::command::args::GameMode::Survival;
        game.mods.server = true;
        game.world
            .chunks
            .insert((0, 0), Arc::new(Chunk::empty(0, 0)));
        let stone = game
            .pack
            .assets
            .registry
            .default_state("minecraft:stone")
            .unwrap();
        let first = BlockChangePos { x: 1, y: 64, z: 0 };
        let second = BlockChangePos { x: 2, y: 64, z: 0 };
        game.world.set(first.x, first.y, first.z, stone).unwrap();
        game.world.set(second.x, second.y, second.z, stone).unwrap();
        game.inventory[game.selected] = Some(ItemStack::new("minecraft:iron_pickaxe", 1));
        // A loaded player already has a level UUID before mods receive their ID.
        // Otherwise the first break initializes it and invalidates the fallback ID.
        game.level.ensure_player_uuid();
        let player = game.player_uuid_string();
        let options = BreakOptions {
            keep_tool: true,
            drops_at: Some(horizon_mods::WorldPoint {
                x: 5.5,
                y: 64.5,
                z: 5.5,
            }),
            exhaustion: 0.25,
        };
        game.apply_mod_breaks(None, "miner", &player, vec![first], options);
        assert!(
            game.res
                .facts()
                .is_air(game.world.block(first.x, first.y, first.z))
        );
        assert_eq!(game.held().unwrap().damage, 1);
        assert!(
            game.dropped
                .iter()
                .any(|d| &*d.stack.item == "minecraft:cobblestone")
        );
        assert!(
            game.dropped
                .iter()
                .all(|d| d.position == Vec3::new(5.5, 64.5, 5.5))
        );
        assert_eq!(game.player.as_ref().unwrap().health.food.exhaustion, 0.255);
        assert_eq!(game.player_uuid_string(), player);
        assert!(game.mods.events.iter().any(|e| matches!(e,super::super::server_mods::ModEvent::Block {exclude:Some(id),..} if id == "miner")));
        let max = game
            .res
            .items()
            .get("minecraft:iron_pickaxe")
            .unwrap()
            .max_damage;
        game.inventory[game.selected].as_mut().unwrap().damage = max - 1;
        game.apply_mod_breaks(None, "miner", &player, vec![second, first], options);
        assert_eq!(game.world.block(second.x, second.y, second.z), stone);
        assert_eq!(game.held().unwrap().damage, max - 1);
        game.creative = true;
        game.mode = horizon_client::command::args::GameMode::Creative;
        let dropped = game.dropped.len();
        game.apply_mod_breaks(None, "miner", &player, vec![second], options);
        assert!(
            game.res
                .facts()
                .is_air(game.world.block(second.x, second.y, second.z))
        );
        assert_eq!(game.dropped.len(), dropped);
        assert_eq!(game.held().unwrap().damage, max - 1);
        assert_eq!(game.player.as_ref().unwrap().health.food.exhaustion, 0.255);
        assert!(game.player_break_options.is_none());
        assert!(game.mods.break_caller.is_none());
        drop(game);
        std::fs::remove_dir_all(scratch).unwrap();
    }
}
