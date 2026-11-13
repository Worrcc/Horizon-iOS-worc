//! Server mods in the game (`docs/mods.md`, "Server mods"): the game that
//! simulates the world (in-frame single player, the server thread, a LAN
//! host, the dedicated server) runs them. At the end of each tick it tells
//! them who joined and left, then what players said, broke and placed since
//! the last one, then ticks them; a player's command reaches them at once.
//! Their lines go to the players they name. With no server mod on, each hook
//! here is one check.
use super::{Game, lan::LanHost};
#[cfg(not(feature = "mods"))]
use horizon_world::registry::BlockStateId;

/// Who did something a server mod hears about, resolved at the tick's end.
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) enum Who {
    /// The window's own player, in-frame.
    Local,
    /// A connection's player (the server thread's owner is one).
    Conn(u32),
    Uuid(u128),
}

/// What players did during a tick, for the server mods at its end.
#[cfg_attr(not(feature = "mods"), allow(dead_code))]
pub(super) enum ModEvent {
    Chat(Who, String),
    Block {
        who: Who,
        pos: [i32; 3],
        state: String,
        placed: bool,
        exclude: Option<String>,
    },
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn tick_server_mods(&mut self) {}
    pub(super) fn guest_mod_chat_filter(
        &mut self,
        _host: &mut LanHost,
        _sender: u128,
        text: &str,
    ) -> Option<String> {
        Some(text.into())
    }
    pub(super) fn guest_mod_block_used(
        &mut self,
        _host: &mut LanHost,
        _conn: u32,
        _hit: &super::Hit,
        _hand: horizon_client::first_person::Hand,
    ) -> (bool, bool) {
        (false, false)
    }
    pub(super) fn mod_use_consumed(&mut self, _conn: u32) -> bool {
        false
    }
    pub(super) fn mod_block_event(&mut self, _pos: [i32; 3], _state: BlockStateId, _placed: bool) {}
    pub(super) fn mod_player_chat(&mut self, _who: Who, _text: &str) {}
    pub(super) fn local_server_mod_command(&mut self, _line: &str) -> bool {
        false
    }
    pub(super) fn guest_server_mod_command_quiet(
        &mut self,
        _host: &mut LanHost,
        _conn: u32,
        _line: &str,
    ) -> bool {
        false
    }
    pub(super) fn guest_server_mod_command(
        &mut self,
        _host: &mut LanHost,
        _conn: u32,
        _line: &str,
    ) -> bool {
        false
    }
    pub(super) fn apply_mod_block_writes(&mut self) {}
    pub(super) fn apply_mod_orders(&mut self) {}
    pub(super) fn mod_item_used(&mut self, _hit: Option<&super::Hit>) {}
    pub(super) fn mod_melee_hit(&mut self, _mob: u32, _damage: f32) {}
    pub(super) fn mod_mob_interacted(&mut self, _index: usize, _item: Option<String>) {}
}

#[cfg(feature = "mods")]
pub(super) mod run {
    use super::{Game, LanHost, ModEvent, Who};
    use horizon_client::chunk_map::ChunkMap;
    use horizon_client::mobs::orders::DATA_ENTRIES;
    use horizon_client::mobs::{Damage, DamageKind, Mob, Who as MobWho};
    use horizon_client::{
        command::args::GameMode as Mode,
        lan::data::component_nbt,
        text::{self, Text},
    };
    use horizon_core::geometry::Vec3;
    use horizon_mods::{
        ArgumentKind, BlockPos, EntityArea, EntityOrder, EntityTarget, GameMode, ModEntity, Output,
        ServerCommand, ServerPlayer, Snapshot, WorldView,
    };
    use horizon_protocol::packets::play::{self, Clientbound as C};
    use horizon_world::registry::{BlockRegistry, BlockStateId};
    use serde_json::{Value, json};

    pub(in crate::game) fn face_id(face: [i32; 3]) -> i32 {
        match face {
            [0, -1, 0] => 0,
            [0, 1, 0] => 1,
            [0, 0, -1] => 2,
            [0, 0, 1] => 3,
            [-1, 0, 0] => 4,
            _ => 5,
        }
    }
    pub(in crate::game) fn dimension_id(d: horizon_world::level::Dimension) -> &'static str {
        match d {
            horizon_world::level::Dimension::Overworld => "minecraft:overworld",
            horizon_world::level::Dimension::Nether => "minecraft:the_nether",
            horizon_world::level::Dimension::End => "minecraft:the_end",
        }
    }
    /// Blocks queued for levels that don't tick (another dimension, in-frame)
    /// are kept up to this many; the oldest go first.
    const MAX_QUEUED_WRITES: usize = 1 << 16;
    /// Orders to mobs in levels that don't tick are kept up to this many, as blocks are.
    const MAX_QUEUED_ORDERS: usize = 1 << 14;
    /// Spawns waiting for their level, at most.
    const MAX_QUEUED_SPAWNS: usize = 1024;
    /// Right clicks on content mods' mobs kept for one tick, at most.
    const MAX_INTERACTIONS: usize = 256;

    /// A server mod's `entities.spawn`, waiting for its dimension's level.
    pub(in crate::game) struct ModSpawn {
        dimension: String,
        kind: String,
        at: [f64; 3],
        yaw: f32,
        data: Vec<(String, String)>,
    }

    /// A mob as capability `entities` reads it.
    fn mod_entity(m: &Mob, dimension: &str, rider: Option<&String>) -> ModEntity {
        ModEntity {
            id: m.id,
            kind: m.kind.id.clone(),
            dimension: dimension.to_owned(),
            x: m.position.x,
            y: m.position.y,
            z: m.position.z,
            yaw: m.body_yaw,
            health: m.health,
            max_health: horizon_client::mobs::finalize::max_health(m),
            on_ground: m.on_ground,
            target: match m.ai.target {
                Some(MobWho::Mob(id)) => Some(id),
                _ => None,
            },
            target_player: m.ai.target == Some(MobWho::Player),
            scale: horizon_client::mobs::finalize::scale(m) as f32,
            data: (m.orders.data.iter())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            age: m.age,
            texture: m.orders.texture.as_deref().map(str::to_owned),
            rider: rider.map(|player| {
                let input = m.rider.unwrap_or_default();
                horizon_mods::EntityRider {
                    player: player.clone(),
                    forward: input.forward,
                    strafe: input.strafe,
                    jumping: input.jumping,
                    sprinting: input.sprint,
                    yaw: input.yaw,
                    pitch: input.pitch,
                }
            }),
            animation: m
                .orders
                .animation
                .as_ref()
                .map(|(name, _)| name.to_string()),
        }
    }

    /// Capability `world`'s view during a call: the levels' chunks, moved out
    /// of the game for the call and back after it (no copy).
    struct LentWorld {
        registry: &'static BlockRegistry,
        block_tags:
            std::sync::Arc<std::collections::HashMap<String, std::collections::HashSet<String>>>,
        biomes: std::sync::Arc<horizon_client::biome::Biomes>,
        dims: Vec<(&'static str, ChunkMap)>,
        containers: super::super::server_mod_world::ContainerSnapshot,
        storage: horizon_mods::StorageView,
        items: std::sync::Arc<std::collections::HashMap<String, u32>>,
        resources: std::path::PathBuf,
        world_root: std::path::PathBuf,
    }
    impl WorldView for LentWorld {
        fn storage(&mut self) -> Option<&mut horizon_mods::StorageView> {
            Some(&mut self.storage)
        }
        fn block_has_tag(&self, block: &str, tag: &str) -> bool {
            let Ok(state) = crate::game::render_control::parse_state(self.registry, block) else {
                return false;
            };
            let Some(state) = self.registry.state(state) else {
                return false;
            };
            self.block_tags
                .get(tag)
                .is_some_and(|t| t.contains(&state.name))
        }
        fn container(
            &self,
            dimension: &str,
            x: i32,
            y: i32,
            z: i32,
        ) -> Option<Vec<Option<horizon_mods::ContainerItemStack>>> {
            self.containers
                .get(&(dimension.to_owned(), (x, y, z)))
                .cloned()
        }
        fn check_item_stack(&self, item: &str, count: u32) -> Result<(), String> {
            let max = self
                .items
                .get(item)
                .ok_or_else(|| format!("unknown item {item}"))?;
            if count == 0 || count > *max {
                return Err(format!("invalid count for {item}: {count} (max {max})"));
            }
            Ok(())
        }
        fn check_give_stack(&self, item: &str, count: u32) -> Result<(), String> {
            let max = self
                .items
                .get(item)
                .ok_or_else(|| format!("unknown item {item}"))?;
            if *max == 0 || count > max.saturating_mul(100) {
                return Err("give-item is at most 100 stacks per call".into());
            }
            Ok(())
        }
        fn check_structure(&self, id: &str) -> Result<(), String> {
            if super::super::server_mod_world::structure_exists(
                &self.resources,
                &self.world_root,
                id,
            ) {
                Ok(())
            } else {
                Err(format!("no structure template {id}"))
            }
        }
        fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            if y < chunk.min_y() || y >= chunk.max_y() {
                return None;
            }
            Some(state_string(self.registry, chunk.block(x & 15, y, z & 15)))
        }
        fn biome(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            Some(self.biomes.get(chunk.biome(x, y, z)).name.clone())
        }
        fn can_see_sky(&self, dimension: &str, x: i32, y: i32, z: i32) -> bool {
            self.dims
                .iter()
                .find(|(d, _)| *d == dimension)
                .is_some_and(|(_, chunks)| {
                    horizon_client::mod_world::can_see_sky(chunks, dimension, x, y, z)
                })
        }
        fn has_dimension(&self, dimension: &str) -> bool {
            matches!(
                dimension,
                "minecraft:overworld" | "minecraft:the_nether" | "minecraft:the_end"
            )
        }
        fn check_state(&self, state: &str) -> Result<(), String> {
            crate::game::render_control::parse_state(self.registry, state)
                .map(drop)
                .map_err(|e| format!("{state}: {e}"))
        }
    }

    /// Server mods' commands as a root of the commands report
    /// (`lan::commands`): a literal per command, then its arguments in a
    /// chain, each node runnable where only optional arguments follow.
    fn command_tree<'a>(commands: impl Iterator<Item = &'a ServerCommand>) -> Value {
        let mut root = serde_json::Map::new();
        for c in commands {
            let mut next: Option<(String, Value)> = None;
            for (i, a) in c.arguments.iter().enumerate().rev() {
                let executable = c.arguments.get(i + 1).is_none_or(|b| b.optional);
                let (parser, properties) = match a.kind {
                    ArgumentKind::Word => ("brigadier:string", json!({"type": "word"})),
                    ArgumentKind::Text => ("brigadier:string", json!({"type": "greedy"})),
                    ArgumentKind::Integer((min, max)) => {
                        let mut p = serde_json::Map::new();
                        if let Some(min) = min {
                            p.insert("min".into(), min.into());
                        }
                        if let Some(max) = max {
                            p.insert("max".into(), max.into());
                        }
                        ("brigadier:integer", Value::Object(p))
                    }
                    ArgumentKind::Number => ("brigadier:double", Value::Null),
                    ArgumentKind::Boolean => ("brigadier:bool", Value::Null),
                    ArgumentKind::Player => (
                        "minecraft:entity",
                        json!({"amount": "single", "type": "players"}),
                    ),
                    ArgumentKind::BlockPos => ("minecraft:block_pos", Value::Null),
                };
                let mut node = json!({
                    "type": "argument",
                    "parser": parser,
                    "properties": properties,
                    "executable": executable,
                });
                if let Some((name, child)) = next.take() {
                    node["children"] = json!({ name: child });
                }
                next = Some((a.name.clone(), node));
            }
            let executable = c.arguments.first().is_none_or(|a| a.optional);
            let mut literal = json!({"type": "literal", "executable": executable});
            if let Some((name, child)) = next {
                literal["children"] = json!({ name: child });
            }
            root.insert(c.name.clone(), literal);
        }
        if root.is_empty() {
            return Value::Null;
        }
        json!({"type": "root", "children": root})
    }

    /// `minecraft:oak_log[axis=y]`.
    pub(in crate::game) fn state_string(registry: &BlockRegistry, state: BlockStateId) -> String {
        let Some(s) = registry.state(state) else {
            return "minecraft:air".into();
        };
        if s.properties.is_empty() {
            return s.name.clone();
        }
        let props: Vec<String> = s
            .properties
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        format!("{}[{}]", s.name, props.join(","))
    }

    /// `UUID.toString()`.
    pub(in crate::game) fn uuid_string(u: u128) -> String {
        let h = format!("{u:032x}");
        format!(
            "{}-{}-{}-{}-{}",
            &h[..8],
            &h[8..12],
            &h[12..16],
            &h[16..20],
            &h[20..]
        )
    }

    fn mode(id: i32) -> GameMode {
        match id {
            1 => GameMode::Creative,
            2 => GameMode::Adventure,
            3 => GameMode::Spectator,
            _ => GameMode::Survival,
        }
    }

    impl Game<'_> {
        /// The players on this game's server: the window's own player
        /// in-frame, then every connection's. `host` is the hub when it is
        /// out of `self.lan`.
        pub(in crate::game) fn server_players(&self, host: Option<&LanHost>) -> Vec<ServerPlayer> {
            let mut out = Vec::new();
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && let Some(p) = &self.player
            {
                out.push(ServerPlayer {
                    id: self.player_uuid_string(),
                    name: crate::game::chat::PLAYER_NAME.to_owned(),
                    x: p.position.x,
                    y: p.position.y,
                    z: p.position.z,
                    dimension: self.dim.id().to_owned(),
                    game_mode: match self.mode {
                        Mode::Survival => GameMode::Survival,
                        Mode::Creative => GameMode::Creative,
                        Mode::Adventure => GameMode::Adventure,
                        Mode::Spectator => GameMode::Spectator,
                    },
                    health: p.health.health(),
                });
            }
            let guests = host
                .or(self.lan.as_ref())
                .into_iter()
                .flat_map(|h| &h.hub.guests);
            out.extend(guests.filter(|g| g.placed).map(|g| ServerPlayer {
                id: uuid_string(g.uuid),
                name: g.name.clone(),
                x: g.position[0],
                y: g.position[1],
                z: g.position[2],
                dimension: g.dimension.clone(),
                game_mode: mode(g.game_mode),
                health: g.health,
            }));
            out
        }

        /// Whether what players do now reaches server mods: one is running
        /// and this game simulates the world.
        fn player_looks(&self, host: Option<&LanHost>) -> Vec<(String, horizon_mods::PlayerLook)> {
            let mut out = Vec::new();
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.is_some()
            {
                let look = horizon_mods::PlayerLook {
                    yaw: self.yaw,
                    pitch: self.pitch,
                };
                out.push((self.player_uuid_string(), look));
            }
            let guests = host
                .or(self.lan.as_ref())
                .into_iter()
                .flat_map(|h| &h.hub.guests);
            out.extend(guests.filter(|g| g.placed).map(|g| {
                let look = horizon_mods::PlayerLook {
                    yaw: g.yaw,
                    pitch: g.pitch,
                };
                (uuid_string(g.uuid), look)
            }));
            out
        }

        /// Whether what players do now reaches server mods: one is running
        /// and this game simulates the world.
        fn hears_players(&self) -> bool {
            self.mods.server && self.remote.is_none()
        }

        pub(in crate::game) fn mod_use_consumed(&mut self, conn: u32) -> bool {
            self.mods
                .consumed_uses
                .retain(|_, tick| *tick == self.ticks);
            self.mods.consumed_uses.contains_key(&conn)
        }
        pub(in crate::game) fn guest_mod_block_used(
            &mut self,
            host: &mut LanHost,
            conn: u32,
            hit: &super::super::Hit,
            hand: horizon_client::first_person::Hand,
        ) -> (bool, bool) {
            if self.mod_use_consumed(conn) {
                return (true, true);
            }
            if !self.hears_players() {
                return (false, false);
            }
            let Some(g) = host.hub.guest(conn) else {
                return (false, false);
            };
            if g.spectator() {
                return (false, false);
            }
            let at = hit.pos;
            let main = hand == horizon_client::first_person::Hand::Main;
            let item = if main {
                g.inventory.get(g.selected).and_then(Option::as_ref)
            } else {
                g.offhand.as_ref()
            }
            .map(|s| s.item.to_string())
            .unwrap_or_else(|| "minecraft:air".into());
            let sneaking = g.sneaking;
            let holding = g
                .inventory
                .get(g.selected)
                .and_then(Option::as_ref)
                .is_some_and(|s| s.count > 0);
            let id = uuid_string(g.uuid);
            let Some(chunk) = self.world.chunks.get(&(at[0] >> 4, at[2] >> 4)) else {
                return (false, false);
            };
            let state = state_string(
                self.pack.assets.registry,
                chunk.block(at[0] & 15, at[1], at[2] & 15),
            );
            if !(main
                && self
                    .mods
                    .mods
                    .wants_block_uses(self.ticks, sneaking, holding))
                && !self
                    .mods
                    .mods
                    .wants_behaviour(horizon_mods::BehaviourFamily::Interacted, &state)
            {
                return (false, false);
            }
            let players = self.server_players(Some(host));
            let Some(player) = players.iter().find(|p| p.id == id).cloned() else {
                return (false, false);
            };
            let mut snapshot = self.server_snapshot(players, Some(host));
            snapshot.channel_peers = self.wasm_channel_peers(Some(host));
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world(Some(host));
            self.lend_mod_inventories(Some(host));
            let pos = BlockPos {
                x: at[0],
                y: at[1],
                z: at[2],
            };
            let face = match hit.face {
                [0, -1, 0] => 0,
                [0, 1, 0] => 1,
                [0, 0, -1] => 2,
                [0, 0, 1] => 3,
                [-1, 0, 0] => 4,
                _ => 5,
            };
            let relative = std::array::from_fn(|i| hit.point[i] - f64::from(at[i]));
            let rich = self.mods.mods.block_interacted(
                &player,
                pos,
                &state,
                face,
                relative,
                if main {
                    horizon_mods::BehaviourHand::Main
                } else {
                    horizon_mods::BehaviourHand::Off
                },
                &item,
                sneaking,
            );
            let used = rich
                || (main
                    && self.mods.mods.block_used(
                        &player,
                        BlockPos {
                            x: at[0],
                            y: at[1],
                            z: at[2],
                        },
                        &state,
                        sneaking,
                        holding,
                    ));
            self.return_world();
            self.return_mod_inventories(Some(host));
            self.route_mod_output(Some(host));
            self.apply_mod_block_changes();
            self.apply_mod_loot_tables();
            self.apply_mod_world_operations(Some(host));
            if used {
                self.mods.consumed_uses.insert(conn, self.ticks);
                if rich {
                    self.mods.restore_uses.insert(conn);
                }
            }
            (used, rich)
        }

        pub(in crate::game) fn guest_mod_chat_filter(
            &mut self,
            host: &mut LanHost,
            sender: u128,
            text: &str,
        ) -> Option<String> {
            let players = self.server_players(Some(host));
            let Some(player) = players
                .iter()
                .find(|p| p.id == uuid_string(sender))
                .cloned()
            else {
                return Some(text.into());
            };
            let snapshot = self.server_snapshot(players, Some(host));
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world(Some(host));
            let text = self.mods.mods.chat_sent_filter(&player, text);
            self.return_world();
            self.route_mod_output(Some(host));
            text
        }
        /// The acting player's state is swapped in by the existing native hook path.
        /// Outputs are queued until the ordinary server-mod flush, so a decision
        /// cannot change the candidate block or shrink the hand under the engine.
        pub(in crate::game) fn mod_behaviour(
            &mut self,
            family: horizon_mods::BehaviourFamily,
            id: &str,
            f: impl FnOnce(&mut horizon_mods::Mods, &ServerPlayer) -> bool,
        ) -> bool {
            if !self.hears_players() || !self.mods.mods.wants_behaviour(family, id) {
                return false;
            }
            let Some(p) = self.player.as_ref() else {
                return false;
            };
            let player = ServerPlayer {
                id: self
                    .lan_acting_uuid
                    .map(uuid_string)
                    .unwrap_or_else(|| self.player_uuid_string()),
                name: self.acting_player_name().to_owned(),
                dimension: self.dim.id().to_owned(),
                x: p.position.x,
                y: p.position.y,
                z: p.position.z,
                game_mode: mode(self.mode_id()),
                health: p.health.health(),
            };
            let mut players = self.server_players(None);
            players.retain(|p| p.id != player.id);
            players.push(player.clone());
            let snapshot = self.server_snapshot(players, None);
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world(None);
            let canceled = f(&mut self.mods.mods, &player);
            self.return_world();
            self.show_mod_output();
            canceled
        }
        pub(in crate::game) fn mod_consume_followups(&mut self) {
            if let Some(conn) = self.lan_acting {
                self.mods.consumed_uses.insert(conn, self.ticks);
                self.mods.restore_uses.insert(conn);
            }
        }

        /// The tick's end: joins and leaves, the tick's events, the tick hook.
        pub(in crate::game) fn tick_server_mods(&mut self) {
            if !self.options.execution_role.is_client() {
                // A server has no frames to check its dev mods in.
                self.reload_dev_mods();
            }
            self.send_server_mod_commands();
            self.announce_wasm_channels();
            if !self.mods.server {
                self.mods.interactions.clear();
                self.mods.item_uses.clear();
                self.mods.use_fallbacks.clear();
                self.mods.attacks.clear();
                self.mods.hurt_events.clear();
                self.mods.deaths.clear();
                self.mods.player_damage.clear();
                if let Some(p) = self.player.as_mut() {
                    p.mod_hurts.clear();
                }
                return;
            }
            if let Some(p) = self.player.as_mut() {
                let events = std::mem::take(&mut p.mod_hurts);
                let id = self.player_uuid_string();
                self.collect_mod_player_damage(&id, events);
            }
            self.mods.last_hurt_events = std::mem::take(&mut self.mods.hurt_events).into();
            self.mods.last_deaths = std::mem::take(&mut self.mods.deaths).into();
            self.mods.last_player_damage = std::mem::take(&mut self.mods.player_damage).into();
            self.mods.last_item_uses = std::mem::take(&mut self.mods.item_uses).into();
            self.mods.use_fallbacks.clear();
            self.mods.last_attacks = std::mem::take(&mut self.mods.attacks).into();
            let players = self.server_players(None);
            let mut snapshot = self.server_snapshot(players.clone(), None);
            snapshot.interactions = self.take_mod_interactions();
            snapshot.generated_chunks = std::mem::take(&mut self.mods.generated_chunks).into();
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world(None);
            self.lend_mod_inventories(None);
            let before = std::mem::take(&mut self.mods.players);
            for p in before
                .iter()
                .filter(|p| !players.iter().any(|q| q.id == p.id))
            {
                self.mods.mods.player_left(p);
            }
            for p in players
                .iter()
                .filter(|p| !before.iter().any(|q| q.id == p.id))
            {
                self.mods.mods.player_joined(p);
            }
            for event in std::mem::take(&mut self.mods.events) {
                let who = |w: &Who| {
                    let id = match w {
                        Who::Local => self.player_uuid_string(),
                        Who::Uuid(u) => uuid_string(*u),
                        Who::Conn(c) => {
                            let guests = self.lan.iter().flat_map(|h| &h.hub.guests);
                            let g = guests.into_iter().find(|g| g.conn == *c);
                            g.map_or_else(String::new, |g| uuid_string(g.uuid))
                        }
                    };
                    players.iter().chain(&before).find(|p| p.id == id).cloned()
                };
                match event {
                    ModEvent::Chat(w, text) => {
                        if let Some(p) = who(&w) {
                            self.mods.mods.player_chat(&p, &text);
                        }
                    }
                    ModEvent::Block {
                        who: w,
                        pos,
                        state,
                        placed,
                        exclude,
                    } => {
                        let Some(p) = who(&w) else { continue };
                        let pos = BlockPos {
                            x: pos[0],
                            y: pos[1],
                            z: pos[2],
                        };
                        if placed {
                            self.mods.mods.block_placed(&p, pos, &state);
                        } else {
                            self.mods
                                .mods
                                .block_broken_except(&p, pos, &state, exclude.as_deref());
                        }
                    }
                }
            }
            self.run_wasm_channels(&players);
            self.mods.mods.flush_behaviour_ticks();
            self.mods.mods.server_tick(self.ticks);
            self.return_world();
            self.return_mod_inventories(None);
            self.mods.players = players;
            self.show_mod_output();
            self.apply_mod_block_writes();
            self.apply_mod_orders();
        }

        /// Players' command trees list the running server mods' commands
        /// after vanilla's, whenever they change (or a hub opens).
        fn send_server_mod_commands(&mut self) {
            let key = self.mods.mods.generation();
            let Some(host) = self.lan.as_mut() else {
                return;
            };
            if host.hub.mod_commands_key() == Some(key) {
                return;
            }
            let tree = command_tree(self.mods.mods.server_commands());
            host.hub.set_mod_commands(key, tree);
        }

        /// Lends the levels' chunks to capability `world` for the calls up to
        /// [`Game::return_world`], when a running server mod has it.
        fn lend_world(&mut self, host: Option<&LanHost>) {
            if !self.mods.world {
                return;
            }
            let containers = self.mod_container_snapshot();
            let storage = if self.mods.mods.wants_storage() {
                self.mod_storage_snapshot(host)
            } else {
                Default::default()
            };
            let items = self
                .mods
                .item_limits
                .get_or_insert_with(|| {
                    std::sync::Arc::new(
                        self.res
                            .items()
                            .items
                            .iter()
                            .map(|(id, info)| (id.to_string(), info.max_stack))
                            .collect(),
                    )
                })
                .clone();
            let mut dims = vec![(self.dim.id(), std::mem::take(&mut self.world.chunks))];
            for (dim, chunks) in self.levels.parked_chunks() {
                dims.push((dim.id(), std::mem::take(chunks)));
            }
            let registry = self.pack.assets.registry;
            let biomes = self.res.biomes().clone();
            self.mods.mods.lend_world(Box::new(LentWorld {
                registry,
                block_tags: self
                    .mods
                    .block_tags
                    .get_or_insert_with(|| std::sync::Arc::new(self.res.items().block_tags.clone()))
                    .clone(),
                biomes,
                dims,
                containers,
                storage,
                items,
                resources: self.options.resources.clone(),
                world_root: self.world_root.clone(),
            }));
        }

        /// Puts back the chunks [`Game::lend_world`] lent.
        fn return_world(&mut self) {
            let Some(view) = self.mods.mods.take_world() else {
                return;
            };
            let view: Box<dyn std::any::Any> = view;
            let Ok(lent) = view.downcast::<LentWorld>() else {
                return;
            };
            let mut dims = lent.dims;
            if let Some(i) = dims.iter().position(|(d, _)| *d == self.dim.id()) {
                self.world.chunks = dims.swap_remove(i).1;
            }
            for (dim, chunks) in self.levels.parked_chunks() {
                if let Some(i) = dims.iter().position(|(d, _)| *d == dim.id()) {
                    *chunks = dims.swap_remove(i).1;
                }
            }
        }

        /// The snapshot server mods read: the game's, the players, and the loaded mobs when a
        /// running mod has capability `entities`.
        fn server_snapshot(
            &mut self,
            players: Vec<ServerPlayer>,
            host: Option<&LanHost>,
        ) -> Snapshot {
            let mut snapshot = self.mod_snapshot_with(players);
            snapshot.player_looks = self.player_looks(host).into();
            let host = host.or(self.lan.as_ref());
            snapshot.player_permissions = snapshot
                .players
                .iter()
                .map(|p| {
                    let level = host
                        .and_then(|h| {
                            h.hub
                                .guests
                                .iter()
                                .find(|g| g.placed && uuid_string(g.uuid) == p.id)
                                // Hub::refresh_permissions resolves ops and integrated-server
                                // command access, including the owner's level 4 with cheats.
                                .map(|g| g.permission)
                        })
                        .unwrap_or_else(|| if self.allow_commands() { 4 } else { 0 });
                    (p.id.clone(), level.min(4))
                })
                .collect::<Vec<_>>()
                .into();
            snapshot.teleport_borders = [
                "minecraft:overworld",
                "minecraft:the_nether",
                "minecraft:the_end",
            ]
            .into_iter()
            .map(|d| (d.to_owned(), self.border.bounds()))
            .collect();
            let [x, y, z] = self.level.world_spawn.unwrap_or([0, 64, 0]);
            snapshot.world_spawn = Some((
                "minecraft:overworld".into(),
                horizon_mods::WorldPoint {
                    x: f64::from(x),
                    y: f64::from(y),
                    z: f64::from(z),
                },
                self.level.world_spawn_angle[0],
            ));
            if self.mods.entities {
                let here = self.dim.id();
                // Who rides which mob: the server's players by their vehicle, and the
                // window's own player in-frame.
                let mut riders: std::collections::HashMap<u32, String> = (self.lan.iter())
                    .flat_map(|h| &h.hub.guests)
                    .filter_map(|g| Some((g.vehicle?, uuid_string(g.uuid))))
                    .collect();
                if self.options.execution_role.is_client()
                    && self.remote.is_none()
                    && let Some(id) = self.riding
                {
                    riders.insert(id, self.player_uuid_string());
                }
                let mut all: Vec<ModEntity> = (self.mobs.iter())
                    .filter(|m| m.alive() && !m.removed)
                    .map(|m| mod_entity(m, here, riders.get(&m.id)))
                    .collect();
                for (dim, mobs) in self.levels.all_parked_mobs() {
                    let alive = mobs.iter().filter(|m| m.alive() && !m.removed);
                    all.extend(alive.map(|m| mod_entity(m, dim.id(), riders.get(&m.id))));
                }
                snapshot.entities = all.into();
                snapshot.tamable_mobs = self
                    .mobs
                    .iter()
                    .chain(self.levels.all_parked_mobs().flat_map(|(_, ms)| ms.iter()))
                    .filter(|m| m.alive() && !m.removed && horizon_client::mobs::tame::can_tame(m))
                    .map(|m| m.id)
                    .collect::<Vec<_>>()
                    .into();
                let mut vehicles: Vec<(u32, u32)> = (self.mobs.iter())
                    .filter_map(|m| Some((m.id, m.extra.mount?)))
                    .collect();
                for (_, mobs) in self.levels.all_parked_mobs() {
                    vehicles.extend(mobs.iter().filter_map(|m| Some((m.id, m.extra.mount?))));
                }
                snapshot.vehicles = vehicles.into();
                let kinds = self.res.mob_types().keys();
                let kinds = kinds.filter(|k| !k.starts_with("minecraft:")).cloned();
                snapshot.mob_kinds = kinds.collect::<Vec<_>>().into();
            }
            if self.mods.entities || self.mods.world {
                let root = &self.options.resources;
                if self
                    .mods
                    .entity_resources
                    .as_ref()
                    .is_none_or(|(p, _)| p != root)
                {
                    let namespaces = horizon_client::mod_content::namespaces(root);
                    let textures = horizon_client::atlas_sources::list_textures(root)
                        .into_iter()
                        .filter(|t| {
                            t.split_once(':')
                                .is_some_and(|(ns, _)| namespaces.iter().any(|n| n == ns))
                        })
                        .filter(|t| horizon_mods::content::entity_texture(t))
                        .collect();
                    let builtin = horizon_protocol::registry::Builtin::find(root)
                        .and_then(|p| horizon_protocol::registry::Builtin::load(&p).ok())
                        .unwrap_or_default();
                    let mut sounds: std::collections::HashSet<String> =
                        self.audio.engine.registry.names().cloned().collect();
                    if let Some(ids) = builtin.get("minecraft:sound_event") {
                        sounds.extend(ids.names.iter().cloned());
                    }
                    // Reading the codec with just its id succeeds exactly for types with no options.
                    let particles = builtin
                        .get("minecraft:particle_type")
                        .into_iter()
                        .flat_map(|ids| &ids.ids)
                        .filter(|(_, id)| {
                            let mut w = horizon_protocol::buf::Writer::new();
                            w.varint(**id);
                            horizon_protocol::entity_data::Particle::read(
                                &mut horizon_protocol::buf::Reader::new(&w.data),
                            )
                            .is_ok()
                        })
                        .map(|(name, id)| (name.clone(), *id))
                        .collect();
                    let resources = horizon_mods::EntityResources {
                        textures,
                        sounds,
                        particles,
                    };
                    self.mods.entity_resources =
                        Some((root.clone(), std::sync::Arc::new(resources)));
                }
                snapshot.entity_resources = self.mods.entity_resources.as_ref().unwrap().1.clone();
            }
            snapshot
        }

        /// Carries out server mods' orders to the mobs of the level in place, in order;
        /// those to mobs of another level wait for it, and those to mobs gone are dropped.
        pub(in crate::game) fn apply_mod_orders(&mut self) {
            self.apply_mod_item_drops();
            self.apply_mod_spawns();
            self.apply_mod_entity_operations();
            let orders = std::mem::take(&mut self.mods.mob_orders);
            let parked: std::collections::HashSet<u32> = (self.levels.all_parked_mobs())
                .flat_map(|(_, mobs)| mobs.iter().map(|m| m.id))
                .collect();
            for (id, order) in orders {
                match self.mobs.iter().position(|m| m.id == id && !m.removed) {
                    Some(i) => self.apply_mod_order(i, order),
                    None if parked.contains(&id) => self.mods.mob_orders.push((id, order)),
                    None => {}
                }
            }
            self.apply_mod_seatings();
            self.apply_mod_rides();
        }

        fn apply_mod_entity_operations(&mut self) {
            let parked: std::collections::HashSet<u32> = self
                .levels
                .all_parked_mobs()
                .flat_map(|(_, mobs)| mobs.iter().map(|m| m.id))
                .collect();
            for op in std::mem::take(&mut self.mods.entity_operations) {
                let id = match &op {
                    Output::Strike { mob, .. }
                    | Output::Tame { mob, .. }
                    | Output::MobEffect { mob, .. }
                    | Output::MobPush { mob, .. } => *mob,
                    _ => continue,
                };
                if self
                    .mobs
                    .iter()
                    .any(|m| m.id == id && m.alive() && !m.removed)
                {
                    match op {
                        Output::Strike { mob, hit } => self.mod_strike(mob, &hit),
                        Output::MobEffect { mob, change } => {
                            if let Some(i) = self.mobs.iter().position(|m| m.id == mob) {
                                self.apply_mod_order(i, EntityOrder::Effect(change));
                            }
                        }
                        Output::MobPush { mob, velocity } => {
                            if let Some(i) = self.mobs.iter().position(|m| m.id == mob) {
                                self.apply_mod_order(i, EntityOrder::Push(velocity));
                            }
                        }
                        Output::Tame { mob, owner } => {
                            let owner = owner
                                .and_then(|s| u128::from_str_radix(&s.replace('-', ""), 16).ok())
                                .map(horizon_client::lan::playerdata::uuid_ints);
                            if let Some(m) = self.mobs.iter_mut().find(|m| m.id == mob)
                                && horizon_client::mobs::tame::can_tame(m)
                            {
                                horizon_client::mobs::tame::tame_with_owner(m, owner);
                            }
                        }
                        _ => unreachable!(),
                    }
                } else if parked.contains(&id) {
                    self.mods.entity_operations.push(op);
                }
            }
        }

        /// `entities.ride`: as `/ride` (`Entity.startRiding`/`stopRiding`), for mobs of
        /// the level in place; those of a parked level wait for it.
        fn apply_mod_rides(&mut self) {
            if self.mods.rides.is_empty() {
                return;
            }
            let parked: std::collections::HashSet<u32> = (self.levels.all_parked_mobs())
                .flat_map(|(_, mobs)| mobs.iter().map(|m| m.id))
                .collect();
            let live = |game: &Self, id: u32| {
                (game.mobs.iter()).position(|m| m.id == id && !m.removed && m.alive())
            };
            for (rider, vehicle) in std::mem::take(&mut self.mods.rides) {
                let Some(i) = live(self, rider) else {
                    if parked.contains(&rider) {
                        self.mods.rides.push((rider, vehicle));
                    }
                    continue;
                };
                let Some(v) = vehicle else {
                    self.dismount_mob(rider);
                    continue;
                };
                if self.mobs[i].extra.mount == Some(v) {
                    continue;
                }
                let Some(j) = live(self, v) else { continue };
                // A vehicle carries one: another mob, the window's player or a guest.
                let player_on = self.riding == Some(v)
                    || (self.lan.iter())
                        .flat_map(|h| &h.hub.guests)
                        .any(|g| g.vehicle == Some(v));
                if self.mobs[j].extra.rider.is_some_and(|r| r != rider) || player_on {
                    continue;
                }
                // No loop: the vehicle's vehicles must not include the rider.
                let mut at = self.mobs[j].extra.mount;
                let mut looped = false;
                for _ in 0..16 {
                    let Some(next) = at else { break };
                    if next == rider {
                        looped = true;
                        break;
                    }
                    at = self
                        .mobs
                        .iter()
                        .find(|m| m.id == next)
                        .and_then(|m| m.extra.mount);
                }
                if looped {
                    continue;
                }
                if self.mobs[i].extra.mount.is_some() {
                    self.dismount_mob(rider);
                }
                self.mount_mob(rider, v);
                self.position_riders();
            }
        }

        /// `mount` and `dismount`: a player gets on a content mod's mob (as a player
        /// mounts a horse: the game's `riding`, swapped in for a server's player) or off it.
        fn apply_mod_seatings(&mut self) {
            for (id, who) in std::mem::take(&mut self.mods.seatings) {
                let Some(i) = self.mobs.iter().position(|m| m.id == id && !m.removed) else {
                    continue;
                };
                if !self.mobs[i].alive() {
                    continue;
                }
                let at = self.mobs[i].position;
                let near = |p: [f64; 3]| {
                    (p[0] - at.x).powi(2) + (p[1] - at.y).powi(2) + (p[2] - at.z).powi(2)
                        <= 16.0 * 16.0
                };
                // The window's own player, in-frame.
                if self.options.execution_role.is_client()
                    && self.remote.is_none()
                    && let Some(p) = self.player.as_ref()
                {
                    let mine = who.as_deref() == Some(self.player_uuid_string().as_str());
                    if mine
                        && self.riding.is_none()
                        && near([p.position.x, p.position.y, p.position.z])
                    {
                        self.riding = Some(id);
                        self.mount.reset_jump();
                        self.position_rider();
                        continue;
                    }
                    if who.is_none() && self.riding == Some(id) {
                        self.dismount_mount();
                        continue;
                    }
                }
                let Some(mut host) = self.lan.take() else {
                    continue;
                };
                let here = self.dim.id().to_owned();
                match who {
                    Some(player) => {
                        let guest = host.hub.guests.iter().find(|g| {
                            g.placed
                                && g.health > 0.0
                                && g.vehicle.is_none()
                                && g.dimension == here
                                && uuid_string(g.uuid) == player
                                && near(g.position)
                        });
                        if let Some(conn) = guest.map(|g| g.conn) {
                            self.as_guest(&mut host, conn, |game| {
                                game.riding = Some(id);
                                game.mount.reset_jump();
                            });
                            self.guest_boarded(&mut host, conn, None);
                        }
                    }
                    None => {
                        let riders: Vec<u32> = (host.hub.guests.iter())
                            .filter(|g| g.vehicle == Some(id))
                            .map(|g| g.conn)
                            .collect();
                        for conn in riders {
                            self.as_guest(&mut host, conn, |game| game.dismount_mount());
                        }
                    }
                }
                self.lan = Some(host);
            }
        }

        /// Spawns server mods asked for in the level in place; those of a level that isn't
        /// in place wait for it, those of a dimension the server hasn't are dropped.
        fn apply_mod_spawns(&mut self) {
            if self.mods.mob_spawns.is_empty() {
                return;
            }
            let here = self.dim.id();
            let parked: Vec<&str> = self.levels.parked_dims().iter().map(|d| d.id()).collect();
            for s in std::mem::take(&mut self.mods.mob_spawns) {
                if s.dimension != here {
                    if parked.contains(&s.dimension.as_str()) {
                        self.mods.mob_spawns.push(s);
                    }
                    continue;
                }
                let Some(id) = self.spawn_mob(&s.kind, s.at, true) else {
                    log::debug!("Horizon: a server mod's {} did not spawn", s.kind);
                    continue;
                };
                if let Some(m) = self.mobs.iter_mut().find(|m| m.id == id) {
                    (m.yaw, m.body_yaw, m.head_yaw) = (s.yaw, s.yaw, s.yaw);
                    m.orders.data.extend(s.data);
                }
            }
        }

        fn apply_mod_order(&mut self, i: usize, order: EntityOrder) {
            let chosen_player = if let EntityOrder::Target(EntityTarget::Player(id)) = &order {
                if !self.server_players(None).iter().any(|p| p.id == *id) {
                    return;
                }
                let Ok(uuid) = u128::from_str_radix(&id.replace('-', ""), 16) else {
                    return;
                };
                Some(horizon_client::lan::playerdata::uuid_ints(uuid))
            } else {
                None
            };
            let m = &mut self.mobs[i];
            match order {
                EntityOrder::WalkTo(w) => m.orders.walk = Some(([w.to.x, w.to.y, w.to.z], w.speed)),
                EntityOrder::Halt => (m.orders.walk, m.orders.look) = (None, None),
                EntityOrder::LookAt(p) => m.orders.look = Some([p.x, p.y, p.z]),
                EntityOrder::Push(p) => {
                    m.velocity = Vec3::new(p.x, p.y, p.z);
                    m.needs_sync = true;
                }
                EntityOrder::Gravity(on) => m.no_gravity = !on,
                EntityOrder::Mount(seat) => {
                    m.orders.seat = Some((seat.height, seat.forward));
                    let id = m.id;
                    self.mods.seatings.push((id, Some(seat.player)));
                }
                EntityOrder::Dismount => {
                    let id = m.id;
                    self.mods.seatings.push((id, None));
                }
                EntityOrder::Texture(name) => m.orders.texture = name.map(std::sync::Arc::from),
                EntityOrder::Sound(sound) => {
                    let at = [m.position.x, m.position.y, m.position.z];
                    self.mod_play_sound(at, &sound);
                }
                EntityOrder::Particles(p) => self.mod_particles(&p),
                EntityOrder::Animate(name) => {
                    let age = m.age;
                    m.orders.animation = name.map(|n| (std::sync::Arc::from(n), age));
                }
                EntityOrder::HurtArea(area) => {
                    let id = m.id;
                    self.mod_hurt_area(id, &area);
                }
                // Entity.discard: gone, no death, no drops.
                EntityOrder::Discard => m.removed = true,
                EntityOrder::Target(t) => {
                    m.orders.target_player = chosen_player;
                    m.ai.target = match t {
                        EntityTarget::None => None,
                        EntityTarget::Mob(id) => Some(MobWho::Mob(id)),
                        EntityTarget::NearestPlayer | EntityTarget::Player(_) => {
                            Some(MobWho::Player)
                        }
                    };
                }
                EntityOrder::Attribute(a) => {
                    horizon_client::mobs::orders::attribute(m, &a.name, a.value)
                }
                EntityOrder::Heal(amount) => m.heal(amount),
                EntityOrder::Effect(effect) => {
                    if let Some(id) = super::super::potions::mob_effect_id(&effect.id) {
                        if effect.seconds == 0 {
                            m.extra.effects.retain(|e| e.0 != id);
                            m.extra.hidden_effects.retain(|e| *e != id);
                        } else {
                            let ticks =
                                effect.seconds.saturating_mul(20).min(i32::MAX as u32) as i32;
                            m.add_effect(id, ticks, i32::from(effect.amplifier));
                        }
                        m.needs_sync = true;
                    }
                }
                EntityOrder::Scale(s) => {
                    use horizon_client::mobs::finalize::{Attr, set_base};
                    set_base(m, Attr::Scale, f64::from(s));
                }
                EntityOrder::SetData(entries) => {
                    for (key, value) in entries {
                        match value {
                            Some(v) => {
                                let full = m.orders.data.len() >= DATA_ENTRIES;
                                if !full || m.orders.data.contains_key(&key) {
                                    m.orders.data.insert(key, v);
                                }
                            }
                            None => {
                                m.orders.data.remove(&key);
                            }
                        }
                    }
                }
            }
        }

        fn mod_play_sound(&mut self, at: [f64; 3], sound: &horizon_mods::EntitySound) {
            if let Some(host) = &self.lan {
                let source = horizon_client::settings::sound_source(&sound.name) as i32;
                host.hub
                    .sound(&sound.name, at, sound.volume, sound.pitch, source, None);
            }
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.as_ref().is_some_and(|p| {
                    let d = [
                        p.position.x - at[0],
                        p.position.y - at[1],
                        p.position.z - at[2],
                    ];
                    d.iter().map(|d| d * d).sum::<f64>()
                        <= (f64::from(sound.volume.max(1.0)) * 16.0).powi(2)
                })
            {
                self.audio
                    .play_unshared(&sound.name, Some(at), sound.volume, sound.pitch);
            }
        }

        fn mod_particles(&mut self, p: &horizon_mods::EntityParticles) {
            use horizon_client::particles::{ParticleOptions, ParticleType};
            let Some((_, resources)) = &self.mods.entity_resources else {
                return;
            };
            let Some(&id) = resources.particles.get(&p.name) else {
                return;
            };
            let at = [p.at.x, p.at.y, p.at.z];
            let delta = [p.delta.x, p.delta.y, p.delta.z];
            self.lan_particle_packet(&play::LevelParticles {
                particle: horizon_protocol::entity_data::Particle { id, data: None },
                override_limiter: false,
                always_show: false,
                pos: at,
                dist: delta.map(|d| d as f32),
                max_speed: [p.speed; 3],
                count: p.count as i32,
                randomization: 0,
            });
            if self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.as_ref().is_some_and(|p| {
                    let pos = [p.position.x, p.position.y, p.position.z];
                    (0..3)
                        .map(|k| (pos[k].floor() + 0.5 - at[k]).powi(2))
                        .sum::<f64>()
                        < 32.0 * 32.0
                })
                && let Some(ty) = ParticleType::from_name(&p.name)
            {
                self.particles.send_particles(
                    &ParticleOptions::simple(ty),
                    at,
                    delta,
                    f64::from(p.speed),
                    p.count,
                );
            }
        }

        /// `hurt-area`: every living thing in the ball but the mob takes the amount as the
        /// mob's attack (`mob_attack`), then burns: mobs of the level, the player and the
        /// level's other players.
        fn mod_hurt_area(&mut self, source: u32, area: &EntityArea) {
            self.mod_strike_from(
                source,
                &horizon_mods::StrikeHit {
                    center: area.center,
                    radius: area.radius,
                    amount: area.amount,
                    fire: area.fire,
                    skip_kinds: vec![],
                    knockback: 0.0,
                    credit_player: None,
                },
                [area.center.x, area.center.y, area.center.z],
            );
        }

        fn mod_strike(&mut self, source: u32, area: &horizon_mods::StrikeHit) {
            let Some(source_pos) = self
                .mobs
                .iter()
                .find(|m| m.id == source)
                .map(|m| [m.position.x, m.position.y, m.position.z])
            else {
                return;
            };
            self.mod_strike_from(source, area, source_pos);
        }
        fn mod_strike_from(
            &mut self,
            source: u32,
            area: &horizon_mods::StrikeHit,
            source_pos: [f64; 3],
        ) {
            // A queued player credit is never silently changed into a mob attack.
            let credit = area.credit_player.as_deref();
            if credit.is_some_and(|id| !self.server_players(None).iter().any(|p| p.id == id)) {
                return;
            }
            let killer = credit.and_then(|id| {
                self.lan
                    .as_ref()?
                    .hub
                    .guests
                    .iter()
                    .find(|g| uuid_string(g.uuid) == id)
                    .map(|g| g.conn)
            });
            let center = [area.center.x, area.center.y, area.center.z];
            let reaches = |feet, half, height| {
                horizon_client::mobs::orders::area_reaches(center, area.radius, feet, half, height)
            };
            let (amount, fire) = (area.amount, area.fire as i32 * 20);
            let attack = Damage {
                kind: if credit.is_some() {
                    DamageKind::PlayerAttack
                } else {
                    DamageKind::MobAttack
                },
                attacker: Some(if credit.is_some() {
                    MobWho::Player
                } else {
                    MobWho::Mob(source)
                }),
                attacker_species: self.mobs.iter().find(|m| m.id == source).map(Mob::species),
                from: Some(source_pos),
            };

            let mut mobs = std::mem::take(&mut self.mobs);
            for m in mobs.iter_mut().filter(|m| {
                m.alive() && !m.removed && m.id != source && !area.skip_kinds.contains(&m.kind.id)
            }) {
                let (w, h, _) = m.dims();
                if !reaches([m.position.x, m.position.y, m.position.z], w / 2.0, h) {
                    continue;
                }
                if amount > 0.0 {
                    let landed = m.damage(amount, attack);
                    if landed && credit.is_some() {
                        self.last_hurt_by_player.insert(m.id, killer);
                        if let Some(player) = credit {
                            m.credit_mod_hurts(player);
                        }
                    }
                }
                if area.knockback > 0.0 {
                    m.knockback(
                        f64::from(area.knockback),
                        source_pos[0] - m.position.x,
                        source_pos[2] - m.position.z,
                        &attack,
                    );
                    m.needs_sync = true;
                }
                if fire > 0 {
                    m.ignite_for_ticks(fire);
                }
            }
            self.mobs = mobs;
            let hurt = |g: &mut Self| {
                let id = g
                    .lan_acting_uuid
                    .map_or_else(|| g.player_uuid_string(), uuid_string);
                if credit == Some(id.as_str())
                    || area.skip_kinds.iter().any(|k| k == "minecraft:player")
                {
                    return;
                }
                let Some(p) = g.player.as_ref().map(|p| p.position) else {
                    return;
                };
                if !reaches([p.x, p.y, p.z], 0.3, 1.8) {
                    return;
                }
                if amount > 0.0 {
                    g.hurt_player_from(
                        amount,
                        Some(source_pos),
                        if credit.is_some() {
                            "minecraft:player_attack"
                        } else {
                            "minecraft:mob_attack"
                        },
                        Some(if credit.is_some() { None } else { Some(source) }),
                    );
                }
                if area.knockback > 0.0 {
                    g.knock_back_player(
                        f64::from(area.knockback),
                        source_pos[0] - p.x,
                        source_pos[2] - p.z,
                        horizon_client::health::HurtSource {
                            damage_type: "minecraft:mob_attack",
                            causing: source as i32,
                            direct: source as i32,
                        },
                    );
                    if let Some(p) = g.player.as_mut() {
                        p.health.sync_velocity = true;
                    }
                }
                if fire > 0
                    && let Some(p) = g.player.as_mut()
                    && let Err(e) = p.ignite_for_ticks(fire)
                {
                    log::debug!("Horizon: a server mod's fire: {e}");
                }
            };
            if self.options.execution_role.is_client() && self.remote.is_none() {
                hurt(self);
            }
            self.each_guest_hit(hurt);
        }

        /// Makes the blocks server mods set in the level in place, in order,
        /// as `/setblock` does. Those of other dimensions wait for their level.
        pub(in crate::game) fn apply_mod_block_writes(&mut self) {
            self.apply_mod_block_changes();
            self.apply_mod_loot_tables();
            self.apply_mod_world_operations(None);
        }

        /// Apply containers and templates only once their dimension is in place.
        fn apply_mod_world_operations(&mut self, mut host: Option<&mut LanHost>) {
            if self.mods.world_operations.is_empty() {
                return;
            }
            let dim = self.dim.id();
            let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.mods.world_operations)
                .into_iter()
                .partition(|op| match op {
                    Output::GiveItemWithData { .. } => true,
                    Output::StorageDataWrite(plan) => match &plan.location {
                        horizon_mods::StorageLocation::Container { dimension, .. } => {
                            dimension == dim
                        }
                        _ => true,
                    },
                    Output::StorageMove(plan) => {
                        [&plan.from, &plan.to].iter().all(|loc| match loc {
                            horizon_mods::StorageLocation::Container { dimension, .. } => {
                                dimension == dim
                            }
                            _ => true,
                        })
                    }
                    Output::SetContainer { dimension, .. }
                    | Output::PlaceStructure { dimension, .. }
                    | Output::SetSpawner { dimension, .. }
                    | Output::PlaySound { dimension, .. }
                    | Output::SpawnItemWithData { dimension, .. }
                    | Output::SpawnItem { dimension, .. }
                    | Output::BreakBlocksAs { dimension, .. } => dimension == dim,
                    _ => false,
                });
            self.mods.world_operations = later;
            for op in now {
                let result = match op {
                    Output::BreakBlocksAs {
                        mod_id,
                        player,
                        blocks,
                        options,
                        ..
                    } => {
                        self.apply_mod_breaks(
                            host.as_deref_mut(),
                            &mod_id,
                            &player,
                            blocks,
                            options,
                        );
                        Ok(())
                    }
                    Output::SpawnItem {
                        at, item, count, ..
                    } => {
                        self.spawn_item(
                            horizon_client::items::ItemStack::new(&item, count),
                            [at.x, at.y, at.z],
                            [0.0, 0.1, 0.0],
                            10,
                        );
                        Ok(())
                    }
                    Output::GiveItemWithData {
                        player,
                        item,
                        count,
                        mod_id,
                        data,
                    } => self.apply_mod_give_data(
                        &player,
                        &item,
                        count,
                        &mod_id,
                        data.as_deref(),
                        host.as_deref_mut(),
                    ),
                    Output::SpawnItemWithData {
                        at,
                        item,
                        count,
                        mod_id,
                        data,
                        ..
                    } => {
                        let mut stack = horizon_client::items::ItemStack::new(&item, count);
                        match stack.set_mod_data(&mod_id, data.as_deref()) {
                            Ok(()) => {
                                self.spawn_item(stack, [at.x, at.y, at.z], [0.0, 0.1, 0.0], 10);
                                Ok(())
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Output::StorageDataWrite(plan) => {
                        self.apply_mod_stack_data(plan, host.as_deref_mut())
                    }
                    Output::PlaySound { at, sound, .. } => {
                        self.mod_play_sound([at.x, at.y, at.z], &sound);
                        Ok(())
                    }
                    Output::StorageMove(plan) => {
                        self.apply_mod_storage_move(plan, host.as_deref_mut())
                    }
                    Output::SetContainer { pos, slots, .. } => self.apply_mod_container(pos, slots),
                    Output::PlaceStructure {
                        id,
                        pos,
                        rotation,
                        mirror,
                        ..
                    } => self.place_mod_template(&id, [pos.0, pos.1, pos.2], rotation, mirror),
                    Output::SetSpawner { pos, entity, .. } => {
                        self.apply_mod_spawner([pos.0, pos.1, pos.2], &entity)
                    }
                    _ => unreachable!(),
                };
                if let Err(error) = result {
                    log::warn!("Horizon: server mod world operation: {error}");
                }
            }
            if let Some(host) = host.or(self.lan.as_mut()) {
                host.hub.sync_containers(&self.world);
            }
        }

        /// `world.set-loot-table`, after the tick's blocks: the container rolls the table as a
        /// generated or `/setblock {LootTable:...}` one does (`take_loot_table`).
        fn apply_mod_loot_tables(&mut self) {
            if self.mods.loot_writes.is_empty() {
                return;
            }
            let dim = self.dim.id().to_owned();
            let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.mods.loot_writes)
                .into_iter()
                .partition(|(d, _, _)| *d == dim);
            self.mods.loot_writes = later;
            for (_, key, table) in now {
                let Some(mut entity) = self.world.block_entities.remove(&key) else {
                    continue;
                };
                if let horizon_client::containers::BlockEntity::Chest(slots) = &mut entity {
                    slots.iter_mut().for_each(|s| *s = None);
                }
                let tag = horizon_world::nbt::Tag::compound([(
                    "LootTable",
                    horizon_world::nbt::Tag::String(table.as_str().into()),
                )]);
                self.take_loot_table(key, &tag, &mut entity);
                self.world.block_entities.insert(key, entity);
                self.world.modified.insert((key.0 >> 4, key.2 >> 4));
            }
        }

        fn apply_mod_block_changes(&mut self) {
            if self.mods.block_writes.is_empty() {
                return;
            }
            let dim = self.dim.id();
            let (now, later): (Vec<_>, Vec<_>) = std::mem::take(&mut self.mods.block_writes)
                .into_iter()
                .partition(|(d, _)| d == dim);
            self.mods.block_writes = later;
            let registry = self.pack.assets.registry;
            for (_, b) in now {
                let state = match crate::game::render_control::parse_state(registry, &b.state) {
                    Ok(state) => state,
                    Err(e) => {
                        log::warn!("Horizon: a server mod set {}: {e}", b.state);
                        continue;
                    }
                };
                if let Err(e) = self.place_state([b.x, b.y, b.z], state, false) {
                    log::debug!(
                        "Horizon: a server mod's block at {} {} {}: {e:?}",
                        b.x,
                        b.y,
                        b.z
                    );
                }
            }
        }

        /// A player's right click on a mob: a content mod's, for capability `entities`.
        pub(in crate::game) fn mod_mob_interacted(&mut self, index: usize, item: Option<String>) {
            let m = &self.mobs[index];
            if !self.hears_players()
                || !self.mods.entities
                || m.kind.id.starts_with("minecraft:")
                || self.mods.interactions.len() >= MAX_INTERACTIONS
            {
                return;
            }
            let player = self
                .lan_acting_uuid
                .map_or_else(|| self.player_uuid_string(), uuid_string);
            self.mods
                .interactions
                .push(horizon_mods::EntityInteraction {
                    player,
                    mob: m.id,
                    kind: m.kind.id.clone(),
                    item,
                    sneaking: self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft),
                    hand: match self.hand {
                        horizon_client::first_person::Hand::Main => "main_hand",
                        horizon_client::first_person::Hand::Off => "off_hand",
                    }
                    .into(),
                });
        }

        fn take_mod_interactions(&mut self) -> std::sync::Arc<[horizon_mods::EntityInteraction]> {
            std::mem::take(&mut self.mods.interactions).into()
        }

        /// A player broke (or placed) `state` at `pos`: the player acting now.
        pub(in crate::game) fn mod_block_event(
            &mut self,
            pos: [i32; 3],
            state: BlockStateId,
            placed: bool,
        ) {
            if !self.hears_players() {
                return;
            }
            let who = self.lan_acting.map_or(Who::Local, Who::Conn);
            let state = self.state_string(state);
            self.mods.events.push(ModEvent::Block {
                who,
                pos,
                state,
                placed,
                exclude: self.mods.break_caller.clone(),
            });
        }

        /// A chat message a player sent.
        pub(in crate::game) fn mod_player_chat(&mut self, who: Who, text: &str) {
            if self.hears_players() {
                self.mods.events.push(ModEvent::Chat(who, text.to_owned()));
            }
        }

        /// `/line` typed by the window's own player in-frame: true when a
        /// server mod owns it.
        pub(in crate::game) fn local_server_mod_command(&mut self, line: &str) -> bool {
            self.local_server_mod_command_mode(line, false)
        }
        pub(in crate::game) fn local_server_mod_command_quiet(&mut self, line: &str) -> bool {
            self.local_server_mod_command_mode(line, true)
        }
        fn local_server_mod_command_mode(&mut self, line: &str, quiet: bool) -> bool {
            if quiet && !self.mod_quiet_registered(line) {
                return false;
            }
            if !self.hears_players() {
                return false;
            }
            let players = self.server_players(None);
            let Some(me) = players.first().cloned() else {
                return false;
            };
            let snapshot = self.server_snapshot(players, None);
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world(None);
            self.lend_mod_inventories(None);
            let owned = if quiet {
                self.mods.mods.server_command_quiet(&me, line)
            } else {
                self.mods.mods.server_command(&me, line)
            };
            self.return_world();
            self.return_mod_inventories(None);
            self.show_mod_output();
            self.apply_mod_block_writes();
            self.apply_mod_orders();
            owned
        }

        /// `/line` typed by a connection's player: true when a server mod
        /// owns it, whatever the player's permission level.
        pub(in crate::game) fn guest_server_mod_command(
            &mut self,
            host: &mut LanHost,
            conn: u32,
            line: &str,
        ) -> bool {
            self.guest_server_mod_command_mode(host, conn, line, false)
        }
        pub(in crate::game) fn guest_server_mod_command_quiet(
            &mut self,
            host: &mut LanHost,
            conn: u32,
            line: &str,
        ) -> bool {
            self.guest_server_mod_command_mode(host, conn, line, true)
        }
        fn mod_quiet_registered(&self, line: &str) -> bool {
            let name = line.split(' ').next().unwrap_or_default();
            !super::super::mods::vanilla_command(name)
                && self.mods.mods.server_commands().any(|c| c.name == name)
        }
        fn guest_server_mod_command_mode(
            &mut self,
            host: &mut LanHost,
            conn: u32,
            line: &str,
            quiet: bool,
        ) -> bool {
            if quiet && !self.mod_quiet_registered(line) {
                return false;
            }
            if !self.hears_players() {
                return false;
            }
            let Some(g) = host.hub.guest(conn) else {
                return false;
            };
            let id = uuid_string(g.uuid);
            if quiet {
                let mut canceled = false;
                self.native_player_command(g.uuid, &g.name, line, &mut canceled, &mut Vec::new());
                if canceled {
                    return false;
                }
            }
            let players = self.server_players(Some(host));
            let Some(me) = players.iter().find(|p| p.id == id).cloned() else {
                return false;
            };
            let mut snapshot = self.server_snapshot(players, Some(host));
            snapshot.channel_peers = self.wasm_channel_peers(Some(host));
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world(Some(host));
            self.lend_mod_inventories(Some(host));
            let owned = if quiet {
                self.mods.mods.server_command_quiet(&me, line)
            } else {
                self.mods.mods.server_command(&me, line)
            };
            self.return_world();
            self.return_mod_inventories(Some(host));
            if !owned {
                return false;
            }
            self.route_mod_output(Some(host));
            self.apply_mod_block_changes();
            self.apply_mod_loot_tables();
            self.apply_mod_world_operations(Some(host));
            host.hub.sync_containers(&self.world);
            // Orders wait for the level's next tick: an area's hurt reaches the players through
            // the hub, which is out of `self.lan` until this returns.
            true
        }

        /// Sends a server mod's line to the player with this id: the
        /// window's own chat for the local player, a system message for a
        /// connection's. `host` is the hub when it is out of `self.lan`.
        pub(in crate::game) fn tell_server_player(
            &mut self,
            host: Option<&mut LanHost>,
            id: &str,
            line: Text,
        ) {
            let local = self.options.execution_role.is_client()
                && self.remote.is_none()
                && self.player.is_some();
            if local && self.player_uuid_string() == id {
                self.add_mod_chat_line(line);
                return;
            }
            let Some(host) = host.or(self.lan.as_mut()) else {
                return;
            };
            let to = host
                .hub
                .guests
                .iter()
                .find(|g| g.placed && uuid_string(g.uuid) == id);
            if let Some(conn) = to.map(|g| g.conn) {
                host.hub.send_to(
                    conn,
                    &C::SystemChat(play::SystemChat {
                        content: component_nbt(&line.component()),
                        overlay: false,
                    }),
                );
            }
        }

        /// Routes a server mod's output: `Tell` and `Reply` to their player,
        /// `Broadcast` to everyone and the log.
        pub(in crate::game) fn route_server_output(
            &mut self,
            mut host: Option<&mut LanHost>,
            output: Output,
        ) {
            let named = |name: &str, text: String| {
                let mut line = Text::literal(format!("[{name}] ")).with_color(text::GRAY);
                line.push(Text::literal(text));
                line
            };
            match output {
                Output::Tell { name, player, text } => {
                    self.tell_server_player(host, &player, named(&name, text));
                }
                Output::Reply {
                    player,
                    text,
                    error,
                } => {
                    let line = Text::literal(text);
                    let line = if error {
                        line.with_color(text::RED)
                    } else {
                        line
                    };
                    self.tell_server_player(host, &player, line);
                }
                Output::Broadcast { name, text } => {
                    let plain = format!("[{name}] {text}");
                    if self.options.execution_role
                        == super::super::execution_role::ExecutionRole::DedicatedServer
                    {
                        super::super::dedicated::log("INFO", &plain);
                    } else {
                        log::info!("{plain}");
                    }
                    for p in self.server_players(host.as_deref()) {
                        let line = named(&name, text.clone());
                        self.tell_server_player(host.as_deref_mut(), &p.id, line);
                    }
                }
                Output::PlayerHeal { player, amount } => {
                    self.with_mod_player(host, &player, |g| {
                        g.apply_effect_action(horizon_client::effects::EffectAction::Heal(amount))
                    });
                }
                Output::DamageItem {
                    player,
                    hand,
                    amount,
                } => {
                    self.apply_mod_damage_item(host, &player, &hand, amount);
                }
                Output::PlayerKnockback {
                    player,
                    strength,
                    dx,
                    dz,
                } => {
                    self.with_mod_player(host, &player, |g| {
                        g.knock_back_player(
                            f64::from(strength),
                            -dx,
                            -dz,
                            horizon_client::health::HurtSource {
                                damage_type: "minecraft:generic",
                                causing: 0,
                                direct: 0,
                            },
                        );
                        if let Some(p) = g.player.as_mut() {
                            p.health.sync_velocity = true;
                        }
                    });
                }
                Output::PlayerTeleport { player, target } => {
                    self.with_mod_player(host, &player, |g| {
                        // Use TeleportCommand itself: it owns same-level placement and
                        // the cross-level ServerPlayer transfer queue. An explicit decimal
                        // prevents Vec3Argument from centering integer x/z coordinates.
                        let number = |v: f64| {
                            let mut text = v.to_string();
                            if !text.contains('.') {
                                text.push_str(".0");
                            }
                            text
                        };
                        let at = &target.at;
                        let mut line =
                            format!("tp @s {} {} {}", number(at.x), number(at.y), number(at.z));
                        if target.yaw.is_some() || target.pitch.is_some() {
                            let angle = |v: Option<f32>| {
                                v.map_or_else(|| "~".into(), |v| number(f64::from(v)))
                            };
                            line.push_str(&format!(
                                " {} {}",
                                angle(target.yaw),
                                angle(target.pitch)
                            ));
                        }
                        let source = super::super::commands::execute::Source {
                            silent: true,
                            rot: [g.yaw, g.pitch],
                            level: Some(super::super::Dim::from_id(&target.dimension)),
                            ..g.player_source()
                        };
                        // Capability players authorizes this operation independently of
                        // the destination player's command permissions. Restore the caller's
                        // command state, including when called inside a mod command hook.
                        let permission = g.ui.registries.permission;
                        g.ui.registries.permission = 4;
                        let exec = std::mem::take(&mut g.exec);
                        let result = g.execute_line(&line, source);
                        g.exec = exec;
                        g.ui.registries.permission = permission;
                        if let Err(error) = result {
                            log::warn!("server mod teleport: {error:?}");
                        }
                    });
                }
                Output::PlayerPush { player, velocity } => {
                    self.with_mod_player(host, &player, |g| {
                        if let Some(p) = g.player.as_mut() {
                            p.velocity = Vec3::new(
                                p.velocity.x + velocity.x,
                                p.velocity.y + velocity.y,
                                p.velocity.z + velocity.z,
                            );
                            p.health.sync_velocity = true;
                        }
                    });
                }
                op @ (Output::Strike { .. }
                | Output::Tame { .. }
                | Output::MobEffect { .. }
                | Output::MobPush { .. }) => {
                    if self.mods.entity_operations.len() < MAX_QUEUED_ORDERS {
                        self.mods.entity_operations.push(op);
                    }
                }
                Output::PlayerEffect { player, effect } => {
                    self.apply_mod_player_effect(host, &player, effect);
                }
                operation @ (Output::GiveItemWithData { .. }
                | Output::StorageDataWrite(_)
                | Output::SpawnItemWithData { .. }
                | Output::StorageMove(_)
                | Output::BreakBlocksAs { .. }
                | Output::SetContainer { .. }
                | Output::PlaceStructure { .. }
                | Output::SetSpawner { .. }
                | Output::PlaySound { .. }
                | Output::SpawnItem { .. }) => {
                    if self.mods.world_operations.len() < MAX_QUEUED_WRITES {
                        self.mods.world_operations.push(operation);
                    }
                }
                Output::SetLootTable {
                    dimension,
                    pos,
                    table,
                } => {
                    if self.mods.loot_writes.len() < MAX_QUEUED_WRITES {
                        self.mods.loot_writes.push((dimension, pos, table));
                    }
                }
                Output::SetBlocks {
                    name,
                    dimension,
                    blocks,
                } => {
                    let queued = &mut self.mods.block_writes;
                    queued.extend(blocks.into_iter().map(|b| (dimension.clone(), b)));
                    if queued.len() > MAX_QUEUED_WRITES {
                        let over = queued.len() - MAX_QUEUED_WRITES;
                        queued.drain(..over);
                        log::warn!(
                            "Horizon: mod {name} set more blocks than wait for their level: {over} dropped"
                        );
                    }
                }
                Output::Spawn {
                    name,
                    kind,
                    dimension,
                    at,
                    yaw,
                    data,
                } => {
                    if self.mods.mob_spawns.len() >= MAX_QUEUED_SPAWNS {
                        log::warn!("Horizon: mod {name} spawned more than wait for their level");
                        return;
                    }
                    let spawn = ModSpawn {
                        dimension,
                        kind,
                        at,
                        yaw,
                        data,
                    };
                    self.mods.mob_spawns.push(spawn);
                }
                Output::Ride { rider, vehicle } => {
                    if self.mods.rides.len() < MAX_QUEUED_ORDERS {
                        self.mods.rides.push((rider, vehicle));
                    }
                }
                Output::Orders { name, orders } => {
                    let queued = &mut self.mods.mob_orders;
                    queued.extend(orders);
                    if queued.len() > MAX_QUEUED_ORDERS {
                        let over = queued.len() - MAX_QUEUED_ORDERS;
                        queued.drain(..over);
                        log::warn!(
                            "Horizon: mod {name} gave more orders than wait for their level: {over} dropped"
                        );
                    }
                }
                _ => {}
            }
        }
    }
}

#[cfg(feature = "mods")]
impl super::GameLevel<'_> {
    pub(in crate::game) fn observe_wasm_tick(
        &self,
        family: horizon_mods::BehaviourFamily,
        dimension: horizon_world::level::Dimension,
        pos: horizon_core::position::BlockPos,
        state: horizon_world::registry::BlockStateId,
        from: &str,
    ) {
        let Some(s) = self.registry.state(state) else {
            return;
        };
        if self.behaviour_mods.wants_behaviour(family, &s.name) {
            self.behaviour_mods.observe_behaviour_block(
                family,
                self.behaviour_tick,
                run::dimension_id(dimension),
                horizon_mods::BlockPos {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                },
                &run::state_string(self.registry, state),
                from,
            );
        }
    }
}
