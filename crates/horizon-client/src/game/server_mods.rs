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
    },
}

#[cfg(not(feature = "mods"))]
impl Game<'_> {
    pub(super) fn tick_server_mods(&mut self) {}
    pub(super) fn mod_block_event(&mut self, _pos: [i32; 3], _state: BlockStateId, _placed: bool) {}
    pub(super) fn mod_player_chat(&mut self, _who: Who, _text: &str) {}
    pub(super) fn local_server_mod_command(&mut self, _line: &str) -> bool {
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
}

#[cfg(feature = "mods")]
mod run {
    use super::{Game, LanHost, ModEvent, Who};
    use horizon_client::chunk_map::ChunkMap;
    use horizon_client::mobs::{Damage, DamageKind, Mob, Who as MobWho};
    use horizon_client::{
        command::args::GameMode as Mode,
        lan::data::component_nbt,
        text::{self, Text},
    };
    use horizon_core::geometry::Vec3;
    use horizon_mods::{
        ArgumentKind, BlockPos, EntityArea, EntityOrder, GameMode, ModEntity, Output,
        ServerCommand, ServerPlayer, Snapshot, WorldView,
    };
    use horizon_protocol::packets::play::{self, Clientbound as C};
    use horizon_world::registry::{BlockRegistry, BlockStateId};
    use serde_json::{Value, json};

    /// Blocks queued for levels that don't tick (another dimension, in-frame)
    /// are kept up to this many; the oldest go first.
    const MAX_QUEUED_WRITES: usize = 1 << 16;
    /// Orders to mobs in levels that don't tick are kept up to this many, as blocks are.
    const MAX_QUEUED_ORDERS: usize = 1 << 14;

    /// A mob as capability `entities` reads it.
    fn mod_entity(m: &Mob, dimension: &str) -> ModEntity {
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
            age: m.age,
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
        biomes: std::sync::Arc<horizon_client::biome::Biomes>,
        dims: Vec<(&'static str, ChunkMap)>,
    }
    impl WorldView for LentWorld {
        fn block(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            Some(state_string(self.registry, chunk.block(x & 15, y, z & 15)))
        }
        fn biome(&self, dimension: &str, x: i32, y: i32, z: i32) -> Option<String> {
            let (_, chunks) = self.dims.iter().find(|(d, _)| *d == dimension)?;
            let chunk = chunks.get(&(x >> 4, z >> 4))?;
            Some(self.biomes.get(chunk.biome(x, y, z)).name.clone())
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
    fn state_string(registry: &BlockRegistry, state: BlockStateId) -> String {
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
    fn uuid_string(u: u128) -> String {
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
        fn hears_players(&self) -> bool {
            self.mods.server && self.remote.is_none()
        }

        /// The tick's end: joins and leaves, the tick's events, the tick hook.
        pub(in crate::game) fn tick_server_mods(&mut self) {
            if !self.options.execution_role.is_client() {
                // A server has no frames to check its dev mods in.
                self.reload_dev_mods();
            }
            self.send_server_mod_commands();
            if !self.mods.server {
                return;
            }
            let players = self.server_players(None);
            let snapshot = self.server_snapshot(players.clone());
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world();
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
                            self.mods.mods.block_broken(&p, pos, &state);
                        }
                    }
                }
            }
            self.mods.mods.server_tick(self.ticks);
            self.return_world();
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
        fn lend_world(&mut self) {
            if !self.mods.world {
                return;
            }
            let mut dims = vec![(self.dim.id(), std::mem::take(&mut self.world.chunks))];
            for (dim, chunks) in self.levels.parked_chunks() {
                dims.push((dim.id(), std::mem::take(chunks)));
            }
            let registry = self.pack.assets.registry;
            let biomes = self.res.biomes().clone();
            self.mods.mods.lend_world(Box::new(LentWorld {
                registry,
                biomes,
                dims,
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
        fn server_snapshot(&mut self, players: Vec<ServerPlayer>) -> Snapshot {
            let mut snapshot = self.mod_snapshot_with(players);
            if self.mods.entities {
                let here = self.dim.id();
                let mut all: Vec<ModEntity> = (self.mobs.iter())
                    .filter(|m| m.alive() && !m.removed)
                    .map(|m| mod_entity(m, here))
                    .collect();
                for (dim, mobs) in self.levels.parked_mobs() {
                    let alive = mobs.iter().filter(|m| m.alive() && !m.removed);
                    all.extend(alive.map(|m| mod_entity(m, dim.id())));
                }
                snapshot.entities = all.into();
            }
            snapshot
        }

        /// Carries out server mods' orders to the mobs of the level in place, in order;
        /// those to mobs of another level wait for it, and those to mobs gone are dropped.
        pub(in crate::game) fn apply_mod_orders(&mut self) {
            if self.mods.mob_orders.is_empty() {
                return;
            }
            let orders = std::mem::take(&mut self.mods.mob_orders);
            let parked: std::collections::HashSet<u32> = (self.levels.parked_mobs())
                .flat_map(|(_, mobs)| mobs.iter().map(|m| m.id))
                .collect();
            for (id, order) in orders {
                match self.mobs.iter().position(|m| m.id == id && !m.removed) {
                    Some(i) => self.apply_mod_order(i, order),
                    None if parked.contains(&id) => self.mods.mob_orders.push((id, order)),
                    None => {}
                }
            }
        }

        fn apply_mod_order(&mut self, i: usize, order: EntityOrder) {
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
                EntityOrder::Animate(name) => {
                    let age = m.age as f32;
                    m.orders.animation = name.map(|n| (std::sync::Arc::from(n), age));
                }
                EntityOrder::HurtArea(area) => {
                    let id = m.id;
                    self.mod_hurt_area(id, &area);
                }
                // Entity.discard: gone, no death, no drops.
                EntityOrder::Discard => m.removed = true,
            }
        }

        /// `hurt-area`: every living thing in the ball but the mob takes the amount as the
        /// mob's attack (`mob_attack`), then burns: mobs of the level, the player and the
        /// level's other players.
        fn mod_hurt_area(&mut self, source: u32, area: &EntityArea) {
            let center = [area.center.x, area.center.y, area.center.z];
            let r2 = area.radius * area.radius;
            // The nearest point of a box to the centre is within the radius.
            let reaches = move |feet: [f64; 3], half: f64, height: f64| {
                let lo = [feet[0] - half, feet[1], feet[2] - half];
                let hi = [feet[0] + half, feet[1] + height, feet[2] + half];
                let d2: f64 = (0..3)
                    .map(|i| (center[i].clamp(lo[i], hi[i]) - center[i]).powi(2))
                    .sum();
                d2 <= r2
            };
            let (amount, fire) = (area.amount, area.fire as i32 * 20);
            let mut mobs = std::mem::take(&mut self.mobs);
            for m in mobs.iter_mut().filter(|m| m.alive() && m.id != source) {
                let (w, h) = (m.kind.width / 2.0, m.kind.height);
                if !reaches([m.position.x, m.position.y, m.position.z], w, h) {
                    continue;
                }
                if amount > 0.0 {
                    m.damage(
                        amount,
                        Damage {
                            kind: DamageKind::MobAttack,
                            attacker: Some(MobWho::Mob(source)),
                            from: Some(center),
                        },
                    );
                }
                if fire > 0 {
                    m.ignite_for_ticks(fire);
                }
            }
            self.mobs = mobs;
            let hurt = move |g: &mut Self| {
                let Some(p) = g.player.as_ref().map(|p| p.position) else {
                    return;
                };
                if !reaches([p.x, p.y, p.z], 0.3, 1.8) {
                    return;
                }
                if amount > 0.0 {
                    g.hurt_player_from(
                        amount,
                        Some(center),
                        "minecraft:mob_attack",
                        Some(Some(source)),
                    );
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
            if !self.hears_players() {
                return false;
            }
            let players = self.server_players(None);
            let Some(me) = players.first().cloned() else {
                return false;
            };
            let snapshot = self.server_snapshot(players);
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world();
            let owned = self.mods.mods.server_command(&me, line);
            self.return_world();
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
            if !self.hears_players() {
                return false;
            }
            let Some(g) = host.hub.guest(conn) else {
                return false;
            };
            let id = uuid_string(g.uuid);
            let players = self.server_players(Some(host));
            let Some(me) = players.iter().find(|p| p.id == id).cloned() else {
                return false;
            };
            let snapshot = self.server_snapshot(players);
            self.mods.mods.set_snapshot(snapshot);
            self.lend_world();
            let owned = self.mods.mods.server_command(&me, line);
            self.return_world();
            if !owned {
                return false;
            }
            self.route_mod_output(Some(host));
            self.apply_mod_block_writes();
            self.apply_mod_orders();
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
                    if self.options.dedicated.is_some() {
                        super::super::dedicated::log("INFO", &plain);
                    } else {
                        log::info!("{plain}");
                    }
                    for p in self.server_players(host.as_deref()) {
                        let line = named(&name, text.clone());
                        self.tell_server_player(host.as_deref_mut(), &p.id, line);
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
