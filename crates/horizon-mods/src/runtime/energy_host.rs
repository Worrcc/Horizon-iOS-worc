//! API 39: shared energy storage on content blocks.
use super::*;
use server_bindings::horizon::mods::energy::{self, EnergyPos, EnergyStats};

const MAX_CAPACITY: u64 = 1_000_000_000;

impl energy::Host for State {
    fn energy_side(
        &mut self,
        dimension: String,
        pos: EnergyPos,
        face: i32,
    ) -> Option<EnergyStats> {
        self.energy_position(&dimension, &pos).ok()?;
        let view = self.world.lock().ok()?;
        let (amount, capacity) = view.as_ref()?.energy_side(
            &dimension, pos.x, pos.y, pos.z, face,
        )?;
        Some(EnergyStats { amount, capacity })
    }

    fn set_energy_capacity(
        &mut self,
        dimension: String,
        pos: EnergyPos,
        capacity: u64,
    ) -> Result<(), String> {
        self.energy_position(&dimension, &pos)?;
        if capacity == 0 || capacity > MAX_CAPACITY {
            return Err("energy capacity must be 1..1,000,000,000".into());
        }
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let current = view
            .as_ref()
            .ok_or("world unavailable")?
            .energy_side(&dimension, pos.x, pos.y, pos.z, -1)
            .ok_or("block has no energy storage")?;
        if current.1 != 0 && current.1 != capacity {
            return Err("energy capacity is already set".into());
        }
        drop(view);
        self.queue_energy(dimension, pos, -1, current, EnergyKind::Capacity, capacity)
    }

    fn insert_energy(
        &mut self,
        dimension: String,
        pos: EnergyPos,
        face: i32,
        amount: u64,
    ) -> Result<u64, String> {
        if amount == 0 {
            return Ok(0);
        }
        self.energy_position(&dimension, &pos)?;
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let current = view
            .as_ref()
            .ok_or("world unavailable")?
            .energy_side(&dimension, pos.x, pos.y, pos.z, face)
            .ok_or("block has no energy storage")?;
        let queued = amount.min(current.1.saturating_sub(current.0));
        drop(view);
        if queued == 0 {
            return Ok(0);
        }
        self.queue_energy(dimension, pos, face, current, EnergyKind::Insert, queued)?;
        Ok(queued)
    }

    fn extract_energy(
        &mut self,
        dimension: String,
        pos: EnergyPos,
        face: i32,
        amount: u64,
    ) -> Result<u64, String> {
        if amount == 0 {
            return Ok(0);
        }
        self.energy_position(&dimension, &pos)?;
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let current = view
            .as_ref()
            .ok_or("world unavailable")?
            .energy_side(&dimension, pos.x, pos.y, pos.z, face)
            .ok_or("block has no energy storage")?;
        let queued = amount.min(current.0);
        drop(view);
        if queued == 0 {
            return Ok(0);
        }
        self.queue_energy(dimension, pos, face, current, EnergyKind::Extract, queued)?;
        Ok(queued)
    }
}

impl State {
    fn energy_position(&self, dimension: &str, pos: &EnergyPos) -> Result<(), String> {
        if !self.server {
            return Err("server only".into());
        }
        if !(-30_000_000..30_000_000).contains(&pos.x)
            || !(-30_000_000..30_000_000).contains(&pos.z)
        {
            return Err("position outside world limits".into());
        }
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let world = view.as_ref().ok_or("world unavailable")?;
        if !world.has_dimension(dimension) || world.block(dimension, pos.x, pos.y, pos.z).is_none() {
            return Err("unknown dimension or unloaded position".into());
        }
        Ok(())
    }

    fn queue_energy(
        &mut self,
        dimension: String,
        pos: EnergyPos,
        face: i32,
        before: (u64, u64),
        kind: EnergyKind,
        amount: u64,
    ) -> Result<(), String> {
        if !channel_host::allowance(&mut self.energy_budget, self.snapshot.tick, 64) {
            return Err("at most 64 energy operations per mod per tick".into());
        }
        self.out.push(Output::EnergyChange(EnergyChange {
            mod_id: self.id.clone(),
            dimension,
            pos: (pos.x, pos.y, pos.z),
            face,
            before,
            kind,
            amount,
        }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_bounds_are_shared_per_tick() {
        let mut budget = (None, 0);
        for tick in [1, 2] {
            for _ in 0..64 {
                assert!(channel_host::allowance(&mut budget, tick, 64));
            }
            assert!(!channel_host::allowance(&mut budget, tick, 64));
        }
    }
}
