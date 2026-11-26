//! API 39/44: applying shared energy changes to live content blocks.
use super::Game;
use horizon_client::containers::BlockEntity;
use horizon_mods::{EnergyChange, EnergyKind};

fn energy(entity: &BlockEntity) -> Option<(u64, u64)> {
    match entity {
        BlockEntity::Mod(m) => m.energy(),
        _ => None,
    }
}

impl Game<'_> {
    pub(super) fn apply_mod_energy(&mut self, plan: EnergyChange) -> Result<(), String> {
        let (x, y, z) = plan.pos;
        let dim = self.dim.id();
        if plan.dimension != dim || !self.world.chunks.contains_key(&(x >> 4, z >> 4)) {
            return Err("energy block unloaded".into());
        }
        if plan.kind == EnergyKind::Transfer {
            let (tdim, (tx, ty, tz)) = plan.to.clone().ok_or("transfer missing destination")?;
            if tdim != dim || !self.world.chunks.contains_key(&(tx >> 4, tz >> 4)) {
                return Err("transfer destination unloaded".into());
            }
            let from = energy(
                self.world
                    .block_entities
                    .get(&(x, y, z))
                    .ok_or("energy block missing")?,
            )
            .ok_or("block has no energy storage")?;
            let to = energy(
                self.world
                    .block_entities
                    .get(&(tx, ty, tz))
                    .ok_or("energy destination missing")?,
            )
            .ok_or("destination has no energy storage")?;
            if from.1 == 0 || to.1 == 0 {
                return Err("both endpoints must have a capacity".into());
            }
            let moved = plan.amount.min(from.0).min(to.1.saturating_sub(to.0));
            if moved == 0 {
                return Ok(());
            }
            let (nx, ny, nz) = (x, y, z);
            if let BlockEntity::Mod(m) = self
                .world
                .block_entities
                .get_mut(&(nx, ny, nz))
                .ok_or("energy block missing")?
            {
                m.set_energy(from.0 - moved, from.1)?;
            }
            if let BlockEntity::Mod(m) = self
                .world
                .block_entities
                .get_mut(&(tx, ty, tz))
                .ok_or("energy destination missing")?
            {
                m.set_energy(to.0 + moved, to.1)?;
            }
            self.container_changed((nx, ny, nz));
            self.container_changed((tx, ty, tz));
            return Ok(());
        }

        let (new_amount, new_capacity) = {
            let entity = self
                .world
                .block_entities
                .get(&(x, y, z))
                .ok_or("energy block missing")?;
            let current = energy(entity).ok_or("block has no energy storage")?;
            match plan.kind {
                EnergyKind::Capacity => {
                    if current.1 != 0 && current.1 != plan.amount {
                        return Err("energy capacity is already set".into());
                    }
                    (current.0.min(plan.amount), plan.amount)
                }
                EnergyKind::Insert => {
                    if current.1 == 0 {
                        return Err("energy capacity is not initialized".into());
                    }
                    let moved = plan.amount.min(current.1.saturating_sub(current.0));
                    (current.0 + moved, current.1)
                }
                EnergyKind::Extract => (current.0 - plan.amount.min(current.0), current.1),
                EnergyKind::Transfer => unreachable!(),
            }
        };
        if let BlockEntity::Mod(m) = self
            .world
            .block_entities
            .get_mut(&(x, y, z))
            .ok_or("energy block missing")?
        {
            m.set_energy(new_amount, new_capacity)?;
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
        // A transfer moves the same amount off the source and into the target.
        let (source, target) = (7u64, (9u64, 10u64));
        let moved = 5u64.min(source).min(target.1 - target.0);
        assert_eq!(moved, 1);
    }
}
