//! API 41 guest checks: watched machines and persistent attachments.
use horizon_mod_sdk::server::{
    Args, BlockRef, Player, attachments, attachments::AttachmentTarget, players, server_hooks,
};

pub fn run(player: &Player, args: &Args) -> Result<Option<String>, String> {
    let words: Vec<_> = args.text().split_whitespace().collect();
    match words.as_slice() {
        ["watch"] => server_hooks::watch_machines(&["test:machine".into()])?,
        ["attach", mode] => {
            let target = AttachmentTarget::Block(BlockRef {
                dimension: "minecraft:overworld".into(),
                x: 0,
                y: 64,
                z: 0,
            });
            match *mode {
                "set" => attachments::set_attachment(&target, "key", Some(&[1, 2, 3]))?,
                "get" => {
                    let value = attachments::attachment(&target, "key").unwrap_or_default();
                    players::tell(&player.id, &format!("attach get [{value:?}]"));
                }
                "clear" => attachments::set_attachment(&target, "key", None)?,
                other => return Err(format!("usage: attach set|get|clear ({other})")),
            }
        }
        _ => return Err("usage: interop41 watch|attach <mode>".into()),
    }
    Ok(Some("interop41 checked".into()))
}
