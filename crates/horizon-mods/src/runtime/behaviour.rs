//! API 29 opt-in behaviour callbacks. Missing exports and exhausted budgets allow.
use super::*;
use std::cell::{Cell, RefCell};

// The generated WIT type cannot carry a per-type derive here.
#[allow(clippy::derivable_impls)]
impl Default for BehaviourConfig {
    fn default() -> Self {
        Self {
            block_breaking: false,
            block_placing: false,
            block_interacted: false,
            block_neighbour_changed: false,
            block_random_tick: false,
            block_scheduled_tick: false,
            item_used_on_block: false,
            item_used: false,
            item_hit_entity: false,
            item_use_finished: false,
            item_picked_up: false,
            chat_sent_filter: false,
        }
    }
}
#[derive(Default)]
pub(super) struct Behaviour {
    pub config: BehaviourConfig,
    pub blocks: Vec<String>,
    pub items: Vec<String>,
    budgets: RefCell<[(Option<u64>, usize); 12]>,
    warned: Cell<u16>,
    bad_chat_warned: Cell<bool>,
    observations: RefCell<Vec<BlockObservation>>,
    scheduled: RefCell<Vec<ScheduledTick>>,
}
/// Validate a whole replacement watch list before installing it.
pub fn validate_watches(ids: &[String]) -> Result<(), String> {
    if ids.len() > 64 {
        return Err("at most 64 watched ids".into());
    }
    for id in ids {
        let Some((ns, path)) = id.split_once(':') else {
            return Err("watched ids need a namespace".into());
        };
        let namespace = !ns.is_empty()
            && ns.len() <= 64
            && ns
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b));
        let path_ok = path == "*"
            || (!path.is_empty()
                && path.len() <= 256
                && path
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-/".contains(&b)));
        if !namespace || !path_ok {
            return Err("expected exact id or namespace:*".into());
        }
    }
    Ok(())
}
fn watches(ids: &[String], state: &str) -> bool {
    let id = state.split('[').next().unwrap_or(state);
    ids.iter()
        .any(|p| p == id || p.strip_suffix('*').is_some_and(|ns| id.starts_with(ns)))
}
#[derive(Clone, Copy)]
pub enum BehaviourFamily {
    Breaking,
    Placing,
    Interacted,
    Neighbour,
    Random,
    Scheduled,
    ItemOn,
    ItemUse,
    ItemHit,
    ItemFinish,
    Pickup,
    ChatFilter,
}
impl BehaviourFamily {
    fn watches(self, b: &Behaviour, id: &str) -> bool {
        if matches!(self, Self::Pickup | Self::ChatFilter) {
            return true;
        }
        watches(
            if matches!(
                self,
                Self::ItemOn | Self::ItemUse | Self::ItemHit | Self::ItemFinish
            ) {
                &b.items
            } else {
                &b.blocks
            },
            id,
        )
    }
    fn available(self, hooks: &Hooks) -> bool {
        let Hooks::Server(m, _) = hooks else {
            return false;
        };
        match self {
            Self::Breaking => m.breaking.is_some(),
            Self::Placing => m.placing.is_some(),
            Self::Interacted => m.interacted.is_some(),
            Self::Neighbour => m.neighbour.is_some(),
            Self::Random => m.random.is_some(),
            Self::Scheduled => m.scheduled.is_some(),
            Self::ItemOn => m.item_used_on_block.is_some(),
            Self::ItemUse => m.item_used.is_some(),
            Self::ItemHit => m.item_hit_entity.is_some(),
            Self::ItemFinish => m.item_use_finished.is_some(),
            Self::Pickup => m.pickup.is_some(),
            Self::ChatFilter => m.chat_filter.is_some(),
        }
    }
    fn enabled(self, b: &Behaviour) -> bool {
        match self {
            Self::Breaking => b.config.block_breaking,
            Self::Placing => b.config.block_placing,
            Self::Interacted => b.config.block_interacted,
            Self::Neighbour => b.config.block_neighbour_changed,
            Self::Random => b.config.block_random_tick,
            Self::Scheduled => b.config.block_scheduled_tick,
            Self::ItemOn => b.config.item_used_on_block,
            Self::ItemUse => b.config.item_used,
            Self::ItemHit => b.config.item_hit_entity,
            Self::ItemFinish => b.config.item_use_finished,
            Self::Pickup => b.config.item_picked_up,
            Self::ChatFilter => b.config.chat_sent_filter,
        }
    }
}
impl Mods {
    /// A cheap filter before constructing a world loan or player snapshot.
    pub fn wants_behaviour(&self, family: BehaviourFamily, id: &str) -> bool {
        self.entries.iter().any(|e| {
            e.run.as_ref().is_some_and(|r| {
                let b = &r.store.data().behaviour;
                family.available(&r.hooks) && family.enabled(b) && family.watches(b, id)
            })
        })
    }
    pub fn item_used_on_block(
        &mut self,
        player: &ServerPlayer,
        item: &str,
        pos: BlockPos,
        state: &str,
        face: i32,
    ) -> bool {
        self.behaviour_decision(BehaviourFamily::ItemOn, item, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.item_used_on_block
            {
                let (v,) = f.call(
                    &mut r.store,
                    (player.clone(), item.into(), pos, state.into(), face),
                )?;
                return Ok(v == Decision::Cancel);
            }
            Ok(false)
        })
    }
    pub fn item_used(&mut self, player: &ServerPlayer, item: &str) -> bool {
        self.behaviour_decision(BehaviourFamily::ItemUse, item, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.item_used
            {
                let (v,) = f.call(&mut r.store, (player.clone(), item.into()))?;
                return Ok(v == Decision::Cancel);
            }
            Ok(false)
        })
    }
    pub fn item_hit_entity(
        &mut self,
        player: &ServerPlayer,
        item: &str,
        entity_kind: &str,
        entity_id: i32,
    ) -> bool {
        self.behaviour_decision(BehaviourFamily::ItemHit, item, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.item_hit_entity
            {
                let (v,) = f.call(
                    &mut r.store,
                    (player.clone(), item.into(), entity_kind.into(), entity_id),
                )?;
                return Ok(v == Decision::Cancel);
            }
            Ok(false)
        })
    }
    pub fn item_use_finished(&mut self, player: &ServerPlayer, item: &str) {
        let _ = self.behaviour_decision(BehaviourFamily::ItemFinish, item, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.item_use_finished
            {
                f.call(&mut r.store, (player.clone(), item.into()))?;
            }
            Ok(false)
        });
    }
    pub fn item_picked_up(&mut self, p: &ServerPlayer, item: &str) -> bool {
        self.behaviour_decision(BehaviourFamily::Pickup, "", |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.pickup
            {
                let (v,) = f.call(&mut r.store, (p.clone(), item.into()))?;
                return Ok(v == Decision::Cancel);
            }
            Ok(false)
        })
    }
    /// Compose replacements in load order; cancellation short circuits. No input mutation.
    pub fn chat_sent_filter(&mut self, p: &ServerPlayer, text: &str) -> Option<String> {
        let mut text = text.to_owned();
        for i in 0..self.entries.len() {
            let Some(r) = self.entries[i].run.as_ref() else {
                continue;
            };
            let Hooks::Server(m, _) = &r.hooks else {
                continue;
            };
            let Some(f) = m.chat_filter else {
                continue;
            };
            let s = r.store.data();
            let b = &s.behaviour;
            if !b.config.chat_sent_filter
                || !b.allowance(BehaviourFamily::ChatFilter, self.snapshot.tick, &s.id)
            {
                continue;
            }
            let result = self.call(i, FUEL_EVENT, |r| {
                let (v,) = f.call(&mut r.store, (p.clone(), text.clone()))?;
                Ok(v)
            });
            match result {
                Some(ChatDecision::Cancel) => return None,
                Some(ChatDecision::Replace(replacement))
                    if valid_chat_replacement(&replacement) =>
                {
                    text = replacement
                }
                Some(ChatDecision::Replace(_)) => {
                    if let Some(r) = self.entries[i].run.as_ref()
                        && !r.store.data().behaviour.bad_chat_warned.replace(true)
                    {
                        log::warn!(
                            "Horizon: mod {} invalid chat replacement; allowing",
                            r.store.data().id
                        );
                    }
                }
                _ => {}
            }
        }
        Some(text)
    }
    /// Rich interaction runs before the unchanged level-28 block-used callback.
    #[allow(clippy::too_many_arguments)]
    pub fn block_interacted(
        &mut self,
        p: &ServerPlayer,
        pos: BlockPos,
        state: &str,
        face: i32,
        hit: [f64; 3],
        hand: BehaviourHand,
        item: &str,
        sneaking: bool,
    ) -> bool {
        self.behaviour_decision(BehaviourFamily::Interacted, state, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.interacted
            {
                let (v,) = f.call(
                    &mut r.store,
                    (
                        p.clone(),
                        pos,
                        state.into(),
                        face,
                        hit[0],
                        hit[1],
                        hit[2],
                        hand,
                        item.into(),
                        sneaking,
                    ),
                )?;
                return Ok(v == Decision::Cancel);
            }
            Ok(false)
        })
    }
    /// True means cancel. Trap/timeout uses the normal hook fuel and stops that mod.
    pub fn block_breaking(&mut self, p: &ServerPlayer, pos: BlockPos, state: &str) -> bool {
        self.behaviour_decision(BehaviourFamily::Breaking, state, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.breaking
            {
                let (v,) = f.call(&mut r.store, (p.clone(), pos, state.into()))?;
                return Ok(v == Decision::Cancel);
            }
            Ok(false)
        })
    }
    pub fn block_placing(
        &mut self,
        p: &ServerPlayer,
        pos: BlockPos,
        state: &str,
        item: &str,
    ) -> bool {
        self.behaviour_decision(BehaviourFamily::Placing, state, |r| {
            if let Hooks::Server(m, _) = &r.hooks
                && let Some(f) = m.placing
            {
                let (v,) = f.call(&mut r.store, (p.clone(), pos, state.into(), item.into()))?;
                return Ok(v == Decision::Cancel);
            }
            Ok(false)
        })
    }
    fn behaviour_decision(
        &mut self,
        family: BehaviourFamily,
        id: &str,
        mut call: impl FnMut(&mut Running) -> wasmtime::Result<bool>,
    ) -> bool {
        for i in 0..self.entries.len() {
            let Some(r) = self.entries[i].run.as_mut() else {
                continue;
            };
            if !family.available(&r.hooks) {
                continue;
            }
            let b = &r.store.data().behaviour;
            if !family.enabled(b) || !family.watches(b, id) {
                continue;
            }
            if !b.allowance(family, self.snapshot.tick, &r.store.data().id) {
                continue;
            }
            if self.call(i, FUEL_EVENT, &mut call) == Some(true) {
                return true;
            }
        }
        false
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_exact_or_namespace_filters() {
        for bad in [
            "*",
            "",
            "minecraft",
            ":*",
            "mine*:foo",
            "minecraft:fo*",
            "Minecraft:stone",
            "minecraft:stone[x=y]",
        ] {
            assert!(validate_watches(&[bad.into()]).is_err(), "{bad}");
        }
        assert!(validate_watches(&vec!["a:b".into(); 65]).is_err());
        let ids = vec!["minecraft:obsidian".into(), "demo:*".into()];
        validate_watches(&ids).unwrap();
        assert!(watches(&ids, "minecraft:obsidian[foo=bar]"));
        assert!(watches(&ids, "demo:block"));
        assert!(!watches(&ids, "demolition:block"));
        assert!(!watches(&ids, "minecraft:tnt"));
    }
}

impl Behaviour {
    fn allowance(&self, family: BehaviourFamily, tick: u64, id: &str) -> bool {
        let n = family as usize;
        if channel_host::allowance(&mut self.budgets.borrow_mut()[n], tick, 32) {
            return true;
        }
        let bit = 1 << n;
        if self.warned.get() & bit == 0 {
            log::warn!("Horizon: mod {id} behaviour family {n} exceeded 32 calls/tick; allowing");
            self.warned.set(self.warned.get() | bit);
        }
        false
    }
}
struct BlockObservation {
    family: BehaviourFamily,
    dimension: String,
    pos: BlockPos,
    state: String,
    from: String,
}
struct ScheduledTick {
    dimension: String,
    pos: BlockPos,
    state: String,
    due: u64,
}
/// Pure schedule bounds; world/chunk/watch checks are made separately.
pub fn validate_schedule(pos: BlockChangePos, delay: u32) -> Result<(), String> {
    if !(1..=72000).contains(&delay) {
        return Err("delay must be 1..=72000 ticks".into());
    }
    if pos.x.unsigned_abs() > 30000000
        || pos.z.unsigned_abs() > 30000000
        || pos.y.unsigned_abs() > 20000000
    {
        return Err("schedule position outside world bounds".into());
    }
    Ok(())
}
impl State {
    pub(super) fn schedule_behaviour_tick(
        &mut self,
        dimension: String,
        pos: BlockChangePos,
        delay: u32,
    ) -> Result<(), String> {
        validate_schedule(pos, delay)?;
        if !self.server || !self.behaviour.config.block_scheduled_tick {
            return Err("scheduled behaviour hook is not enabled".into());
        }
        let view = self.world.lock().map_err(|_| "world unavailable")?;
        let view = view.as_ref().ok_or("world unavailable")?;
        if !view.has_dimension(&dimension) {
            return Err("unknown dimension".into());
        }
        let state = view
            .block(&dimension, pos.x, pos.y, pos.z)
            .ok_or("block is not loaded or outside build height")?;
        if !watches(&self.behaviour.blocks, &state) {
            return Err("block is not watched".into());
        }
        let mut pending = self.behaviour.scheduled.borrow_mut();
        if pending.len() >= 256 {
            return Err("at most 256 scheduled ticks per mod".into());
        }
        let due = self
            .snapshot
            .tick
            .checked_add(u64::from(delay))
            .ok_or("tick overflow")?;
        pending.push(ScheduledTick {
            dimension,
            pos: BlockPos {
                x: pos.x,
                y: pos.y,
                z: pos.z,
            },
            state,
            due,
        });
        Ok(())
    }
}
impl Mods {
    /// Called at the native hook point; reserve budget before allocating an observation.
    pub fn observe_behaviour_block(
        &self,
        family: BehaviourFamily,
        tick: u64,
        dimension: &str,
        pos: BlockPos,
        state: &str,
        from: &str,
    ) {
        if !matches!(family, BehaviourFamily::Neighbour | BehaviourFamily::Random) {
            return;
        }
        for e in &self.entries {
            let Some(r) = &e.run else {
                continue;
            };
            let Hooks::Server(m, _) = &r.hooks else {
                continue;
            };
            if (matches!(family, BehaviourFamily::Neighbour) && m.neighbour.is_none())
                || (matches!(family, BehaviourFamily::Random) && m.random.is_none())
            {
                continue;
            }
            let s = r.store.data();
            let b = &s.behaviour;
            if family.enabled(b) && watches(&b.blocks, state) && b.allowance(family, tick, &s.id) {
                let mut queue = b.observations.borrow_mut();
                // At most two families * 32; old observations never accumulate.
                if queue.len() < 64 {
                    queue.push(BlockObservation {
                        family,
                        dimension: dimension.into(),
                        pos,
                        state: state.into(),
                        from: from.into(),
                    });
                }
            }
        }
    }
    /// Invalidate even if a block changes away and back in the same engine turn.
    pub fn behaviour_block_changed(&self, dimension: &str, pos: BlockPos) {
        for e in &self.entries {
            if let Some(r) = &e.run {
                r.store
                    .data()
                    .behaviour
                    .scheduled
                    .borrow_mut()
                    .retain(|t| t.dimension != dimension || t.pos != pos);
            }
        }
    }
    pub fn behaviour_chunk_unloaded(&self, dimension: &str, chunk: [i32; 2]) {
        for e in &self.entries {
            if let Some(r) = &e.run {
                r.store
                    .data()
                    .behaviour
                    .scheduled
                    .borrow_mut()
                    .retain(|t| t.dimension != dimension || [t.pos.x >> 4, t.pos.z >> 4] != chunk);
            }
        }
    }
    /// End of the server turn, with the real loaded world lent. Never loads a chunk.
    pub fn flush_behaviour_ticks(&mut self) {
        for i in 0..self.entries.len() {
            let Some(r) = &self.entries[i].run else {
                continue;
            };
            let s = r.store.data();
            let b = &s.behaviour;
            let mut events = std::mem::take(&mut *b.observations.borrow_mut());
            {
                let view = self.world.lock().ok();
                let view = view.as_ref().and_then(|v| v.as_ref());
                b.scheduled.borrow_mut().retain(|t| {
                    if !view.is_some_and(|v| {
                        v.block(&t.dimension, t.pos.x, t.pos.y, t.pos.z).as_deref()
                            == Some(&t.state)
                    }) {
                        return false;
                    }
                    if t.due > self.snapshot.tick {
                        return true;
                    }
                    if b.config.block_scheduled_tick
                        && watches(&b.blocks, &t.state)
                        && b.allowance(BehaviourFamily::Scheduled, self.snapshot.tick, &s.id)
                    {
                        events.push(BlockObservation {
                            family: BehaviourFamily::Scheduled,
                            dimension: t.dimension.clone(),
                            pos: t.pos,
                            state: t.state.clone(),
                            from: String::new(),
                        });
                    }
                    false
                });
            }
            for e in events {
                if !self.entries[i].run.as_ref().is_some_and(|r| {
                    e.family.enabled(&r.store.data().behaviour)
                        && e.family.watches(&r.store.data().behaviour, &e.state)
                }) {
                    continue;
                }
                self.call(i, FUEL_EVENT, |r| {
                    let Hooks::Server(m, _) = &r.hooks else {
                        return Ok(());
                    };
                    match e.family {
                        BehaviourFamily::Neighbour => {
                            if let Some(f) = m.neighbour {
                                f.call(&mut r.store, (e.dimension, e.pos, e.state, e.from))?;
                            }
                        }
                        BehaviourFamily::Random => {
                            if let Some(f) = m.random {
                                f.call(&mut r.store, (e.dimension, e.pos, e.state))?;
                            }
                        }
                        BehaviourFamily::Scheduled => {
                            if let Some(f) = m.scheduled {
                                f.call(&mut r.store, (e.dimension, e.pos, e.state))?;
                            }
                        }
                        _ => {}
                    }
                    Ok(())
                });
            }
        }
    }
}

/// Vanilla chat characters and a Unicode scalar count, independent of UTF-8 bytes.
pub fn valid_chat_replacement(text: &str) -> bool {
    text.chars().take(257).count() <= 256
        && text
            .chars()
            .all(|c| c >= ' ' && c != '\u{7f}' && c != '\u{a7}')
}
#[cfg(test)]
mod validator_tests {
    use super::*;
    #[test]
    fn schedule_bounds_and_unicode_chat_are_checked_before_use() {
        let p = BlockChangePos { x: 0, y: 64, z: 0 };
        for delay in [1, 72000] {
            assert!(validate_schedule(p, delay).is_ok());
        }
        for delay in [0, 72001, u32::MAX] {
            assert!(validate_schedule(p, delay).is_err());
        }
        assert!(validate_schedule(BlockChangePos { x: i32::MIN, ..p }, 1).is_err());
        assert!(valid_chat_replacement(&"é".repeat(256)));
        assert!(!valid_chat_replacement(&"é".repeat(257)));
        for illegal in ["bad\nline", "a\u{a7}b", "a\u{7f}b", "a\tb"] {
            assert!(!valid_chat_replacement(illegal));
        }
    }
}

impl Mods {
    /// World-close boundary: pending work never migrates to the next save.
    pub fn clear_behaviour_ticks(&mut self) {
        for e in &mut self.entries {
            if let Some(r) = &mut e.run {
                let b = &mut r.store.data_mut().behaviour;
                b.scheduled.get_mut().clear();
                b.observations.get_mut().clear();
                *b.budgets.get_mut() = [(None, 0); 12];
            }
        }
    }
}
