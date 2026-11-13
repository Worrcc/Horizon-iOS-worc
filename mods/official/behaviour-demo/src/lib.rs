use horizon_mod_sdk::server::{
    BehaviourConfig, BlockChange, BlockChangePos, BlockPos, Decision, Player, Registration,
    ServerMod, players, server_hooks, world,
};
struct Demo;
impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        server_hooks::set_behaviour_hooks(BehaviourConfig {
            block_breaking: true,
            block_placing: true,
            block_interacted: true,
            block_scheduled_tick: true,
            item_used: true,
            chat_sent_filter: true,
            ..Default::default()
        });
        server_hooks::watch_blocks(&[
            "minecraft:obsidian".into(),
            "minecraft:tnt".into(),
            "minecraft:lectern".into(),
            "behaviour_demo:*".into(),
        ])?;
        server_hooks::watch_items(&["minecraft:ender_pearl".into()])?;
        Ok((Self, Registration::new().command("stickdata")))
    }
    fn block_interacted(
        &mut self,
        player: &Player,
        pos: BlockPos,
        state: &str,
        face: i32,
        x: f64,
        y: f64,
        z: f64,
        hand: horizon_mod_sdk::server::BehaviourHand,
        item: &str,
        sneaking: bool,
    ) -> Decision {
        players::tell(
            &player.id,
            &format!(
                "behaviour:interacted {} {} {} {state} {face} {x} {y} {z} {hand:?} {item} {sneaking}",
                pos.x, pos.y, pos.z
            ),
        );
        if state.split('[').next() == Some("minecraft:lectern") {
            if item == "minecraft:stick" && hand == horizon_mod_sdk::server::BehaviourHand::Main {
                let result = (|| -> Result<(), String> {
                    let next = world::player_held_data(&player.id)?
                        .and_then(|s| s.parse::<u64>().ok())
                        .unwrap_or(0)
                        .saturating_add(1)
                        .to_string();
                    world::set_player_held_data(&player.id, Some(&next))?;
                    horizon_mod_sdk::log::write(
                        horizon_mod_sdk::log::Level::Info,
                        &format!("behaviour-demo: queued stick counter {next}"),
                    );
                    Ok(())
                })();
                if let Err(error) = result {
                    horizon_mod_sdk::log::write(horizon_mod_sdk::log::Level::Warn, &error);
                }
            }

            let _ = world::schedule_tick(
                &player.dimension,
                BlockChangePos {
                    x: pos.x,
                    y: pos.y,
                    z: pos.z,
                },
                20,
            );
        }
        Decision::Cancel
    }
    fn command(
        &mut self,
        player: &Player,
        _name: &str,
        _args: &horizon_mod_sdk::server::Args,
    ) -> Result<Option<String>, String> {
        Ok(Some(format!(
            "stickdata: {}",
            world::player_held_data(&player.id)?
                .as_deref()
                .unwrap_or("none")
        )))
    }
    fn block_scheduled_tick(&mut self, dimension: &str, pos: BlockPos, _state: &str) {
        let _ = world::set_blocks(
            dimension,
            &[BlockChange {
                x: pos.x,
                y: pos.y + 1,
                z: pos.z,
                state: "minecraft:gold_block".into(),
            }],
        );
        horizon_mod_sdk::log::write(
            horizon_mod_sdk::log::Level::Info,
            "behaviour-demo: scheduled lectern tick placed gold above it",
        );
    }
    fn chat_sent_filter(
        &mut self,
        _player: &Player,
        text: &str,
    ) -> horizon_mod_sdk::server::ChatDecision {
        let replacement = text
            .split_inclusive(|c: char| !c.is_alphanumeric() && c != '_')
            .map(|part| {
                if part == "heck" {
                    "hmm".into()
                } else if let Some(tail) = part.strip_prefix("heck")
                    && tail
                        .chars()
                        .next()
                        .is_some_and(|c| !c.is_alphanumeric() && c != '_')
                {
                    format!("hmm{tail}")
                } else {
                    part.to_owned()
                }
            })
            .collect::<String>();
        if replacement != text {
            horizon_mod_sdk::server::ChatDecision::Replace(replacement)
        } else {
            horizon_mod_sdk::server::ChatDecision::Allow
        }
    }
    fn item_used(&mut self, player: &Player, _item: &str) -> Decision {
        players::tell(&player.id, "behaviour:item-used");
        Decision::Cancel
    }
    fn block_breaking(&mut self, player: &Player, _pos: BlockPos, state: &str) -> Decision {
        players::tell(&player.id, "behaviour:breaking");
        if state.split('[').next() == Some("minecraft:obsidian") {
            Decision::Cancel
        } else {
            Decision::Allow
        }
    }
    fn block_placing(
        &mut self,
        player: &Player,
        _pos: BlockPos,
        state: &str,
        _item: &str,
    ) -> Decision {
        players::tell(&player.id, "behaviour:placing");
        if state.split('[').next() == Some("minecraft:tnt") {
            Decision::Cancel
        } else {
            Decision::Allow
        }
    }
}
horizon_mod_sdk::export_server_mod!(Demo);
