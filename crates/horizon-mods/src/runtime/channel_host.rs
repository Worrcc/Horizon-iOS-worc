//! API 28: opt-in block use and mod-scoped, bounded messages.
use super::*;
mod bindings {
    wasmtime::component::bindgen!({ path: "../horizon-mod-sdk/wit", world: "channel-host", with: { "horizon:mods/server-hooks": super::server_bindings::horizon::mods::server_hooks } });
}
use bindings::horizon::mods::{channel, server_hooks};
pub const CHANNEL_DATA: usize = 65536;
pub const CHANNEL_MESSAGES: usize = 32;
/// Mod id -> online peer UUIDs; a client's server peer uses the empty string.
pub type ChannelPeers = HashMap<String, Vec<String>>;

pub(super) fn add_to_linker(linker: &mut Linker<State>) -> wasmtime::Result<()> {
    channel::add_to_linker::<State, HasSelf<State>>(linker, |s| s)?;
    server_hooks::add_to_linker::<State, HasSelf<State>>(linker, |s| s)
}
pub(super) fn callback(
    store: &mut Store<State>,
    instance: &Instance,
) -> Option<TypedFunc<(String, Vec<u8>), ()>> {
    let interface =
        instance.get_export_index(&mut *store, None, "horizon:mods/channel-events@0.1.0")?;
    let function = instance.get_export_index(&mut *store, Some(&interface), "message")?;
    instance.get_typed_func(store, function).ok()
}
fn name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/' | b'-')
        })
}
pub(super) fn allowance(budget: &mut (Option<u64>, usize), tick: u64, limit: usize) -> bool {
    if budget.0 != Some(tick) {
        *budget = (Some(tick), 0);
    }
    if budget.1 >= limit {
        return false;
    }
    budget.1 += 1;
    true
}
impl server_hooks::Host for State {
    fn set_behaviour_hooks(&mut self, config: BehaviourConfig) {
        if self.server {
            self.behaviour.config = config;
        }
    }
    fn watch_blocks(&mut self, ids: Vec<String>) -> Result<(), String> {
        validate_watches(&ids)?;
        if self.server {
            self.behaviour.blocks = ids;
        }
        Ok(())
    }
    fn watch_items(&mut self, ids: Vec<String>) -> Result<(), String> {
        validate_watches(&ids)?;
        if self.server {
            self.behaviour.items = ids;
        }
        Ok(())
    }
    fn set_block_uses(&mut self, enabled: bool, including_sneaking_item: bool) {
        if self.server {
            self.uses = (enabled, including_sneaking_item);
        }
    }
}
impl State {
    fn channel_send(
        &mut self,
        player: Option<String>,
        name: String,
        data: Vec<u8>,
    ) -> Result<(), String> {
        if !name_ok(&name) {
            return Err("channel name must be [a-z0-9_./-]{1,64}".into());
        }
        if data.len() > CHANNEL_DATA {
            return Err("channel payload exceeds 65536 bytes".into());
        }
        let peer = player.as_deref().unwrap_or("");
        if !self
            .snapshot
            .channel_peers
            .get(&self.id)
            .is_some_and(|p| p.iter().any(|p| p == peer))
        {
            return Err("peer has not negotiated this mod's channel".into());
        }
        if !allowance(
            &mut self.channel_budget,
            self.snapshot.tick,
            CHANNEL_MESSAGES,
        ) {
            return Err("at most 32 channel messages per mod per tick".into());
        }
        self.out.push(Output::Channel {
            mod_id: self.id.clone(),
            player,
            name,
            data,
        });
        Ok(())
    }
}
impl channel::Host for State {
    fn send(&mut self, name: String, data: Vec<u8>) -> Result<(), String> {
        if self.server {
            return Err("channel.send is client only".into());
        }
        self.channel_send(None, name, data)
    }
    fn send_to_player(
        &mut self,
        player: String,
        name: String,
        data: Vec<u8>,
    ) -> Result<(), String> {
        if !self.server {
            return Err("channel.send-to-player is server only".into());
        }
        let player = self
            .snapshot
            .players
            .iter()
            .find(|p| p.id == player || p.name == player)
            .ok_or("unknown or offline player")?
            .id
            .clone();
        self.channel_send(Some(player), name, data)
    }
}
impl Mods {
    /// Running mods that can receive channels; used for peer negotiation.
    pub fn channel_mods(&self) -> Vec<String> {
        self.entries
            .iter()
            .filter(|e| {
                e.run.is_some()
                    && e.manifest
                        .as_ref()
                        .is_some_and(|m| m.capabilities.contains(&crate::Capability::Channel))
            })
            .filter_map(|e| e.manifest.as_ref().map(|m| m.id.clone()))
            .collect()
    }
    /// Deliver only to the same mod id, with a receive budget independent of sends.
    pub fn channel_message(
        &mut self,
        id: &str,
        player: Option<&ServerPlayer>,
        name: &str,
        data: &[u8],
    ) {
        if !name_ok(name) || data.len() > CHANNEL_DATA {
            return;
        }
        let Some(i) = self.entries.iter().position(|e| {
            e.run.is_some()
                && e.manifest.as_ref().is_some_and(|m| {
                    m.id == id && m.capabilities.contains(&crate::Capability::Channel)
                })
        }) else {
            return;
        };
        let tick = self.snapshot.tick;
        let r = self.entries[i].run.as_mut().unwrap();
        if r.hooks.server().is_some() != player.is_some()
            || !allowance(
                &mut r.store.data_mut().channel_receive_budget,
                tick,
                CHANNEL_MESSAGES,
            )
        {
            return;
        }
        self.call(i, FUEL_EVENT, |r| {
            if let (Hooks::Server(m, _), Some(p)) = (&r.hooks, player) {
                if let Some(f) = m.channel {
                    f.call(&mut r.store, (p.clone(), name.into(), data.to_vec()))?;
                }
            } else if let Some(f) = r.store.data().channel_callback {
                f.call(&mut r.store, (name.into(), data.to_vec()))?;
            }
            Ok(())
        });
    }
    /// Skip the world loan entirely when no opted-in callback has budget for this use.
    pub fn wants_block_uses(&self, tick: u64, sneaking: bool, holding_item: bool) -> bool {
        self.entries.iter().any(|e| {
            e.run.as_ref().is_some_and(|r| {
                let s = r.store.data();
                s.uses.0
                    && (!sneaking || !holding_item || s.uses.1)
                    && !(s.use_budget.0 == Some(tick) && s.use_budget.1 >= 32)
                    && matches!(&r.hooks, Hooks::Server(m, _) if m.used.is_some())
            })
        })
    }
    /// Main hand only; callers validate reach, border, protection and spectator status first.
    pub fn block_used(
        &mut self,
        player: &ServerPlayer,
        pos: BlockPos,
        state: &str,
        sneaking: bool,
        holding_item: bool,
    ) -> bool {
        for i in 0..self.entries.len() {
            let Some(r) = self.entries[i].run.as_mut() else {
                continue;
            };
            let s = r.store.data_mut();
            if !s.uses.0 || (sneaking && holding_item && !s.uses.1) {
                continue;
            }
            let Hooks::Server(m, _) = &r.hooks else {
                continue;
            };
            let Some(f) = m.used else {
                continue;
            };
            if !allowance(&mut r.store.data_mut().use_budget, self.snapshot.tick, 32) {
                continue;
            }
            if self.call(i, FUEL_EVENT, |r| {
                r.store.data_mut().machines.anchor = Some((player.dimension.clone(), pos));
                let result = f.call(&mut r.store, (player.clone(), pos, state.into(), sneaking));
                r.store.data_mut().machines.anchor = None;
                let (used,) = result?;
                Ok(used)
            }) == Some(true)
            {
                return true;
            }
        }
        false
    }
}
