//! API 43 guest checks: register deterministic features and query generated chunks.
use horizon_mod_sdk::server::{Args, Player, players, world, worldgen};

pub fn run(player: &Player, args: &Args) -> Result<Option<String>, String> {
    match args.text().trim() {
        "register" => worldgen::register_feature("test:ruin", "test:temple", 4, 7)?,
        "unregister" => worldgen::unregister_feature("test:ruin")?,
        "query" => {
            let rejected = worldgen::site_for_chunk("minecraft:overworld", 3, -2, "test:ruin");
            let site = worldgen::site_for_chunk("minecraft:overworld", 2, -4, "test:ruin");
            let text = site.map_or_else(
                || "none".into(),
                |s| format!("{}:{}:{}:{}", s.x, s.z, s.rotation, s.mirror),
            );
            let rejected = rejected.map_or_else(|| String::from("none"), |_| String::from("site"));
            players::tell(
                &player.id,
                &format!("worldgen [{text}] nonselected [{rejected}]"),
            );
            players::tell(&player.id, &format!("seed {}", world::world_seed()));
        }
        other => {
            return Err(format!(
                "usage: worldgen43 register|unregister|query ({other})"
            ));
        }
    }
    Ok(Some("worldgen checked".into()))
}
