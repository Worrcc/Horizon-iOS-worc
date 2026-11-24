//! API 39 guest checks: shared energy and provider lookup.
use horizon_mod_sdk::server::{
    Args, Player, energy, energy::EnergyPos, lookup, lookup::TargetKind, players,
};

pub fn run(player: &Player, args: &Args) -> Result<Option<String>, String> {
    let words: Vec<_> = args.text().split_whitespace().collect();
    match words.as_slice() {
        ["lookup", mode] => {
            let found = match *mode {
                "register" => {
                    let _ = lookup::register(
                        TargetKind::Block,
                        "minecraft:chest",
                        "horizon:energy",
                        "Chest energy",
                    );
                    lookup::find(TargetKind::Block, "minecraft:chest", "horizon:energy")
                }
                "unregister" => {
                    let _ = lookup::unregister(
                        TargetKind::Block,
                        "minecraft:chest",
                        "horizon:energy",
                    );
                    lookup::find(TargetKind::Block, "minecraft:chest", "horizon:energy")
                }
                _ => return Err("usage: interop39 lookup register|unregister".into()),
            };
            let text = found.map_or_else(
                || "none".into(),
                |(owner, label)| format!("{owner}:{label}"),
            );
            players::tell(&player.id, &format!("lookup {mode} [{text}]"));
        }
        ["energy", mode] => {
            let pos = EnergyPos {
                x: 0,
                y: 64,
                z: 0,
            };
            let before = energy::energy_side("minecraft:overworld", pos, -1);
            let text = match *mode {
                "read" => before.map_or_else(
                    || "none".into(),
                    |s| format!("{}:{}", s.amount, s.capacity),
                ),
                "cap" => {
                    energy::set_energy_capacity("minecraft:overworld", pos, 10)?;
                    "set".into()
                }
                "insert" => format!("{}", energy::insert_energy("minecraft:overworld", pos, -1, 4)?),
                "extract" => format!("{}", energy::extract_energy("minecraft:overworld", pos, -1, 3)?),
                _ => return Err("usage: interop39 energy read|cap|insert|extract".into()),
            };
            players::tell(&player.id, &format!("energy {mode} [{text}]"));
        }
        _ => return Err("usage: interop39 <lookup|energy> <mode>".into()),
    }
    Ok(Some("interop checked".into()))
}
