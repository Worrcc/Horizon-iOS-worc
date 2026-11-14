//! API 31: what server mods' player setters and world effects do on the live game.
use super::{
    Game, lan::LanHost, native_explosions::NativeBlast, native_falling::NativeFalling,
    render_control::parse_state, server_mods::run::uuid_string,
};
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_mods::{
    BlastKind, BlockChangePos, EffectChange, EntitySound, ParticleBurst, WorldPoint,
};

/// What a mod's blast does under the `mob_griefing` rule: with it off, blocks
/// stay and fire is not lit.
fn mod_blast(kind: BlastKind, griefing: bool, fire: bool) -> (NativeBlast, bool) {
    if !griefing {
        return (NativeBlast::Keep, false);
    }
    let blast = match kind {
        BlastKind::Keep => NativeBlast::Keep,
        BlastKind::Block => NativeBlast::Block,
        BlastKind::Tnt => NativeBlast::Tnt,
    };
    (blast, fire)
}

/// `DustParticleOptions` colour channels from 0xRRGGBB.
fn dust_rgb(rgb: u32) -> [f32; 3] {
    [
        ((rgb >> 16) & 255) as f32 / 255.0,
        ((rgb >> 8) & 255) as f32 / 255.0,
        (rgb & 255) as f32 / 255.0,
    ]
}

/// Whether a block may be turned into a falling one: loaded, still the block
/// the mod saw, and with no block entity (its contents would be lost).
fn fall_ready(loaded: bool, matches: bool, has_block_entity: bool) -> Result<(), &'static str> {
    if !loaded {
        Err("not loaded")
    } else if !matches {
        Err("block changed")
    } else if has_block_entity {
        Err("block has a block entity")
    } else {
        Ok(())
    }
}

impl Game<'_> {
    /// `players.set-health`: never kills and never revives.
    pub(super) fn apply_mod_set_health(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        health: f32,
    ) {
        self.with_mod_player(host, player, |g| {
            if let Some(p) = g.player.as_mut()
                && p.health.health() > 0.0
            {
                let max = p.attribute("minecraft:max_health").unwrap_or(20.0) as f32;
                let _ = p.set_health(health.min(max), false);
            }
        });
    }

    /// `players.hurt`: vanilla damage of a registered type; armour applies and
    /// it can kill. Creative players are immune (`hurt_player_by`).
    pub(super) fn apply_mod_hurt(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        amount: f32,
        damage_type: &str,
    ) {
        self.with_mod_player(host, player, |g| {
            if !g.player.as_ref().is_some_and(|p| p.health.health() > 0.0) {
                return;
            }
            if g.health_resources.damage.id(damage_type).is_none() {
                log::warn!("Horizon: server mod hurt with unknown damage type {damage_type}");
                return;
            }
            g.hurt_player_by(damage_type, amount);
        });
    }

    pub(super) fn apply_mod_set_food(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        food: u32,
        saturation: f32,
    ) {
        self.with_mod_player(host, player, |g| {
            if let Some(p) = g.player.as_mut() {
                p.health.food.food = food as i32;
                p.health.food.saturation = saturation.min(food as f32);
            }
        });
    }

    pub(super) fn apply_mod_give_experience(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        points: u32,
    ) {
        self.with_mod_player(host, player, |g| g.give_orb_points(points));
    }

    pub(super) fn apply_mod_give_levels(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        levels: i32,
    ) {
        self.with_mod_player(host, player, |g| g.give_levels(levels));
    }

    /// `players.clear-effects`: removes each id the mod saw active.
    pub(super) fn apply_mod_clear_effects(
        &mut self,
        mut host: Option<&mut LanHost>,
        player: &str,
        ids: Vec<String>,
    ) {
        for id in ids {
            self.apply_mod_player_effect(
                host.as_deref_mut(),
                player,
                EffectChange {
                    id,
                    seconds: 0,
                    amplifier: 0,
                },
            );
        }
    }

    /// `players.play-sound-to`: only this player hears it, at their position.
    pub(super) fn apply_mod_sound_to(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        sound: &EntitySound,
    ) {
        if self.options.execution_role.is_client()
            && self.remote.is_none()
            && self.player.is_some()
            && self.player_uuid_string() == player
        {
            if let Some(p) = &self.player {
                let at = [p.position.x, p.position.y, p.position.z];
                self.audio
                    .play_unshared(&sound.name, Some(at), sound.volume, sound.pitch);
            }
            return;
        }
        let send = |host: &LanHost| {
            if let Some(g) = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && uuid_string(g.uuid) == player)
            {
                let seed = host.hub.next_sound_seed();
                let packet = host.hub.sound_packet(
                    &sound.name,
                    g.position,
                    sound.volume,
                    sound.pitch,
                    horizon_client::settings::sound_source(&sound.name) as i32,
                    seed,
                );
                host.hub.send_to(g.conn, &packet);
            }
        };
        match host {
            Some(host) => send(host),
            None => {
                if let Some(host) = self.lan.as_ref() {
                    send(host);
                }
            }
        }
    }

    /// `world.spawn-particles` in the level in place.
    pub(super) fn apply_mod_particles(&mut self, burst: &ParticleBurst) {
        let Some(ty) = ParticleType::from_name(&burst.name) else {
            log::warn!(
                "Horizon: server mod asked for unknown particle {}",
                burst.name
            );
            return;
        };
        let (options, state) = if let Some(block) = &burst.block {
            match parse_state(self.pack.assets.registry, block) {
                Ok(state) => (ParticleOptions::simple(ty), Some(state)),
                Err(error) => {
                    log::warn!("Horizon: server mod particle block {block}: {error}");
                    return;
                }
            }
        } else if let Some(dust) = &burst.dust {
            (ParticleOptions::dust(dust_rgb(dust.rgb), dust.scale), None)
        } else {
            (ParticleOptions::simple(ty), None)
        };
        let at = [burst.at.x, burst.at.y, burst.at.z];
        let delta = [burst.delta.x, burst.delta.y, burst.delta.z];
        if !self.native_level_send_particles(
            &options,
            state,
            at,
            delta,
            f64::from(burst.speed),
            burst.count as i32,
        ) {
            log::debug!("Horizon: server mod particles {} were not sent", burst.name);
        }
    }

    /// `world.explode`: `Level.explode` with no source.
    pub(super) fn apply_mod_explode(
        &mut self,
        dimension: &str,
        center: WorldPoint,
        radius: f32,
        fire: bool,
        kind: BlastKind,
    ) {
        let (blast, fire) = mod_blast(kind, self.ui.rules.bool("mob_griefing"), fire);
        if !self.native_explode(
            dimension,
            [center.x, center.y, center.z],
            radius,
            fire,
            blast,
        ) {
            log::warn!("Horizon: server mod explosion at {center:?} was refused");
        }
    }

    /// `world.make-block-fall`: spawn the falling block first, then remove the
    /// original, so a refused spawn changes nothing.
    pub(super) fn apply_mod_block_fall(
        &mut self,
        dimension: &str,
        pos: BlockChangePos,
        expected: &str,
        velocity: WorldPoint,
        hurts: bool,
    ) {
        let state = match parse_state(self.pack.assets.registry, expected) {
            Ok(state) => state,
            Err(error) => {
                log::warn!("Horizon: server mod falling block {expected}: {error}");
                return;
            }
        };
        let loaded = self.world.chunks.contains_key(&(pos.x >> 4, pos.z >> 4));
        let matches = loaded && self.world.block(pos.x, pos.y, pos.z) == state;
        let has_block_entity = self
            .world
            .block_entities
            .contains_key(&(pos.x, pos.y, pos.z));
        if let Err(why) = fall_ready(loaded, matches, has_block_entity) {
            log::warn!("Horizon: server mod falling block at {pos:?}: {why}");
            return;
        }
        let spawned = self.native_spawn_falling_block(
            dimension,
            state,
            [
                f64::from(pos.x) + 0.5,
                f64::from(pos.y),
                f64::from(pos.z) + 0.5,
            ],
            [velocity.x, velocity.y, velocity.z],
            NativeFalling {
                drop_item: true,
                hurts,
            },
        );
        if spawned {
            let air = self.world.air;
            let _ = self.place_state([pos.x, pos.y, pos.z], air, false);
        } else {
            log::warn!("Horizon: server mod falling block at {pos:?} was refused");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blast_follows_the_griefing_rule() {
        for kind in [BlastKind::Keep, BlastKind::Block, BlastKind::Tnt] {
            for fire in [false, true] {
                assert_eq!(mod_blast(kind, false, fire), (NativeBlast::Keep, false));
                let expected = match kind {
                    BlastKind::Keep => NativeBlast::Keep,
                    BlastKind::Block => NativeBlast::Block,
                    BlastKind::Tnt => NativeBlast::Tnt,
                };
                assert_eq!(mod_blast(kind, true, fire), (expected, fire));
            }
        }
    }

    #[test]
    fn dust_channels_are_bytes_over_255() {
        assert_eq!(dust_rgb(0xFF8000), [1.0, 128.0 / 255.0, 0.0]);
        assert_eq!(dust_rgb(0), [0.0; 3]);
        assert_eq!(dust_rgb(0xFFFFFF), [1.0; 3]);
    }

    #[test]
    fn falls_need_a_loaded_matching_block_without_a_block_entity() {
        assert_eq!(fall_ready(true, true, false), Ok(()));
        assert_eq!(fall_ready(false, false, false), Err("not loaded"));
        assert_eq!(fall_ready(false, true, false), Err("not loaded"));
        assert_eq!(fall_ready(true, false, false), Err("block changed"));
        assert!(fall_ready(true, true, true).is_err());
    }
}
