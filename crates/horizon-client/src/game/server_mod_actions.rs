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
        Some(GuestUse::OnBlock(_) | GuestUse::Entity(_)) => {
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
