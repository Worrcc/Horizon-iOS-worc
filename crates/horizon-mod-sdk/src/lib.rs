//! Write Horizon mods in Rust (`docs/mods.md`).
//!
//! API level 28: [`canvas::set_screen_items`] and [`canvas::set_box_items`]
//! draw item icons on canvases; [`server::world::player_stacks`] reads an online
//! player's main inventory, including earlier storage reservations in the batch.
//!
//! A mod is a type implementing [`Mod`], exported with [`export_mod!`] from
//! a `cdylib` crate built for `wasm32-unknown-unknown`;
//! `cargo horizon-mod build` turns it into the component the game loads.
//!
//! ```ignore
//! use horizon_mod_sdk::{Mod, Registration, chat};
//!
//! struct Hello;
//! impl Mod for Hello {
//!     fn init() -> Result<(Self, Registration), String> {
//!         Ok((Hello, Registration::new().command("hello")))
//!     }
//!     fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
//!         chat::show("Hello from a mod!");
//!         Ok(None)
//!     }
//! }
//! horizon_mod_sdk::export_mod!(Hello);
//! ```
//!
//! Every interface but `log` is a capability: list the ones the mod uses in
//! `horizon-mod.toml` (`game-read`, `chat`, `hud`, `storage`, `command`, `ui`;
//! a server mod `storage` and `players`). Server mods: [`server`].
//!
//! Widgets (capability `ui`): a mod shows a screen it owns with
//! [`ui::open_screen`] and a page in Options > Mods with
//! [`ui::set_settings`]; what the player does with them comes back as
//! [`Mod::ui_select`], [`Mod::ui_closed`] and [`Mod::ui_settings_opened`].
//!
//! Pixels (capability `render`, API level 17): [`canvas::create_image`] and
//! [`canvas::update_image`] fill images that [`canvas::set_corner`] shows in
//! a corner box; [`canvas::request_frame_budget`] and [`canvas::work_left`]
//! keep heavy `frame` work to a time budget. The loaded world (capability
//! `world-read`, API level 17): [`client_world::columns`] reads whole chunks,
//! [`client_world::block_table`] and [`client_world::texture`] the packs.
//! Level 23 adds [`client_world::set_see_through`] and
//! [`client_world::prepared_columns`] to prepare columns natively.
//! Level 24 adds [`client_world::harvest`] (the tool and tier a block needs)
//! and [`input::set_auto_button_shown`]: automatic touch buttons start hidden.
//! Level 25 adds [`input::set_touch_button_context`] for riding/held-item visibility.
//! Level 26 adds [`server::players::teleport`] for vanilla player teleports across dimensions.
//! [`server::players::permission_level`] reads an online player's vanilla op level.
//! [`server::world::spawn_point`] reads the overworld's world spawn and angle.
//! Level 27 adds [`input::set_aim_assist`] for controller/touch look and [`input::look_device`].
//! Level 28 adds server block-use hooks, scoped mod channels and component-preserving container moves.
//! Level 29 adds behaviour hooks and [`server::world::player_held_data`], with
//! scoped UTF-8 data in minecraft:custom_data (1024 bytes; JSON/base64 allowed).
//! Data writes and creation share storage moves' 64 requests/mod/tick and apply
//! after hooks. [`server::StackInfoWithData`] leaves the older record unchanged.
//!
//! API level 20: [`command::run_mod_quiet`] invokes server-mod commands with replies discarded.
//! [`input::TouchButton`] accepts an optional item icon alongside its fallback label.
//! API level 21: [`canvas::set_box_key`] makes a touch on the HUD box hold a registered mod key.
//! [`chat::open`] prefills the player's chat box without sending anything; touch chat opens
//! the on-screen keyboard. A leading `/` prefills a command.

wit_bindgen::generate!({
    path: "wit",
    world: "horizon-ui-mod",
    pub_export_macro: true,
    export_macro_name: "__export_horizon_mod",
    default_bindings_module: "horizon_mod_sdk",
});

pub use exports::horizon::mods::channel_events::Guest as __ChannelEventsGuest;
pub use exports::horizon::mods::container_events::{
    Guest as __ContainerEventsGuest, PointerKind as ContainerPointerKind,
};
pub use exports::horizon::mods::events::{Guest as __Guest, Registration};
pub use exports::horizon::mods::ui_events::{Guest as __UiEventsGuest, PointerKind};
pub use horizon::mods::{
    browser, browser_ext, canvas, channel, chat, client_world, command, container, game, hud,
    input, log, recipes, render, storage, ui,
};

/// `client_world::BlockStates::state_flags`' bits (capability `world-read`).
pub mod state_flags {
    pub const AIR: u16 = 1;
    /// Drawn as nothing: a barrier, a light block, a structure void.
    pub const INVISIBLE: u16 = 2;
    pub const WATER: u16 = 4;
    pub const LAVA: u16 = 8;
    pub const WATERLOGGED: u16 = 16;
    /// See-through like glass, ice or leaves.
    pub const SEE_THROUGH: u16 = 32;
    /// No collision, like a plant or a torch.
    pub const NO_COLLISION: u16 = 64;
    /// A full face on top (a minimap draws the block, not what's under it).
    pub const FULL_TOP: u16 = 128;
    /// The light a state gives (0 to 15), from these bits.
    pub fn emission(flags: u16) -> u8 {
        (flags >> 8) as u8 & 15
    }
}
/// `client_world::ChunkColumns::tops` for a column with no block.
pub const NO_TOP: i16 = i16::MIN;
/// `client_world::Face::sprite` for a side that draws nothing.
pub const NO_SPRITE: u32 = u32::MAX;

/// The game's calls into a mod. Only the hooks [`Registration`] asks for
/// are called; the others may stay as their defaults.
pub trait Mod: Sized + 'static {
    /// Once, after loading: the mod's state and the hooks it wants. An error
    /// keeps the mod off and is shown to the player.
    fn init() -> Result<(Self, Registration), String>;
    /// Every game tick, when [`Registration::tick`] is set.
    fn tick(&mut self, _tick: u64) {}
    /// API 28: a message from this mod id on the server.
    fn channel_message(&mut self, _name: &str, _data: &[u8]) {}
    /// Every rendered frame, when [`Registration::frame`] is set.
    fn frame(&mut self, _seconds: f32) {}
    /// `/name args` for a command the mod registered. `Ok(Some(text))` is
    /// shown as feedback, `Err(text)` in red.
    fn command(&mut self, _name: &str, _args: &str) -> Result<Option<String>, String> {
        Err("This command does nothing".into())
    }
    /// A chat line the player received, when [`Registration::chat`] is set.
    fn chat(&mut self, _text: &str) {}
    /// A row of one of the mod's `list` widgets was picked (capability
    /// `ui`): the widget's `id` and the row.
    fn ui_select(&mut self, _id: u32, _row: u32) {}
    /// Pointer action on the canvas screen, in GUI pixels (API level 18).
    fn ui_pointer(&mut self, _kind: PointerKind, _x: f32, _y: f32, _button: u32) {}
    /// Wheel on the canvas screen; positive steps away from the player.
    fn ui_scroll(&mut self, _x: f32, _y: f32, _steps: f32) {}
    /// A key.keyboard.* key on the canvas screen, excluding Escape.
    fn ui_screen_key(&mut self, _key: &str, _down: bool) {}
    /// Before vanilla container input (API 22, capability `container`).
    fn container_pointer(
        &mut self,
        _kind: ContainerPointerKind,
        _x: f32,
        _y: f32,
        _button: u32,
        _slot: Option<u32>,
        _shift: bool,
    ) {
    }
    /// True consumes the container wheel/two-finger scroll.
    fn container_scroll(&mut self, _x: f32, _y: f32, _steps: f32, _slot: Option<u32>) -> bool {
        false
    }
    fn container_button(&mut self, _id: u32) {}
    fn container_opened(&mut self) {}
    fn container_closed(&mut self) {}
    /// The player left the mod's screen (Esc or Done; capability `ui`).
    fn ui_closed(&mut self) {}
    /// The player opened the mod's settings page (capability `ui`): call
    /// [`ui::set_settings`] for fresh widgets.
    fn ui_settings_opened(&mut self) {}
}

impl Registration {
    /// No commands and no hooks.
    pub fn new() -> Self {
        Self {
            commands: Vec::new(),
            tick: false,
            frame: false,
            chat: false,
        }
    }
    /// Adds `/name`.
    pub fn command(mut self, name: &str) -> Self {
        self.commands.push(name.into());
        self
    }
    pub fn on_tick(mut self) -> Self {
        self.tick = true;
        self
    }
    pub fn on_frame(mut self) -> Self {
        self.frame = true;
        self
    }
    pub fn on_chat(mut self) -> Self {
        self.chat = true;
        self
    }
}
impl Default for Registration {
    fn default() -> Self {
        Self::new()
    }
}

/// `log::write` at info level.
pub fn info(message: &str) {
    log::write(log::Level::Info, message);
}
/// `log::write` at warn level.
pub fn warn(message: &str) {
    log::write(log::Level::Warn, message);
}

/// Exports a [`Mod`] as the component's `events` interface. The state lives
/// in a thread local: a mod instance is single-threaded.
#[macro_export]
macro_rules! export_mod {
    ($t:ty) => {
        const _: () = {
            use ::std::cell::RefCell;
            ::std::thread_local! {
                static STATE: RefCell<Option<$t>> = const { RefCell::new(None) };
            }
            fn with(f: impl FnOnce(&mut $t)) {
                STATE.with(|s| {
                    if let Some(m) = s.borrow_mut().as_mut() {
                        f(m)
                    }
                })
            }
            struct Export;
            impl $crate::__Guest for Export {
                fn init() -> Result<$crate::Registration, String> {
                    let (m, registration) = <$t as $crate::Mod>::init()?;
                    STATE.with(|s| *s.borrow_mut() = Some(m));
                    Ok(registration)
                }
                fn tick(tick: u64) {
                    with(|m| $crate::Mod::tick(m, tick))
                }
                fn frame(seconds: f32) {
                    with(|m| $crate::Mod::frame(m, seconds))
                }
                fn command(name: String, args: String) -> Result<Option<String>, String> {
                    let mut out = Err("The mod is not running".to_string());
                    with(|m| out = $crate::Mod::command(m, &name, &args));
                    out
                }
                fn chat(text: String) {
                    with(|m| $crate::Mod::chat(m, &text))
                }
            }
            impl $crate::__ChannelEventsGuest for Export {
                fn message(name: String, data: Vec<u8>) {
                    with(|m| $crate::Mod::channel_message(m, &name, &data))
                }
            }
            impl $crate::__UiEventsGuest for Export {
                fn pointer(kind: $crate::PointerKind, x: f32, y: f32, button: u32) {
                    with(|m| $crate::Mod::ui_pointer(m, kind, x, y, button))
                }
                fn scroll(x: f32, y: f32, steps: f32) {
                    with(|m| $crate::Mod::ui_scroll(m, x, y, steps))
                }
                fn screen_key(key: String, down: bool) {
                    with(|m| $crate::Mod::ui_screen_key(m, &key, down))
                }
                fn select(id: u32, row: u32) {
                    with(|m| $crate::Mod::ui_select(m, id, row))
                }
                fn closed() {
                    with(|m| $crate::Mod::ui_closed(m))
                }
                fn settings_opened() {
                    with(|m| $crate::Mod::ui_settings_opened(m))
                }
            }
            impl $crate::__ContainerEventsGuest for Export {
                fn pointer(
                    kind: $crate::ContainerPointerKind,
                    x: f32,
                    y: f32,
                    button: u32,
                    slot: Option<u32>,
                    shift: bool,
                ) {
                    with(|m| $crate::Mod::container_pointer(m, kind, x, y, button, slot, shift))
                }
                fn scroll(x: f32, y: f32, steps: f32, slot: Option<u32>) -> bool {
                    let mut consumed = false;
                    with(|m| consumed = $crate::Mod::container_scroll(m, x, y, steps, slot));
                    consumed
                }
                fn button(id: u32) {
                    with(|m| $crate::Mod::container_button(m, id))
                }
                fn opened() {
                    with(|m| $crate::Mod::container_opened(m))
                }
                fn closed() {
                    with(|m| $crate::Mod::container_closed(m))
                }
            }
            // Only WebAssembly has the component's exports: a host build (cargo test or
            // clippy on the workspace) checks the mod without them, which macOS
            // couldn't link (its exported-symbols list cuts names at the `#`).
            #[cfg(target_arch = "wasm32")]
            $crate::__export_horizon_mod!(Export);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = (
                <Export as $crate::__Guest>::init,
                <Export as $crate::__UiEventsGuest>::select,
            );
        };
    };
}

/// Server mods (manifest `side = "server"`, `api = "0.3"`, experimental): a
/// type implementing [`ServerMod`](server::ServerMod), exported with
/// [`export_server_mod!`]. It runs where the world is simulated, and reads
/// and messages the players with capability `players`.
pub mod server {
    wit_bindgen::generate!({
        path: "wit",
        world: "horizon-server-mod",
        pub_export_macro: true,
        export_macro_name: "__export_horizon_server_mod",
        default_bindings_module: "horizon_mod_sdk::server",
        with: {
            "horizon:mods/log@0.1.0": crate::log,
            "horizon:mods/storage@0.1.0": crate::storage,
        },
    });

    pub use exports::horizon::mods::server_events::{
        Argument, ArgumentKind, ArgumentValue, BlockPos, ChatDecision, Decision, Guest as __Guest,
        Hand as BehaviourHand, Registration, ServerCommand,
    };
    pub use horizon::mods::entities::{
        self, Area, AttributeChange, Death, EffectChange, Entity, HurtEvent, Mount, Order,
        Particles, Point, Rider, Sound, StrikeHit, Target, Walk,
    };
    pub use horizon::mods::players::{
        self, ActiveEffect, Attack, BlockHit, EquipmentSlots, GameMode, ItemFilter, ItemUse, Look,
        Player, PlayerDamage, PlayerDeath, PlayerRespawn, PlayerVitals, Point as PlayerPoint,
        TeleportTarget,
    };
    pub use horizon::mods::world::{
        self, BlastKind, BlockChange, BlockChangePos, BreakOptions, ChunkPos, DustColor, ItemStack,
        ParticleBurst, Point as WorldPoint, Stack as MachineStack, StackInfo, StackInfoWithData,
        WorldTime,
    };
    pub use horizon::mods::{channel, menus, server_hooks};
    pub use menus::MenuItem;
    pub use server_hooks::BehaviourConfig;
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

    /// The server's calls into a mod. Only the hooks [`Registration`] asks
    /// for are called; the others may stay as their defaults.
    pub trait ServerMod: Sized + 'static {
        /// Once, after loading: the mod's state and the hooks it wants. An
        /// error keeps the mod off and is logged.
        fn init() -> Result<(Self, Registration), String>;
        /// Every server tick, when [`Registration::tick`] is set.
        fn tick(&mut self, _tick: u64) {}
        /// When [`Registration::joins`] is set.
        fn player_joined(&mut self, _player: &Player) {}
        fn player_left(&mut self, _player: &Player) {}
        /// A chat message a player sent, when [`Registration::chat`] is set.
        fn chat(&mut self, _player: &Player, _text: &str) {}
        /// `/name args` typed by `player`, parsed as the command's
        /// arguments say ([`Registration::command_with`]). `Ok(Some(text))`
        /// is their feedback, `Err(text)` their error in red.
        fn command(
            &mut self,
            _player: &Player,
            _name: &str,
            _args: &Args,
        ) -> Result<Option<String>, String> {
            Err("This command does nothing".into())
        }
        /// When [`Registration::blocks`] is set: `state` is what was there.
        fn block_broken(&mut self, _player: &Player, _pos: BlockPos, _state: &str) {}
        fn block_placed(&mut self, _player: &Player, _pos: BlockPos, _state: &str) {}
        fn block_neighbour_changed(
            &mut self,
            _dimension: &str,
            _pos: BlockPos,
            _state: &str,
            _from: &str,
        ) {
        }
        fn block_random_tick(&mut self, _dimension: &str, _pos: BlockPos, _state: &str) {}
        fn menu_clicked(&mut self, _player: &Player, _slot: i32, _button: i32, _mode: i32) {}
        fn menu_closed(&mut self, _player: &Player) {}
        fn block_scheduled_tick(&mut self, _dimension: &str, _pos: BlockPos, _state: &str) {}
        fn item_used_on_block(
            &mut self,
            _player: &Player,
            _item: &str,
            _pos: BlockPos,
            _state: &str,
            _face: i32,
        ) -> Decision {
            Decision::Allow
        }
        fn item_used(&mut self, _player: &Player, _item: &str) -> Decision {
            Decision::Allow
        }
        fn item_hit_entity(
            &mut self,
            _player: &Player,
            _item: &str,
            _entity_kind: &str,
            _entity_id: i32,
        ) -> Decision {
            Decision::Allow
        }
        fn item_use_finished(&mut self, _player: &Player, _item: &str) {}
        fn item_picked_up(&mut self, _player: &Player, _item: &str) -> Decision {
            Decision::Allow
        }
        fn chat_sent_filter(&mut self, _player: &Player, _text: &str) -> ChatDecision {
            ChatDecision::Allow
        }
        /// API 28: opt in with server_hooks::set_block_uses during init.
        fn block_used(
            &mut self,
            _player: &Player,
            _pos: BlockPos,
            _state: &str,
            _sneaking: bool,
        ) -> bool {
            false
        }
        #[allow(clippy::too_many_arguments)]
        fn block_interacted(
            &mut self,
            _player: &Player,
            _pos: BlockPos,
            _state: &str,
            _face: i32,
            _hit_x: f64,
            _hit_y: f64,
            _hit_z: f64,
            _hand: BehaviourHand,
            _item: &str,
            _sneaking: bool,
        ) -> Decision {
            Decision::Allow
        }
        fn block_breaking(&mut self, _player: &Player, _pos: BlockPos, _state: &str) -> Decision {
            Decision::Allow
        }
        fn block_placing(
            &mut self,
            _player: &Player,
            _pos: BlockPos,
            _state: &str,
            _item: &str,
        ) -> Decision {
            Decision::Allow
        }
        fn channel_message(&mut self, _player: &Player, _name: &str, _data: &[u8]) {}
    }

    impl Registration {
        /// No commands and no hooks.
        pub fn new() -> Self {
            Self {
                commands: Vec::new(),
                tick: false,
                joins: false,
                chat: false,
                blocks: false,
            }
        }
        /// Adds `/name`, taking free text ([`Args::text`]): one optional
        /// `text` argument named `args`.
        pub fn command(self, name: &str) -> Self {
            self.command_with(name, [Argument::text("args").optional()])
        }
        /// Adds `/name` with typed arguments (none: nothing after the name):
        /// players' chat boxes complete and check them, and the server
        /// parses them into [`Args`].
        pub fn command_with(
            mut self,
            name: &str,
            arguments: impl IntoIterator<Item = Argument>,
        ) -> Self {
            self.commands.push(ServerCommand {
                name: name.into(),
                arguments: arguments.into_iter().collect(),
            });
            self
        }
        pub fn on_tick(mut self) -> Self {
            self.tick = true;
            self
        }
        pub fn on_joins(mut self) -> Self {
            self.joins = true;
            self
        }
        pub fn on_chat(mut self) -> Self {
            self.chat = true;
            self
        }
        pub fn on_blocks(mut self) -> Self {
            self.blocks = true;
            self
        }
    }
    impl Default for Registration {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Argument {
        fn of(name: &str, kind: ArgumentKind) -> Self {
            Self {
                name: name.into(),
                kind,
                optional: false,
            }
        }
        /// One word: letters, digits and `_ - . +`.
        pub fn word(name: &str) -> Self {
            Self::of(name, ArgumentKind::Word)
        }
        /// The rest of the line: only last.
        pub fn text(name: &str) -> Self {
            Self::of(name, ArgumentKind::Text)
        }
        /// A whole number, at least `min` and at most `max` when given.
        pub fn integer(name: &str, min: Option<i32>, max: Option<i32>) -> Self {
            Self::of(name, ArgumentKind::Integer((min, max)))
        }
        pub fn number(name: &str) -> Self {
            Self::of(name, ArgumentKind::Number)
        }
        pub fn boolean(name: &str) -> Self {
            Self::of(name, ArgumentKind::Boolean)
        }
        /// One online player: a name, `@s` or `@p`.
        pub fn player(name: &str) -> Self {
            Self::of(name, ArgumentKind::Player)
        }
        /// `x y z`, each one also `~` or `~n`.
        pub fn block_pos(name: &str) -> Self {
            Self::of(name, ArgumentKind::BlockPos)
        }
        /// May be left out; optional arguments come after the required ones.
        pub fn optional(mut self) -> Self {
            self.optional = true;
            self
        }
    }

    /// A command's arguments: the line as typed, and the values the server
    /// parsed from it, by argument name.
    #[derive(Clone, Debug, Default)]
    pub struct Args {
        text: String,
        values: Vec<(String, ArgumentValue)>,
    }
    impl Args {
        pub fn new(text: String, values: Vec<(String, ArgumentValue)>) -> Self {
            Self { text, values }
        }
        /// The line after the command's name, as typed.
        pub fn text(&self) -> &str {
            &self.text
        }
        /// The value of argument `name`; `None` when it was left out.
        pub fn get(&self, name: &str) -> Option<&ArgumentValue> {
            self.values.iter().find(|(n, _)| n == name).map(|(_, v)| v)
        }
        pub fn integer(&self, name: &str) -> Option<i32> {
            match self.get(name)? {
                ArgumentValue::Integer(n) => Some(*n),
                _ => None,
            }
        }
        pub fn number(&self, name: &str) -> Option<f64> {
            match self.get(name)? {
                ArgumentValue::Number(n) => Some(*n),
                _ => None,
            }
        }
        /// A `word` or `text` argument.
        pub fn string(&self, name: &str) -> Option<&str> {
            match self.get(name)? {
                ArgumentValue::Word(s) | ArgumentValue::Text(s) => Some(s),
                _ => None,
            }
        }
        pub fn boolean(&self, name: &str) -> Option<bool> {
            match self.get(name)? {
                ArgumentValue::Boolean(b) => Some(*b),
                _ => None,
            }
        }
        pub fn player(&self, name: &str) -> Option<&Player> {
            match self.get(name)? {
                ArgumentValue::Player(p) => Some(p),
                _ => None,
            }
        }
        pub fn block_pos(&self, name: &str) -> Option<BlockPos> {
            match self.get(name)? {
                ArgumentValue::BlockPos(p) => Some(*p),
                _ => None,
            }
        }
    }
}

/// Exports a [`ServerMod`](server::ServerMod) as the component's
/// `server-events` interface, as [`export_mod!`] does for a client mod.
#[macro_export]
macro_rules! export_server_mod {
    ($t:ty) => {
        const _: () = {
            use ::std::cell::RefCell;
            use $crate::server::{Args, ArgumentValue, BlockPos, Player, Registration, ServerMod};
            ::std::thread_local! {
                static STATE: RefCell<Option<$t>> = const { RefCell::new(None) };
            }
            fn with(f: impl FnOnce(&mut $t)) {
                STATE.with(|s| {
                    if let Some(m) = s.borrow_mut().as_mut() {
                        f(m)
                    }
                })
            }
            struct Export;
            impl $crate::server::__Guest for Export {
                fn init() -> Result<Registration, String> {
                    let (m, registration) = <$t as ServerMod>::init()?;
                    STATE.with(|s| *s.borrow_mut() = Some(m));
                    Ok(registration)
                }
                fn tick(tick: u64) {
                    with(|m| m.tick(tick))
                }
                fn player_joined(player: Player) {
                    with(|m| m.player_joined(&player))
                }
                fn player_left(player: Player) {
                    with(|m| m.player_left(&player))
                }
                fn chat(player: Player, text: String) {
                    with(|m| m.chat(&player, &text))
                }
                fn command(
                    player: Player,
                    name: String,
                    args: String,
                    values: Vec<(String, ArgumentValue)>,
                ) -> Result<Option<String>, String> {
                    let mut out = Err("The mod is not running".to_string());
                    let args = Args::new(args, values);
                    with(|m| out = m.command(&player, &name, &args));
                    out
                }
                fn block_neighbour_changed(
                    dimension: String,
                    pos: BlockPos,
                    state: String,
                    from: String,
                ) {
                    with(|m| m.block_neighbour_changed(&dimension, pos, &state, &from))
                }
                fn block_random_tick(dimension: String, pos: BlockPos, state: String) {
                    with(|m| m.block_random_tick(&dimension, pos, &state))
                }
                fn menu_clicked(player: Player, slot: i32, button: i32, mode: i32) {
                    with(|m| m.menu_clicked(&player, slot, button, mode))
                }
                fn menu_closed(player: Player) {
                    with(|m| m.menu_closed(&player))
                }
                fn block_scheduled_tick(dimension: String, pos: BlockPos, state: String) {
                    with(|m| m.block_scheduled_tick(&dimension, pos, &state))
                }
                fn item_used_on_block(
                    player: Player,
                    item: String,
                    pos: BlockPos,
                    state: String,
                    face: i32,
                ) -> $crate::server::Decision {
                    let mut answer = $crate::server::Decision::Allow;
                    with(|m| answer = m.item_used_on_block(&player, &item, pos, &state, face));
                    answer
                }
                fn item_used(player: Player, item: String) -> $crate::server::Decision {
                    let mut answer = $crate::server::Decision::Allow;
                    with(|m| answer = m.item_used(&player, &item));
                    answer
                }
                fn item_hit_entity(
                    player: Player,
                    item: String,
                    entity_kind: String,
                    entity_id: i32,
                ) -> $crate::server::Decision {
                    let mut answer = $crate::server::Decision::Allow;
                    with(|m| answer = m.item_hit_entity(&player, &item, &entity_kind, entity_id));
                    answer
                }
                fn item_use_finished(player: Player, item: String) {
                    with(|m| m.item_use_finished(&player, &item));
                }
                fn item_picked_up(player: Player, item: String) -> $crate::server::Decision {
                    let mut answer = $crate::server::Decision::Allow;
                    with(|m| answer = m.item_picked_up(&player, &item));
                    answer
                }
                fn chat_sent_filter(player: Player, text: String) -> $crate::server::ChatDecision {
                    let mut answer = $crate::server::ChatDecision::Allow;
                    with(|m| answer = m.chat_sent_filter(&player, &text));
                    answer
                }
                fn block_used(
                    player: Player,
                    pos: BlockPos,
                    state: String,
                    sneaking: bool,
                ) -> bool {
                    let mut consumed = false;
                    with(|m| consumed = m.block_used(&player, pos, &state, sneaking));
                    consumed
                }
                fn block_interacted(
                    player: Player,
                    pos: BlockPos,
                    state: String,
                    face: i32,
                    hit_x: f64,
                    hit_y: f64,
                    hit_z: f64,
                    hand: $crate::server::BehaviourHand,
                    item: String,
                    sneaking: bool,
                ) -> $crate::server::Decision {
                    let mut answer = $crate::server::Decision::Allow;
                    with(|m| {
                        answer = m.block_interacted(
                            &player, pos, &state, face, hit_x, hit_y, hit_z, hand, &item, sneaking,
                        )
                    });
                    answer
                }
                fn block_breaking(
                    player: Player,
                    pos: BlockPos,
                    state: String,
                ) -> $crate::server::Decision {
                    let mut answer = $crate::server::Decision::Allow;
                    with(|m| answer = m.block_breaking(&player, pos, &state));
                    answer
                }
                fn block_placing(
                    player: Player,
                    pos: BlockPos,
                    state: String,
                    item: String,
                ) -> $crate::server::Decision {
                    let mut answer = $crate::server::Decision::Allow;
                    with(|m| answer = m.block_placing(&player, pos, &state, &item));
                    answer
                }
                fn channel_message(player: Player, name: String, data: Vec<u8>) {
                    with(|m| m.channel_message(&player, &name, &data))
                }
                fn block_broken(player: Player, pos: BlockPos, state: String) {
                    with(|m| m.block_broken(&player, pos, &state))
                }
                fn block_placed(player: Player, pos: BlockPos, state: String) {
                    with(|m| m.block_placed(&player, pos, &state))
                }
            }
            #[cfg(target_arch = "wasm32")]
            $crate::server::__export_horizon_server_mod!(Export);
            #[cfg(not(target_arch = "wasm32"))]
            let _ = <Export as $crate::server::__Guest>::init;
        };
    };
}
