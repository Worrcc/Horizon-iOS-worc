//! Server guest for API 20 queueing and caller-exclusion tests.
use horizon_mod_sdk::server::{
    Args, BlockChangePos, BlockPos, BreakOptions, Player, Registration, ServerMod, channel,
    players, server_hooks, world,
};
struct Guest;
impl ServerMod for Guest {
    fn init() -> Result<(Self, Registration), String> {
        server_hooks::set_block_uses(true, false);
        Ok((
            Self,
            Registration::new()
                .on_tick()
                .on_blocks()
                .command("storage26")
                .command("sneak26"),
        ))
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
    fn command(
        &mut self,
        player: &Player,
        name: &str,
        _args: &Args,
    ) -> Result<Option<String>, String> {
        if name == "sneak26" {
            server_hooks::set_block_uses(true, true);
            return Ok(None);
        }
        let a = BlockChangePos { x: 0, y: 64, z: 0 };
        let b = BlockChangePos { x: 1, y: 64, z: 0 };
        let slots = world::container_stacks(&player.dimension, a).ok_or("no container")?;
        assert_eq!(
            slots[0].as_ref().unwrap().custom_name.as_deref(),
            Some("Named")
        );
        assert_eq!(
            world::container_move(&player.dimension, a, 0, b, None, 4)?,
            4
        );
        assert_eq!(
            world::container_to_player(&player.dimension, b, 0, 2, &player.id)?,
            2
        );
        assert_eq!(
            world::player_to_container(&player.id, 0, &player.dimension, a, 1)?,
            1
        );
        assert!(world::container_move(&player.dimension, a, 99, b, None, 1).is_err());
        assert!(channel::send("state", &[]).is_err());
        assert!(channel::send_to_player("offline", "state", &[]).is_err());
        channel::send_to_player(&player.name, "state", &[7, 8, 9])?;
        Ok(None)
    }
    fn block_used(
        &mut self,
        player: &Player,
        _pos: BlockPos,
        state: &str,
        _sneaking: bool,
    ) -> bool {
        players::tell(&player.id, "heard use");
        state == "test:terminal"
    }
    fn channel_message(&mut self, player: &Player, name: &str, data: &[u8]) {
        channel::send_to_player(&player.id, name, data).unwrap();
    }
    fn block_broken(&mut self, player: &Player, _pos: BlockPos, _state: &str) {
        players::tell(&player.id, "heard break");
    }
}
horizon_mod_sdk::export_server_mod!(Guest);
