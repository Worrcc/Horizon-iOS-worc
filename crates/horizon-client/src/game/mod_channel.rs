//! API 28 messages use custom payloads on every connection, including memory
//! connections to the integrated server. Offers contain only running mod ids.
use super::{Game, lan::LanHost};
use horizon_protocol::mod_channel as wire;

#[derive(Default)]
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) struct Peer {
    pub supported: bool,
    pub ids: Vec<String>,
    sent: Option<Vec<String>>,
    assembly: wire::Assembly,
    budget: (u64, usize),
}
#[cfg(feature = "mods")]
#[derive(Default)]
pub(super) struct Channels {
    peers: std::collections::HashMap<u128, Peer>,
    incoming: Vec<(u128, String, String, Vec<u8>)>,
    serial: u32,
    received: std::collections::HashMap<String, (u64, usize)>,
}
#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn guest_wasm_channel(&mut self, _host: &mut LanHost, _conn: u32, _data: &[u8]) {}
    pub(super) fn remote_wasm_channel(&mut self, _data: &[u8]) {}
}
#[cfg(feature = "mods")]
impl Game<'_> {
    pub(super) fn wasm_channel_peers(&self, host: Option<&LanHost>) -> horizon_mods::ChannelPeers {
        let mut peers = horizon_mods::ChannelPeers::new();
        if let Some(r) = self
            .remote
            .as_ref()
            .filter(|r| r.joined && r.reconfiguring.is_none())
        {
            for id in &r.wasm_channel.ids {
                peers.insert(id.clone(), vec![String::new()]);
            }
        } else if let Some(host) = host.or(self.lan.as_ref()) {
            for g in host.hub.guests.iter().filter(|g| g.placed) {
                if let Some(p) = self.mods.channel.peers.get(&g.uuid) {
                    for id in &p.ids {
                        peers
                            .entry(id.clone())
                            .or_default()
                            .push(super::server_mods::run::uuid_string(g.uuid));
                    }
                }
            }
        }
        peers
    }
    pub(super) fn announce_wasm_channels(&mut self) {
        use horizon_protocol::packets::{
            configuration::CustomPayload,
            play::{Clientbound as C, Serverbound as S},
        };
        let mut ids = self.mods.mods.channel_mods();
        ids.sort();
        ids.truncate(wire::MAX_MODS);
        if let Some(remote) = self.remote.as_mut() {
            if remote.joined
                && remote.reconfiguring.is_none()
                && remote.wasm_channel.supported
                && remote.wasm_channel.sent.as_ref() != Some(&ids)
            {
                remote.wasm_channel.sent = Some(ids.clone());
                self.send(S::CustomPayload(CustomPayload {
                    channel: wire::CHANNEL.into(),
                    data: wire::offer(&ids),
                }));
            }
        } else if let Some(host) = self.lan.as_ref() {
            self.mods
                .channel
                .peers
                .retain(|uuid, _| host.hub.guests.iter().any(|g| g.placed && g.uuid == *uuid));
            for g in host
                .hub
                .guests
                .iter()
                .filter(|g| g.placed && g.brand == "horizon")
            {
                let peer = self.mods.channel.peers.entry(g.uuid).or_default();
                if peer.sent.as_ref() != Some(&ids) {
                    peer.sent = Some(ids.clone());
                    host.hub.send_to(
                        g.conn,
                        &C::CustomPayload(CustomPayload {
                            channel: wire::CHANNEL.into(),
                            data: wire::offer(&ids),
                        }),
                    );
                }
            }
        }
    }
    pub(super) fn guest_wasm_channel(&mut self, host: &mut LanHost, conn: u32, data: &[u8]) {
        let Some(g) = host
            .hub
            .guest(conn)
            .filter(|g| g.placed && g.brand == "horizon")
        else {
            return;
        };
        let uuid = g.uuid;
        let Ok(packet) = wire::decode(data) else {
            return;
        };
        let peer = self.mods.channel.peers.entry(uuid).or_default();
        if peer.budget.0 != self.ticks {
            peer.budget = (self.ticks, 0);
        }
        // Bound offers and fragments before allocating or running any guest.
        if peer.budget.1 >= wire::MAX_MODS * 32 * 3 + 1 {
            return;
        }
        peer.budget.1 += 1;
        if let wire::Packet::Offer(ids) = packet {
            peer.ids = ids;
            peer.supported = true;
            return;
        }
        if let wire::Packet::Part { id, .. } = &packet
            && (!peer.ids.contains(id) || !self.mods.mods.channel_mods().contains(id))
        {
            return;
        }
        if let Some((id, name, data)) = peer.assembly.accept(packet)
            && self.mods.channel.incoming.len() < 256
        {
            let budget = self.mods.channel.received.entry(id.clone()).or_default();
            if budget.0 != self.ticks {
                *budget = (self.ticks, 0);
            }
            if budget.1 >= 32 {
                return;
            }
            budget.1 += 1;
            self.mods.channel.incoming.push((uuid, id, name, data));
        }
    }
    pub(super) fn run_wasm_channels(&mut self, players: &[horizon_mods::ServerPlayer]) {
        for (uuid, id, name, data) in std::mem::take(&mut self.mods.channel.incoming) {
            if let Some(player) = players
                .iter()
                .find(|p| p.id == super::server_mods::run::uuid_string(uuid))
            {
                self.with_machine_mods(None, |mods| {
                    mods.channel_message(&id, Some(player), &name, &data)
                });
            }
        }
    }
    pub(super) fn remote_wasm_channel(&mut self, data: &[u8]) {
        let Ok(packet) = wire::decode(data) else {
            return;
        };
        let Some(remote) = self.remote.as_mut() else {
            return;
        };
        let peer = &mut remote.wasm_channel;
        if peer.budget.0 != self.ticks {
            peer.budget = (self.ticks, 0);
        }
        if peer.budget.1 >= wire::MAX_MODS * 32 * 3 + 1 {
            return;
        }
        peer.budget.1 += 1;
        if let wire::Packet::Offer(ids) = packet {
            peer.ids = ids;
            peer.supported = true;
            return;
        }
        if let wire::Packet::Part { id, .. } = &packet
            && !peer.ids.contains(id)
        {
            return;
        }
        if let Some((id, name, data)) = peer.assembly.accept(packet) {
            let snapshot = self.mod_snapshot_with(Vec::new());
            self.mods.mods.set_snapshot(snapshot);
            self.lend_client_world(false);
            self.with_mod_container(|mods| mods.channel_message(&id, None, &name, &data));
            self.return_client_world();
            self.show_mod_output();
        }
    }
    pub(super) fn send_wasm_channel(
        &mut self,
        host: Option<&mut LanHost>,
        id: &str,
        player: Option<&str>,
        name: &str,
        data: &[u8],
    ) {
        use horizon_protocol::packets::{
            configuration::CustomPayload,
            play::{Clientbound as C, Serverbound as S},
        };
        self.mods.channel.serial = self.mods.channel.serial.wrapping_add(1);
        let Ok(parts) = wire::message(id, name, self.mods.channel.serial, data) else {
            return;
        };
        if let Some(player) = player {
            let Some(host) = host.or(self.lan.as_mut()) else {
                return;
            };
            let Some(g) = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && super::server_mods::run::uuid_string(g.uuid) == player)
            else {
                return;
            };
            if !self
                .mods
                .channel
                .peers
                .get(&g.uuid)
                .is_some_and(|p| p.ids.iter().any(|p| p == id))
            {
                return;
            }
            for data in parts {
                host.hub.send_to(
                    g.conn,
                    &C::CustomPayload(CustomPayload {
                        channel: wire::CHANNEL.into(),
                        data,
                    }),
                );
            }
        } else if self.remote.as_ref().is_some_and(|r| {
            r.joined && r.reconfiguring.is_none() && r.wasm_channel.ids.iter().any(|p| p == id)
        }) {
            for data in parts {
                self.send(S::CustomPayload(CustomPayload {
                    channel: wire::CHANNEL.into(),
                    data,
                }));
            }
        }
    }
}
