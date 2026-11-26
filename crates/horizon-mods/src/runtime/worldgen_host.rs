//! API 43: deterministic worldgen feature sites.
use super::*;
use server_bindings::horizon::mods::worldgen::{self, FeatureSite};

/// splitmix64: a small, stable mixer. Not vanilla's RNG, but deterministic
/// across reloads and platforms for the same seed, chunk and salt.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn site(seed: u64, cx: i32, cz: i32, salt: u32) -> FeatureSite {
    let base = mix(seed
        ^ (salt as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (cx as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
        ^ (cz as u64).wrapping_mul(0x94d0_49bb_1331_11eb));
    let rotation = (mix(base) % 4) as u32;
    let mirror = mix(base ^ 1) & 1 == 1;
    let ox = (mix(base ^ 2) % 16) as i32;
    let oz = (mix(base ^ 3) % 16) as i32;
    FeatureSite {
        x: cx * 16 + ox,
        z: cz * 16 + oz,
        rotation,
        mirror,
    }
}

impl worldgen::Host for State {
    fn register_feature(
        &mut self,
        id: String,
        structure: String,
        spacing: u32,
        salt: u32,
    ) -> Result<(), String> {
        if !self.server {
            return Err("server only".into());
        }
        if !resource_id(&id) || !resource_id(&structure) {
            return Err("worldgen ids must be resource ids".into());
        }
        if !(1..=1024).contains(&spacing) {
            return Err("feature spacing must be 1..1024 chunks".into());
        }
        self.world
            .lock()
            .map_err(|_| "world unavailable")?
            .as_ref()
            .ok_or("world unavailable")?
            .check_structure(&structure)?;
        if !self.features.contains_key(&id) && self.features.len() >= 64 {
            return Err("at most 64 features per mod".into());
        }
        self.features.insert(
            id,
            WorldFeature {
                structure,
                spacing,
                salt,
            },
        );
        Ok(())
    }

    fn unregister_feature(&mut self, id: String) -> Result<(), String> {
        self.features.remove(&id);
        Ok(())
    }

    fn site_for_chunk(
        &mut self,
        dimension: String,
        chunk_x: i32,
        chunk_z: i32,
        feature: String,
    ) -> Option<FeatureSite> {
        if !self.server || !resource_id(&dimension) {
            return None;
        }
        // Only answer for chunks generated during the current tick. This
        // prevents a mod from racing chunk generation or placing in unloaded
        // terrain; set-blocks/place-structure remains the write path.
        if !self
            .snapshot
            .generated_chunks
            .iter()
            .any(|c| c.dimension == dimension && c.x == chunk_x && c.z == chunk_z)
        {
            return None;
        }
        let feature = self.features.get(&feature)?;
        let spacing = feature.spacing as i32;
        let region_x = chunk_x.div_euclid(spacing);
        let region_z = chunk_z.div_euclid(spacing);
        let region_hash = mix(self.snapshot.world_seed
            ^ (feature.salt as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
            ^ (region_x as u64).wrapping_mul(0xbf58_476d_1ce4_e5b9)
            ^ (region_z as u64).wrapping_mul(0x94d0_49bb_1331_11eb));
        let chosen_x = region_x * spacing + (region_hash % spacing as u64) as i32;
        let chosen_z = region_z * spacing + (mix(region_hash) % spacing as u64) as i32;
        if chunk_x != chosen_x || chunk_z != chosen_z {
            return None;
        }
        Some(site(
            self.snapshot.world_seed,
            chunk_x,
            chunk_z,
            feature.salt,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_are_stable_and_differ_by_seed_and_salt() {
        let a = site(42, 3, -2, 7);
        assert_eq!(a, site(42, 3, -2, 7));
        assert_ne!(a, site(43, 3, -2, 7));
        assert_ne!(a, site(42, 3, -2, 8));
        assert!((0..16).contains(&(a.x & 15)));
        assert!((0..16).contains(&(a.z & 15)));
    }
}
