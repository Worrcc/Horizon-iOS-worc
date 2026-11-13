//! Resolve server exports individually so pre-28 components stay loadable.
use super::*;

type CommandResult = Result<Option<String>, String>;
type CommandParams = (ServerPlayer, String, String, Vec<(String, ArgumentValue)>);

pub(super) struct ServerCallbacks {
    init: TypedFunc<(), (Result<ServerRegistration, String>,)>,
    tick: TypedFunc<(u64,), ()>,
    joined: TypedFunc<(ServerPlayer,), ()>,
    left: TypedFunc<(ServerPlayer,), ()>,
    chat: TypedFunc<(ServerPlayer, String), ()>,
    command: TypedFunc<CommandParams, (CommandResult,)>,
    broken: TypedFunc<(ServerPlayer, BlockPos, String), ()>,
    placed: TypedFunc<(ServerPlayer, BlockPos, String), ()>,
    pub used: Option<TypedFunc<(ServerPlayer, BlockPos, String, bool), (bool,)>>,
    pub channel: Option<TypedFunc<(ServerPlayer, String, Vec<u8>), ()>>,
}
impl ServerCallbacks {
    pub fn new(store: &mut Store<State>, instance: &Instance) -> wasmtime::Result<Self> {
        let interface = instance
            .get_export_index(&mut *store, None, "horizon:mods/server-events@0.1.0")
            .ok_or_else(|| wasmtime::Error::msg("missing server-events"))?;
        macro_rules! get {
            ($name:literal) => {
                instance
                    .get_export_index(&mut *store, Some(&interface), $name)
                    .and_then(|i| instance.get_typed_func(&mut *store, i).ok())
            };
        }
        macro_rules! required {
            ($name:literal) => {
                get!($name)
                    .ok_or_else(|| wasmtime::Error::msg(concat!("missing server-events.", $name)))?
            };
        }
        Ok(Self {
            init: required!("init"),
            tick: required!("tick"),
            joined: required!("player-joined"),
            left: required!("player-left"),
            chat: required!("chat"),
            command: required!("command"),
            broken: required!("block-broken"),
            placed: required!("block-placed"),
            used: get!("block-used"),
            channel: get!("channel-message"),
        })
    }
    pub fn horizon_mods_server_events(&self) -> &Self {
        self
    }
    pub fn call_init(
        &self,
        s: &mut Store<State>,
    ) -> wasmtime::Result<Result<ServerRegistration, String>> {
        let (r,) = self.init.call(&mut *s, ())?;
        Ok(r)
    }
    pub fn call_tick(&self, s: &mut Store<State>, tick: u64) -> wasmtime::Result<()> {
        self.tick.call(&mut *s, (tick,))?;
        Ok(())
    }
    pub fn call_player_joined(
        &self,
        s: &mut Store<State>,
        p: &ServerPlayer,
    ) -> wasmtime::Result<()> {
        self.joined.call(&mut *s, (p.clone(),))?;
        Ok(())
    }
    pub fn call_player_left(&self, s: &mut Store<State>, p: &ServerPlayer) -> wasmtime::Result<()> {
        self.left.call(&mut *s, (p.clone(),))?;
        Ok(())
    }
    pub fn call_chat(
        &self,
        s: &mut Store<State>,
        p: &ServerPlayer,
        text: &str,
    ) -> wasmtime::Result<()> {
        self.chat.call(&mut *s, (p.clone(), text.into()))?;
        Ok(())
    }
    pub fn call_command(
        &self,
        s: &mut Store<State>,
        p: &ServerPlayer,
        name: &str,
        args: &str,
        values: &[(String, ArgumentValue)],
    ) -> wasmtime::Result<CommandResult> {
        let (r,) = self.command.call(
            &mut *s,
            (p.clone(), name.into(), args.into(), values.to_vec()),
        )?;
        Ok(r)
    }
    pub fn call_block_broken(
        &self,
        s: &mut Store<State>,
        p: &ServerPlayer,
        pos: BlockPos,
        state: &str,
    ) -> wasmtime::Result<()> {
        self.broken.call(&mut *s, (p.clone(), pos, state.into()))?;
        Ok(())
    }
    pub fn call_block_placed(
        &self,
        s: &mut Store<State>,
        p: &ServerPlayer,
        pos: BlockPos,
        state: &str,
    ) -> wasmtime::Result<()> {
        self.placed.call(&mut *s, (p.clone(), pos, state.into()))?;
        Ok(())
    }
}
