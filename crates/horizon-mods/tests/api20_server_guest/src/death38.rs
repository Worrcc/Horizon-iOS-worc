//! API 38 guest checks: claiming a player's death drops.
use horizon_mod_sdk::server::{
    Args, DeathPoint, DeathStack, Player, players,
    players::{ClaimedDeath, ClaimedStack},
    server_hooks,
};
use std::sync::atomic::{AtomicBool, Ordering};

static CLAIM: AtomicBool = AtomicBool::new(false);

pub fn run(player: &Player, args: &Args) -> Result<Option<String>, String> {
    match args.text().trim() {
        "on" => server_hooks::set_death_drops(true),
        "off" => server_hooks::set_death_drops(false),
        "claim" => CLAIM.store(true, Ordering::Relaxed),
        "skip" => CLAIM.store(false, Ordering::Relaxed),
        "read" => {
            let deaths = players::claimed_deaths();
            let summary = deaths
                .iter()
                .map(|d: &ClaimedDeath| {
                    format!(
                        "{}:{}:{}",
                        d.player,
                        d.dimension,
                        d.drops
                            .iter()
                            .map(|s: &ClaimedStack| s.item.clone())
                            .collect::<Vec<_>>()
                            .join("+")
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            players::tell(&player.id, &format!("claimed [{summary}]"));
        }
        other => return Err(format!("usage: death38 on|off|claim|skip|read ({other})")),
    }
    Ok(Some("death checked".into()))
}

pub fn died(player: &Player, dimension: &str, _at: DeathPoint, drops: &[DeathStack]) -> bool {
    let items = drops
        .iter()
        .map(|s| format!("{}x{}", s.item, s.count))
        .collect::<Vec<_>>()
        .join(",");
    players::tell(&player.id, &format!("died {dimension} {items}"));
    CLAIM.load(Ordering::Relaxed)
}
