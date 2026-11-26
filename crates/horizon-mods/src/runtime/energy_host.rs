//! API 39/44: shared energy storage on content blocks.
use super::*;
use server_bindings::horizon::mods::energy::{self, EnergyPos, EnergyStats};

const MAX_CAPACITY: u64 = 1_000_000_000;

impl energy::Host for State {
    fn energy_side(&mut self, dimension: String, pos: EnergyPos, face: i32) -> Option<EnergyStats> {
        self.energy_position(&dimension, &pos).ok()?;
        let view = self.world.lock().ok()?;
        let (amount, capacity) = view
            .as_ref()?
            .energy_side(&dimension, pos.x, pos.y, pos.z, face)?;
        let amount = self.projected(&dimension, (pos.x, pos.y, pos.z), amount);
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
        let current = self.saved_energy(&dimension, &pos)?;
        if current.1 != 0 && current.1 != capacity {
            return Err("energy capacity is already set".into());
        }
        self.queue_energy(
            dimension,
            pos,
            -1,
            current,
            EnergyKind::Capacity,
            capacity,
            None,
        )
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
        let (projected, capacity) = self.projected_with_capacity(&dimension, &pos, face)?;
        let queued = amount.min(capacity.saturating_sub(projected));
        if queued == 0 {
            return Ok(0);
        }
        let current = self.saved_energy(&dimension, &pos)?;
        self.reserve(&dimension, (pos.x, pos.y, pos.z), queued as i64);
        self.queue_energy(
            dimension,
            pos,
            face,
            current,
            EnergyKind::Insert,
            queued,
            None,
        )?;
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
        let (projected, _) = self.projected_with_capacity(&dimension, &pos, face)?;
        let queued = amount.min(projected);
        if queued == 0 {
            return Ok(0);
        }
        let current = self.saved_energy(&dimension, &pos)?;
        self.reserve(&dimension, (pos.x, pos.y, pos.z), -(queued as i64));
        self.queue_energy(
            dimension,
            pos,
            face,
            current,
            EnergyKind::Extract,
            queued,
            None,
        )?;
        Ok(queued)
    }

    fn transfer_energy(
        &mut self,
        from_dimension: String,
        from: EnergyPos,
        to_dimension: String,
        to: EnergyPos,
        amount: u64,
    ) -> Result<u64, String> {
        if amount == 0 {
            return Ok(0);
        }
        self.energy_position(&from_dimension, &from)?;
        self.energy_position(&to_dimension, &to)?;
        if from_dimension == to_dimension && from.x == to.x && from.y == to.y && from.z == to.z {
            return Err("cannot transfer energy to the same store".into());
        }
        let (source, source_capacity) = self.projected_with_capacity(&from_dimension, &from, -1)?;
        let (target, target_capacity) = self.projected_with_capacity(&to_dimension, &to, -1)?;
        if source_capacity == 0 || target_capacity == 0 {
            return Err("both endpoints must have energy storage".into());
        }
        let queued = amount
            .min(source)
            .min(target_capacity.saturating_sub(target));
        if queued == 0 {
            return Ok(0);
        }
        let before = self.saved_energy(&from_dimension, &from)?;
        self.reserve(&from_dimension, (from.x, from.y, from.z), -(queued as i64));
        self.reserve(&to_dimension, (to.x, to.y, to.z), queued as i64);
        self.queue_energy(
            from_dimension,
            from,
            -1,
            before,
            EnergyKind::Transfer,
            queued,
            Some((to_dimension, (to.x, to.y, to.z))),
        )?;
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
        if !world.has_dimension(dimension) || world.block(dimension, pos.x, pos.y, pos.z).is_none()
        {
            return Err("unknown dimension or unloaded position".into());
        }
        Ok(())
    }

    fn saved_energy(&self, dimension: &str, pos: &EnergyPos) -> Result<(u64, u64), String> {
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        view.as_ref()
            .ok_or("world unavailable")?
            .energy_side(dimension, pos.x, pos.y, pos.z, -1)
            .ok_or("block has no energy storage".into())
    }

    fn projected_with_capacity(
        &self,
        dimension: &str,
        pos: &EnergyPos,
        face: i32,
    ) -> Result<(u64, u64), String> {
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let (amount, capacity) = view
            .as_ref()
            .ok_or("world unavailable")?
            .energy_side(dimension, pos.x, pos.y, pos.z, face)
            .ok_or("block has no energy storage")?;
        Ok((
            self.projected(dimension, (pos.x, pos.y, pos.z), amount),
            capacity,
        ))
    }

    /// The saved amount plus this tick's already-promised changes.
    fn projected(&self, dimension: &str, pos: (i32, i32, i32), amount: u64) -> u64 {
        let Ok(ledger) = self.energy_ledger.lock() else {
            return amount;
        };
        if ledger.tick != self.snapshot.tick {
            return amount;
        }
        match ledger
            .deltas
            .get(&(dimension.to_owned(), pos.0, pos.1, pos.2))
        {
            Some(delta) if *delta >= 0 => amount.saturating_add(*delta as u64),
            Some(delta) => amount.saturating_sub((-*delta) as u64),
            None => amount,
        }
    }

    fn reserve(&self, dimension: &str, pos: (i32, i32, i32), delta: i64) {
        if let Ok(mut ledger) = self.energy_ledger.lock() {
            if ledger.tick != self.snapshot.tick {
                ledger.tick = self.snapshot.tick;
                ledger.deltas.clear();
            }
            *ledger
                .deltas
                .entry((dimension.to_owned(), pos.0, pos.1, pos.2))
                .or_default() += delta;
        }
    }

    fn queue_energy(
        &mut self,
        dimension: String,
        pos: EnergyPos,
        face: i32,
        before: (u64, u64),
        kind: EnergyKind,
        amount: u64,
        to: Option<(String, (i32, i32, i32))>,
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
            to,
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
