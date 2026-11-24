//! API 40 guest checks: opt-in server lifecycle callbacks.
use horizon_mod_sdk::server::{Args, Player, LifecycleConfig, players, server_hooks};

pub fn run(_player: &Player, args: &Args) -> Result<Option<String>, String> {
    match args.text().trim() {
        "on" => server_hooks::set_lifecycle_hooks(LifecycleConfig {
            server_tick: true,
            player_respawned: true,
            save: true,
        }),
        "off" => server_hooks::set_lifecycle_hooks(LifecycleConfig {
            server_tick: false,
            player_respawned: false,
            save: false,
        }),
        other => return Err(format!("usage: lifecycle40 on|off ({other})")),
    }
    Ok(Some("lifecycle checked".into()))
}

pub fn tick_start(tick: u64) {
    players::broadcast(&format!("tick start {tick}"));
}
pub fn tick_end(tick: u64) {
    players::broadcast(&format!("tick end {tick}"));
}
pub fn respawned(player: &Player, dimension: &str) {
    players::tell(&player.id, &format!("respawned {dimension}"));
}
pub fn before_save() {
    players::broadcast("before save");
}
pub fn after_save(saved: bool) {
    players::broadcast(&format!("after save {saved}"));
}
