//! Polled player actions and effect changes for server mods.
use super::{Game, GuestUse, Hit, lan::LanHost, server_mods::run::uuid_string};
use horizon_client::{effects::EffectInstance, first_person::Hand};
use horizon_mods::{EffectChange, PlayerAttack, PlayerBlockHit, PlayerItemUse};

const MAX_ACTIONS: usize = horizon_mods::PLAYER_ACTIONS;

fn record_use(mods: &mut super::mods::ModsState, event: PlayerItemUse, source: Option<GuestUse>) {
    if mods.item_uses.len() >= MAX_ACTIONS {
        return;
    }
    let key = (event.player.clone(), event.hand.clone());
    match source {
        Some(GuestUse::Item(_)) => {
            if mods.use_fallbacks.remove(&key).as_deref() == Some(&event.item) {
                return;
            }
        }
        Some(GuestUse::OnBlock(_) | GuestUse::Entity(..)) => {
            mods.use_fallbacks.insert(key, event.item.clone());
        }
        None => {}
    }
    mods.item_uses.push(event);
}

fn record_attack(mods: &mut super::mods::ModsState, event: PlayerAttack) {
    if mods.attacks.len() < MAX_ACTIONS {
        mods.attacks.push(event);
    }
}

impl Game<'_> {
    /// Records before the item's own behaviour can replace or consume it. `as_guest`
    /// supplies the UUID, selected hand and sneaking state on every server path.
    pub(super) fn mod_item_used(&mut self, hit: Option<&Hit>) {
        if !self.mods.server || self.remote.is_some() || self.mods.item_uses.len() >= MAX_ACTIONS {
            return;
        }
        let Some(stack) = self.held() else { return };
        let item = stack.item.to_string();
        let player = self
            .lan_acting_uuid
            .map_or_else(|| self.player_uuid_string(), uuid_string);
        let hand = match self.hand {
            Hand::Main => "main_hand",
            Hand::Off => "off_hand",
        }
        .to_owned();
        let target = hit.map(|h| PlayerBlockHit {
            x: h.pos[0],
            y: h.pos[1],
            z: h.pos[2],
            face: match h.face {
                [0, -1, 0] => "down",
                [0, 1, 0] => "up",
                [0, 0, -1] => "north",
                [0, 0, 1] => "south",
                [-1, 0, 0] => "west",
                _ => "east",
            }
            .into(),
        });
        let event = PlayerItemUse {
            player,
            item,
            hand,
            sneaking: self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft),
            target,
        };
        record_use(&mut self.mods, event, self.guest_use);
    }

    pub(super) fn mod_melee_hit(&mut self, mob: u32, damage: f32) {
        if !self.mods.server || self.remote.is_some() || self.mods.attacks.len() >= MAX_ACTIONS {
            return;
        }
        let event = PlayerAttack {
            player: self
                .lan_acting_uuid
                .map_or_else(|| self.player_uuid_string(), uuid_string),
            item: self.inventory[self.selected]
                .as_ref()
                .map(|s| s.item.to_string()),
            mob: Some(mob),
            target_player: None,
            damage: damage.max(0.0),
        };
        record_attack(&mut self.mods, event);
    }

    /// PvP damage is measured as the victim; attribution belongs to the attacker,
    /// whose turn may already have ended (and whose item may break after the hit).
    pub(super) fn mod_player_hit(
        &mut self,
        player: String,
        item: Option<String>,
        target: String,
        damage: f32,
    ) {
        if !self.mods.server || self.remote.is_some() || self.mods.attacks.len() >= MAX_ACTIONS {
            return;
        }
        let event = PlayerAttack {
            player,
            item,
            mob: None,
            target_player: Some(target),
            damage: damage.max(0.0),
        };
        record_attack(&mut self.mods, event);
    }

    pub(super) fn mod_record_death(&mut self, death: horizon_mods::Death) {
        if self.mods.server
            && self.mods.deaths.len() < 256
            && !self.mods.deaths.iter().any(|d| d.id == death.id)
        {
            self.mods.deaths.push(death);
        }
    }
    pub(super) fn collect_mod_player_damage(
        &mut self,
        player: &str,
        events: Vec<(f32, String, Option<u32>)>,
    ) {
        if !self.mods.server {
            return;
        }
        let room = 256usize.saturating_sub(self.mods.player_damage.len());
        self.mods
            .player_damage
            .extend(
                events
                    .into_iter()
                    .take(room)
                    .map(
                        |(amount, source_kind, attacker_mob)| horizon_mods::PlayerDamage {
                            player: player.into(),
                            amount,
                            source_kind,
                            attacker_mob,
                        },
                    ),
            );
    }
    pub(super) fn mod_credit_player_hurts(&mut self, mob: u32) {
        let player = self
            .lan_acting_uuid
            .map_or_else(|| self.player_uuid_string(), uuid_string);
        if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob) {
            m.credit_mod_hurts(&player);
        }
    }

    pub(super) fn apply_mod_damage_item(
        &mut self,
        mut host: Option<&mut LanHost>,
        player: &str,
        hand: &str,
        amount: u32,
    ) {
        let hand = if hand == "off_hand" {
            Hand::Off
        } else {
            Hand::Main
        };
        let mut broken = None;
        self.with_mod_player(host.as_deref_mut(), player, |g| {
            g.with_hand(hand, |g| {
                let before = g.held().cloned();
                if before.as_ref().is_none_or(|stack| {
                    !horizon_client::stations::is_damageable(g.res.items(), stack)
                }) {
                    return;
                }
                g.damage_held(amount);
                if let Some(stack) = before
                    && g.held().is_none()
                {
                    g.eat_particles(&stack, 5);
                    if let Some(eye) = g.eye_position() {
                        broken = Some((stack, eye));
                    }
                }
            });
        });
        if let Some(host) = host.or(self.lan.as_mut()) {
            let guest = host
                .hub
                .guests
                .iter()
                .position(|g| g.placed && uuid_string(g.uuid) == player);
            if let Some(i) = guest {
                host.hub.send_inventory(i);
            }
            if let Some((stack, at)) = broken {
                let dimension = guest
                    .map(|i| host.hub.guests[i].dimension.clone())
                    .unwrap_or_else(|| self.dim.id().into());
                if let Some(stack) = host.hub.stack(&Some(stack)) {
                    use horizon_protocol::{
                        entity_data::{Particle, ParticleData},
                        packets::play::{self, Clientbound as C},
                    };
                    let packet = C::LevelParticles(play::LevelParticles {
                        particle: Particle {
                            id: 57,
                            data: Some(ParticleData::Item(stack)),
                        },
                        override_limiter: false,
                        always_show: false,
                        pos: at,
                        dist: [0.15; 3],
                        max_speed: [0.1; 3],
                        count: 5,
                        randomization: 0,
                    })
                    .encode();
                    for g in host.hub.guests.iter().filter(|g| {
                        g.placed
                            && g.dimension == dimension
                            && (0..3).map(|k| (g.position[k] - at[k]).powi(2)).sum::<f64>()
                                < 32.0 * 32.0
                    }) {
                        g.sender.send(packet.clone());
                    }
                }
            }
        }
    }

    /// Run as the real server player, including the integrated server's guest.
    pub(super) fn with_mod_player(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        f: impl FnOnce(&mut Self),
    ) {
        if self.player.is_some()
            && (self
                .lan_acting_uuid
                .is_some_and(|id| uuid_string(id) == player)
                || (self.options.execution_role.is_client()
                    && self.remote.is_none()
                    && self.player_uuid_string() == player))
        {
            f(self);
        } else if let Some(host) = host {
            self.with_mod_guest(host, player, f);
        } else if let Some(mut host) = self.lan.take() {
            self.with_mod_guest(&mut host, player, f);
            self.lan = Some(host);
        }
    }
    fn with_mod_guest(&mut self, host: &mut LanHost, player: &str, f: impl FnOnce(&mut Self)) {
        self.with_mod_guest_host(host, player, |g, _| f(g));
    }
    pub(super) fn with_mod_guest_host(
        &mut self,
        host: &mut LanHost,
        player: &str,
        f: impl FnOnce(&mut Self, &LanHost),
    ) {
        if let Some(conn) = host
            .hub
            .guests
            .iter()
            .find(|g| g.placed && uuid_string(g.uuid) == player)
            .map(|g| g.conn)
        {
            let back = self.dim;
            let dimension = host
                .hub
                .guest(conn)
                .map(|g| super::Dim::from_id(&g.dimension));
            if let Some(dim) = dimension
                && let Err(e) = self.enter_level(Some(host), dim)
            {
                log::warn!("server mod player level: {e}");
                return;
            }
            let heard = self.audio.capture.as_ref().map_or(0, Vec::len);
            self.as_guest_with_host(host, conn, f);
            // Capture only this operation's sounds, while its dimension is in place.
            // Older queued sounds still belong to the level we will return to.
            if let Some(capture) = self.audio.capture.as_mut() {
                let from = heard.min(capture.len());
                for sound in capture.drain(from..) {
                    host.hub.captured_sound(&sound, None);
                }
            }
            if self.dim != back
                && let Err(e) = self.enter_level(Some(host), back)
            {
                log::warn!("server mod restoring level: {e}");
            }
        }
    }

    pub(super) fn apply_mod_player_effect(
        &mut self,
        host: Option<&mut LanHost>,
        player: &str,
        effect: EffectChange,
    ) {
        if self.options.execution_role.is_client()
            && self.remote.is_none()
            && self.player.is_some()
            && self.player_uuid_string() == player
        {
            self.change_mod_effect(&effect);
            return;
        }
        // The hub can be lent out by a command; it is otherwise in self.lan.
        if let Some(host) = host {
            self.change_guest_mod_effect(host, player, &effect);
        } else if let Some(mut host) = self.lan.take() {
            self.change_guest_mod_effect(&mut host, player, &effect);
            self.lan = Some(host);
        }
    }

    fn change_guest_mod_effect(&mut self, host: &mut LanHost, player: &str, effect: &EffectChange) {
        let conn = host
            .hub
            .guests
            .iter()
            .find(|g| g.placed && uuid_string(g.uuid) == player)
            .map(|g| g.conn);
        if let Some(conn) = conn {
            self.as_guest(host, conn, |g| g.change_mod_effect(effect));
            // Existing vanilla effect packets update the owning client immediately.
            host.hub.sync_all_effects();
        }
    }

    fn change_mod_effect(&mut self, change: &EffectChange) {
        let Some(effect) = horizon_client::brewing::effect(&change.id) else {
            return;
        };
        if change.seconds == 0 {
            self.potions.effects.list.retain(|e| e.id != effect.id);
            self.apply_effect_status();
        } else {
            let ticks = change.seconds.saturating_mul(20).min(i32::MAX as u32) as i32;
            self.add_player_effect(EffectInstance::new(
                effect.id,
                ticks,
                i32::from(change.amplifier),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item_use(player: &str, hand: &str, x: i32) -> PlayerItemUse {
        PlayerItemUse {
            player: player.into(),
            item: "minecraft:stick".into(),
            hand: hand.into(),
            sneaking: true,
            target: Some(PlayerBlockHit {
                x,
                y: 64,
                z: 0,
                face: "up".into(),
            }),
        }
    }

    #[test]
    fn use_collection_keeps_first_256_with_target_hand_and_player() {
        let mut mods = super::super::mods::ModsState::default();
        for x in 0..300 {
            record_use(&mut mods, item_use("guest-uuid", "off_hand", x), None);
        }
        assert_eq!(mods.item_uses.len(), MAX_ACTIONS);
        assert_eq!(mods.item_uses[0], item_use("guest-uuid", "off_hand", 0));
        assert_eq!(mods.item_uses[255], item_use("guest-uuid", "off_hand", 255));
    }

    #[test]
    fn attack_collection_bounds_both_mob_and_player_hits_together() {
        let mut mods = super::super::mods::ModsState::default();
        for i in 0..300 {
            record_attack(
                &mut mods,
                PlayerAttack {
                    player: "attacker-uuid".into(),
                    item: None,
                    mob: (i % 2 == 0).then_some(i),
                    target_player: (i % 2 == 1).then(|| "victim-uuid".into()),
                    damage: i as f32,
                },
            );
        }
        assert_eq!(mods.attacks.len(), MAX_ACTIONS);
        assert_eq!(mods.attacks[0].mob, Some(0));
        assert_eq!(
            mods.attacks[255].target_player.as_deref(),
            Some("victim-uuid")
        );
        assert_eq!(mods.attacks[255].damage, 255.0);
    }

    #[test]
    fn item_packet_fallback_preserves_block_target_without_merging_players_or_hands() {
        let mut mods = super::super::mods::ModsState::default();
        let event = item_use("one", "main_hand", 1);
        record_use(
            &mut mods,
            event.clone(),
            Some(GuestUse::OnBlock(Hand::Main)),
        );
        let mut air = event.clone();
        air.target = None;
        record_use(
            &mut mods,
            item_use("two", "main_hand", 2),
            Some(GuestUse::Item(Hand::Main)),
        );
        record_use(
            &mut mods,
            item_use("one", "off_hand", 3),
            Some(GuestUse::Item(Hand::Off)),
        );
        record_use(&mut mods, air.clone(), Some(GuestUse::Item(Hand::Main)));
        assert_eq!(mods.item_uses.len(), 3);
        assert_eq!(mods.item_uses[0], event);
        // Only one continuation is consumed; another item click is a new use.
        record_use(&mut mods, air, Some(GuestUse::Item(Hand::Main)));
        assert_eq!(mods.item_uses.len(), 4);
    }
}
