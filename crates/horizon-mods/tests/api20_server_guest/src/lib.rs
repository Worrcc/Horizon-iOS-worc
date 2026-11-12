//! Server guest for API 20 queueing and caller-exclusion tests.
use horizon_mod_sdk::server::{
    Args, BlockChangePos, BlockPos, BreakOptions, Player, PlayerPoint, Registration, ServerMod,
    TeleportTarget, players, world,
};
struct Guest;
impl ServerMod for Guest {
    fn init() -> Result<(Self, Registration), String> {
        Ok((
            Self,
            Registration::new()
                .on_tick()
                .on_blocks()
                .command("api26_teleport")
                .command("api26_permission")
                .command("api26_spawn"),
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
        args: &Args,
    ) -> Result<Option<String>, String> {
        let mode = args.text();
        match name {
            "api26_teleport" => {
                let mut target = TeleportTarget {
                    dimension: "minecraft:overworld".into(),
                    at: PlayerPoint {
                        x: 12.0,
                        y: 70.25,
                        z: -4.5,
                    },
                    yaw: None,
                    pitch: None,
                };
                match mode {
                    "unknown" => target.dimension = "test:missing".into(),
                    "cross" => {
                        target.dimension = "minecraft:the_nether".into();
                        target.yaw = Some(90.0);
                        target.pitch = Some(-30.0);
                    }
                    "yaw" => target.yaw = Some(45.0),
                    "nan" => target.at.x = f64::NAN,
                    "infinite" => target.at.z = f64::INFINITY,
                    "angle" => target.pitch = Some(f32::INFINITY),
                    "border" => target.at.x = 100.0,
                    "height" => target.at.y = 20_000_000.0,
                    _ => {}
                }
                let id = if mode == "offline" {
                    "offline"
                } else {
                    &player.id
                };
                players::teleport(id, &target)?;
                Ok(Some("queued".into()))
            }
            "api26_permission" => {
                let id = if mode == "offline" {
                    "offline"
                } else {
                    &player.id
                };
                Ok(Some(format!("{:?}", players::permission_level(id))))
            }
            "api26_spawn" => {
                let (dimension, at, yaw) = world::spawn_point();
                Ok(Some(format!(
                    "{dimension} {} {} {} {yaw}",
                    at.x, at.y, at.z
                )))
            }
            _ => Err("unknown test command".into()),
        }
    }
    fn block_broken(&mut self, player: &Player, _pos: BlockPos, _state: &str) {
        players::tell(&player.id, "heard break");
    }
}
horizon_mod_sdk::export_server_mod!(Guest);
