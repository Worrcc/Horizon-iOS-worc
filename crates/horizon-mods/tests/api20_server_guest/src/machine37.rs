//! API 37 guest checks: component-preserving, preimage-checked machine writes.
use horizon_mod_sdk::server::{Args, BlockChangePos, Player, players, world};

fn pos() -> BlockChangePos {
    BlockChangePos { x: 0, y: 64, z: 0 }
}

pub fn run(player: &Player, args: &Args) -> Result<Option<String>, String> {
    let mode = args.text().trim();
    let dim = &player.dimension;
    let slots = world::machine_stacks(dim, pos()).ok_or("unloaded")?;
    match mode {
        "read" => {
            let summary = slots
                .iter()
                .map(|slot| match slot {
                    None => "_".into(),
                    Some(s) => format!(
                        "{}x{}{}",
                        s.item,
                        s.count,
                        if s.components.is_some() { "+" } else { "" }
                    ),
                })
                .collect::<Vec<_>>()
                .join(",");
            players::tell(&player.id, &format!("machine [{summary}]"));
        }
        "write" | "bad" => {
            let expect = slots.first().cloned().flatten();
            let mut write = expect.clone();
            if let Some(stack) = write.as_mut() {
                stack.count += 1;
            }
            let mut checked = expect;
            if mode == "bad"
                && let Some(stack) = checked.as_mut()
            {
                stack.count += 100;
            }
            let changes = vec![world::MachineSlotChange {
                slot: 0,
                expect: checked,
                write,
            }];
            world::set_machine_stacks(dim, pos(), &changes)?;
            players::tell(&player.id, "machine queued");
        }
        other => return Err(format!("usage: machine37 read|write|bad ({other})")),
    }
    Ok(Some("machine checked".into()))
}
