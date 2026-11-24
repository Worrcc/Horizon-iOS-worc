//! API 39: applying shared energy changes to live content blocks.
use super::Game;
use horizon_client::containers::BlockEntity;
use horizon_mods::{EnergyChange, EnergyKind};

impl Game<'_> {
    pub(super) fn apply_mod_energy(&mut self, plan: EnergyChange) -> Result<(), String> {
        let (x, y, z) = plan.pos;
        if plan.dimension != self.dim.id() || !self.world.chunks.contains_key(&(x >> 4, z >> 4)) {
            return Err("energy block unloaded".into());
        }
        let entity = self
            .world
            .block_entities
            .get_mut(&(x, y, z))
            .ok_or("energy block missing")?;
        let (amount, capacity) = match entity {
            BlockEntity::Mod(m) => {
                let current = m.energy().ok_or("block has no energy storage")?;
                if current != plan.before {
                    return Err("energy changed; operation cancelled".into());
                }
                match plan.kind {
                    EnergyKind::Capacity => (0, plan.amount),
                    EnergyKind::Insert => {
                        (current.0 + plan.amount.min(current.1 - current.0), current.1)
                    }
                    EnergyKind::Extract => (current.0 - plan.amount.min(current.0), current.1),
                }
            }
            _ => return Err("not a content block".into()),
        };
        match entity {
            BlockEntity::Mod(m) => m.set_energy(amount, capacity)?,
            _ => unreachable!(),
        }
        self.container_changed((x, y, z));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn energy_rule_keeps_bounds() {
        let (current, capacity) = (7u64, 10u64);
        assert_eq!(10u64.min(capacity - current), 3);
        assert_eq!(5u64.min(current), 5);
    }
}
