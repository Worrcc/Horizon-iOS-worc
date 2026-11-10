//! Server guest for API 20 queueing and caller-exclusion tests.
use horizon_mod_sdk::server::{
    BlockChangePos, BlockPos, BreakOptions, Player, Registration, ServerMod, players, world,
};
struct Guest;
impl ServerMod for Guest {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Self, Registration::new().on_tick().on_blocks()))
    }
    fn tick(&mut self, _tick: u64) {
        if let Some(player) = players::online().first() {
            world::break_blocks_as(
                &player.id,
                &[BlockChangePos { x: 1, y: 64, z: 0 }],
                BreakOptions {
                    keep_tool: true,
                    drops_at: None,
                    exhaustion: 0.1,
                },
            )
            .unwrap();
        }
    }
    fn block_broken(&mut self, player: &Player, _pos: BlockPos, _state: &str) {
        players::tell(&player.id, "heard break");
    }
}
horizon_mod_sdk::export_server_mod!(Guest);
