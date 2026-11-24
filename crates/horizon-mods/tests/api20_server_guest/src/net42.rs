//! API 42 guest checks: cross-mod net messages.
use horizon_mod_sdk::server::{Args, Player, net};

pub fn run(_player: &Player, args: &Args) -> Result<Option<String>, String> {
    match args.text().trim() {
        "register" => net::register("demo")?,
        "send" => net::send_to_all("demo", &[1, 2, 3])?,
        other => return Err(format!("usage: net42 register|send ({other})")),
    }
    Ok(Some("net42 checked".into()))
}
