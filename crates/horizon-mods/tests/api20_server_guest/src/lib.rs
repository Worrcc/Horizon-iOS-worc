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
                .command("sneak26")
                .command("player_peek26"),
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
        if name == "player_peek26" {
            assert!(world::player_stacks("offline").is_none());
            let by_id = world::player_stacks(&player.id);
            let by_name = world::player_stacks(&player.name);
            assert_eq!(by_id.is_some(), by_name.is_some());
            if let (Some(a), Some(b)) = (&by_id, &by_name) {
                assert_eq!(a.len(), b.len());
                for (a, b) in a.iter().zip(b) {
                    assert_eq!(a.is_some(), b.is_some());
                    if let (Some(a), Some(b)) = (a, b) {
                        assert_eq!(
                            (&a.item, a.count, &a.display_key, &a.custom_name),
                            (&b.item, b.count, &b.display_key, &b.custom_name)
                        );
                    }
                }
            }
            return Ok(Some(
                match by_id {
                    Some(slots) => {
                        assert_eq!(slots.len(), 36);
                        assert_eq!(slots.iter().filter(|s| s.is_some()).count(), 1);
                        let stack = slots[35].as_ref().unwrap();
                        assert_eq!(stack.item, "minecraft:stone");
                        assert_eq!(stack.count, 12);
                        assert_eq!(stack.display_key, "named-enchanted");
                        assert_eq!(stack.custom_name.as_deref(), Some("Named"));
                        "player slots checked"
                    }
                    None => "no player inventory",
                }
                .into(),
            ));
        }
        let inventory = world::player_stacks(&player.id).ok_or("no player inventory")?;
        assert_eq!(inventory.len(), 36);
        assert!(inventory.iter().all(Option::is_none));
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
        let inventory = world::player_stacks(&player.name).unwrap();
        let stack = inventory[0].as_ref().unwrap();
        assert_eq!(stack.item, "minecraft:stone");
        assert_eq!(stack.count, 2);
        assert_eq!(stack.display_key, "named-enchanted");
        assert_eq!(stack.custom_name.as_deref(), Some("Named"));
        assert_eq!(
            world::player_to_container(&player.id, 0, &player.dimension, a, 1)?,
            1
        );
        let inventory = world::player_stacks(&player.id).unwrap();
        assert_eq!(inventory[0].as_ref().unwrap().count, 1);
        assert_eq!(inventory.iter().flatten().count(), 1);
        assert!(world::player_stacks("offline").is_none());
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
