//! Server guest for API 20 queueing and caller-exclusion tests.
use horizon_mod_sdk::server::{
    Args, BlockChangePos, BlockPos, BreakOptions, Player, PlayerPoint, Registration, ServerMod,
    TeleportTarget, channel, players, server_hooks, world,
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
                .command("storage28")
                .command("data29")
                .command("behaviour29")
                .command("schedule29")
                .command("filters29")
                .command("sneak28")
                .command("player_peek28")
                .command("api26_teleport")
                .command("api26_permission")
                .command("api26_spawn"),
        ))
    }
    fn block_neighbour_changed(&mut self, dimension: &str, pos: BlockPos, state: &str, from: &str) {
        players::broadcast(&format!("neighbour {dimension} {} {state} {from}", pos.x));
    }
    fn block_random_tick(&mut self, dimension: &str, pos: BlockPos, state: &str) {
        players::broadcast(&format!("random {dimension} {} {state}", pos.x));
    }
    fn block_scheduled_tick(&mut self, dimension: &str, pos: BlockPos, state: &str) {
        players::broadcast(&format!("scheduled {dimension} {} {state}", pos.x));
    }
    fn item_used_on_block(
        &mut self,
        p: &Player,
        item: &str,
        pos: BlockPos,
        state: &str,
        face: i32,
    ) -> horizon_mod_sdk::server::Decision {
        players::tell(&p.id, &format!("item-on {item} {} {state} {face}", pos.y));
        horizon_mod_sdk::server::Decision::Cancel
    }
    fn item_used(&mut self, p: &Player, item: &str) -> horizon_mod_sdk::server::Decision {
        if item == "test:read_data" {
            players::tell(
                &p.id,
                &format!(
                    "data-use {}",
                    world::player_held_data(&p.id).unwrap().unwrap()
                ),
            );
            return horizon_mod_sdk::server::Decision::Cancel;
        }
        players::tell(&p.id, &format!("item-use {item}"));
        horizon_mod_sdk::server::Decision::Cancel
    }
    fn item_hit_entity(
        &mut self,
        p: &Player,
        item: &str,
        kind: &str,
        id: i32,
    ) -> horizon_mod_sdk::server::Decision {
        players::tell(&p.id, &format!("item-hit {item} {kind} {id}"));
        horizon_mod_sdk::server::Decision::Cancel
    }
    fn item_use_finished(&mut self, p: &Player, item: &str) {
        players::tell(&p.id, &format!("item-finish {item}"));
    }
    fn item_picked_up(&mut self, p: &Player, item: &str) -> horizon_mod_sdk::server::Decision {
        players::tell(&p.id, &format!("pickup {item}"));
        horizon_mod_sdk::server::Decision::Cancel
    }
    fn chat_sent_filter(
        &mut self,
        _: &Player,
        text: &str,
    ) -> horizon_mod_sdk::server::ChatDecision {
        use horizon_mod_sdk::server::ChatDecision as C;
        match text {
            "cancel" => C::Cancel,
            "replace" => C::Replace("replacement".into()),
            "large" => C::Replace("x".repeat(257)),
            "unicode" => C::Replace("é".repeat(256)),
            "illegal" => C::Replace("bad\nline".into()),
            _ => C::Allow,
        }
    }
    fn block_breaking(
        &mut self,
        _: &Player,
        _: BlockPos,
        state: &str,
    ) -> horizon_mod_sdk::server::Decision {
        if state == "test:trap" {
            panic!("broken behaviour guest");
        }
        if state == "test:spin" {
            loop {
                std::hint::spin_loop();
            }
        }
        horizon_mod_sdk::server::Decision::Cancel
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
        if name == "data29" {
            let pos = BlockChangePos { x: 0, y: 64, z: 0 };
            let dim = &player.dimension;
            let item = "minecraft:stick";
            match args.text() {
                "read" => {
                    assert_eq!(
                        world::player_stack_data("Alice", 0)?.as_deref(),
                        Some("own")
                    );
                    assert_eq!(
                        world::player_held_data(&player.id)?.as_deref(),
                        Some("held")
                    );
                    assert_eq!(
                        world::container_stack_data(dim, pos, 0)?.as_deref(),
                        Some("chest")
                    );
                    assert_eq!(world::container_stack_data(dim, pos, 1)?, None);
                    assert_eq!(world::player_stack_data(&player.id, 35)?, None); // only another mod's key
                    assert_eq!(
                        world::player_stacks_with_data("Alice")?[0]
                            .as_ref()
                            .unwrap()
                            .data
                            .as_deref(),
                        Some("own")
                    );
                    assert_eq!(
                        world::container_stacks_with_data(dim, pos)?[0]
                            .as_ref()
                            .unwrap()
                            .data
                            .as_deref(),
                        Some("chest")
                    );
                }
                "write" => {
                    world::set_player_stack_data("Alice", 0, Some("new"))?;
                    world::set_player_held_data(&player.id, None)?;
                    world::set_container_stack_data(dim, pos, 0, Some("new chest"))?;
                    // Deferred writes do not change the snapshot during this hook.
                    assert_eq!(
                        world::player_stack_data(&player.id, 0)?.as_deref(),
                        Some("own")
                    );
                }
                "invalid" => {
                    let big = "é".repeat(513);
                    assert!(world::set_player_stack_data(&player.id, 0, Some(&big)).is_err());
                    assert!(world::set_player_held_data(&player.id, Some(&big)).is_err());
                    assert!(world::set_container_stack_data(dim, pos, 0, Some(&big)).is_err());
                    assert!(world::give_item_with_data(&player.id, item, 1, Some(&big)).is_err());
                    assert!(
                        world::spawn_item_with_data(
                            dim,
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.
                            },
                            item,
                            1,
                            Some(&big)
                        )
                        .is_err()
                    );
                    assert!(world::player_stack_data(&player.id, 36).is_err());
                    assert!(world::set_player_stack_data(&player.id, 36, None).is_err());
                    assert!(world::container_stack_data(dim, pos, 99).is_err());
                    assert!(world::set_container_stack_data(dim, pos, 99, None).is_err());
                    assert!(world::set_container_stack_data(dim, pos, 1, None).is_err());
                    let special = BlockChangePos { x: 1, ..pos };
                    assert!(world::set_container_stack_data(dim, special, 0, None).is_err());
                    let unloaded = BlockChangePos { x: 99, ..pos };
                    assert!(world::container_stack_data(dim, unloaded, 0).is_err());
                    assert!(world::container_stacks_with_data(dim, unloaded).is_err());
                    assert!(world::set_container_stack_data(dim, unloaded, 0, None).is_err());
                    assert!(world::player_stack_data("offline", 0).is_err());
                    assert!(world::player_held_data("offline").is_err());
                    assert!(world::player_stacks_with_data("offline").is_err());
                    assert!(world::set_player_stack_data("offline", 0, None).is_err());
                    assert!(world::set_player_held_data("offline", None).is_err());
                    assert!(world::give_item_with_data("offline", item, 1, None).is_err());
                    assert!(world::give_item_with_data(&player.id, item, 6401, None).is_err());
                    assert!(world::give_item_with_data(&player.id, "bad id", 1, None).is_err());
                    for (d, at, id, n) in [
                        (
                            "missing",
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.,
                            },
                            item,
                            1,
                        ),
                        (
                            dim.as_str(),
                            world::Point {
                                x: f64::NAN,
                                y: 64.,
                                z: 0.,
                            },
                            item,
                            1,
                        ),
                        (
                            dim.as_str(),
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.,
                            },
                            item,
                            65,
                        ),
                        (
                            dim.as_str(),
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.,
                            },
                            "minecraft:air",
                            1,
                        ),
                    ] {
                        assert!(world::spawn_item_with_data(d, at, id, n, None).is_err());
                    }
                }
                "budget" => {
                    // A move consumes the same budget even when no items move.
                    assert_eq!(world::container_to_player(dim, pos, 0, 0, &player.id)?, 0);
                    for _ in 0..63 {
                        world::set_player_held_data(&player.id, Some("x"))?;
                    }
                    assert!(world::set_player_held_data(&player.id, None).is_err());
                    assert!(world::set_player_stack_data(&player.id, 0, None).is_err());
                    assert!(world::set_container_stack_data(dim, pos, 0, None).is_err());
                    assert!(world::give_item_with_data(&player.id, item, 1, None).is_err());
                    assert!(
                        world::spawn_item_with_data(
                            dim,
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.
                            },
                            item,
                            1,
                            None
                        )
                        .is_err()
                    );
                }
                "create" => {
                    world::give_item_with_data("Alice", item, 6400, Some("é"))?;
                    world::give_item_with_data(&player.id, item, 1, None)?;
                    for _ in 0..8 {
                        world::spawn_item(
                            dim,
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.,
                            },
                            item,
                            1,
                        )?;
                        world::spawn_item_with_data(
                            dim,
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.,
                            },
                            item,
                            64,
                            Some("é"),
                        )?;
                    }
                    assert!(
                        world::spawn_item(
                            dim,
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.
                            },
                            item,
                            1
                        )
                        .is_err()
                    );
                    assert!(
                        world::spawn_item_with_data(
                            dim,
                            world::Point {
                                x: 0.,
                                y: 64.,
                                z: 0.
                            },
                            item,
                            1,
                            None
                        )
                        .is_err()
                    );
                }
                _ => return Err("unknown data29 mode".into()),
            }
            return Ok(Some("data checked".into()));
        }
        if name == "filters29" {
            assert!(server_hooks::watch_blocks(&["*".into()]).is_err());
            assert!(server_hooks::watch_items(&["test:fo*".into()]).is_err());
            assert!(server_hooks::watch_blocks(&vec!["test:a".into(); 65]).is_err());
            assert!(server_hooks::watch_items(&vec!["test:a".into(); 65]).is_err());
            return Ok(None);
        }
        if name == "schedule29" {
            let pos = BlockChangePos { x: 0, y: 64, z: 0 };
            let dim = "minecraft:overworld";
            match args.text() {
                "caps" => {
                    for _ in 0..256 {
                        world::schedule_tick(dim, pos, 20)?;
                    }
                    assert!(world::schedule_tick(dim, pos, 20).is_err());
                }
                "invalid" => {
                    for delay in [0, 72001, u32::MAX] {
                        assert!(world::schedule_tick(dim, pos, delay).is_err());
                    }
                    assert!(world::schedule_tick("missing:dimension", pos, 20).is_err());
                    assert!(world::schedule_tick(dim, BlockChangePos { x: 1, ..pos }, 20).is_err());
                    assert!(world::schedule_tick(dim, BlockChangePos { x: 2, ..pos }, 20).is_err());
                    assert!(
                        world::schedule_tick(dim, BlockChangePos { x: i32::MAX, ..pos }, 20)
                            .is_err()
                    );
                }
                delay => world::schedule_tick(dim, pos, delay.parse().map_err(|_| "bad delay")?)?,
            }
            return Ok(None);
        }
        if name == "behaviour29" {
            server_hooks::watch_blocks(&["test:*".into()])?;
            server_hooks::watch_items(&["test:*".into(), "minecraft:ender_pearl".into()])?;
            server_hooks::set_behaviour_hooks(horizon_mod_sdk::server::BehaviourConfig {
                block_breaking: true,
                block_neighbour_changed: true,
                block_random_tick: true,
                block_scheduled_tick: true,
                item_used_on_block: true,
                item_used: true,
                item_hit_entity: true,
                item_use_finished: true,
                item_picked_up: true,
                chat_sent_filter: true,
                ..Default::default()
            });
            return Ok(None);
        }
        if name.starts_with("api26_") {
            let mode = args.text();
            return match name {
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
            };
        }
        if name == "sneak28" {
            server_hooks::set_block_uses(true, true);
            return Ok(None);
        }
        if name == "player_peek28" {
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
