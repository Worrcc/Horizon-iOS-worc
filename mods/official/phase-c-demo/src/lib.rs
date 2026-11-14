use horizon_mod_sdk::server::{
    Args, BlastKind, BlockChangePos, DustColor, ItemFilter, ParticleBurst, Player, PlayerPoint,
    Registration, ServerMod, WorldPoint, players, world,
};

struct Demo;

fn point(x: f64, y: f64, z: f64) -> WorldPoint {
    WorldPoint { x, y, z }
}
fn number<T: std::str::FromStr>(word: Option<&str>, what: &str) -> Result<T, String> {
    word.ok_or_else(|| format!("missing {what}"))?
        .parse()
        .map_err(|_| format!("{what} is not a number"))
}
fn block_pos(words: &mut std::str::SplitWhitespace<'_>) -> Result<BlockChangePos, String> {
    Ok(BlockChangePos {
        x: number(words.next(), "x")?,
        y: number(words.next(), "y")?,
        z: number(words.next(), "z")?,
    })
}
fn name_of(id: &str) -> String {
    players::online()
        .into_iter()
        .find(|p| p.id == id)
        .map_or_else(|| id.to_owned(), |p| p.name)
}
fn done(result: Result<(), String>, text: &str) -> Result<Option<String>, String> {
    result.map(|()| Some(text.to_owned()))
}

impl ServerMod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        Ok((Self, Registration::new().on_tick().command("pcd")))
    }
    fn tick(&mut self, _tick: u64) {
        for death in players::deaths() {
            let cause = death.cause.as_deref().unwrap_or("combat");
            players::broadcast(&format!("{} died ({cause})", name_of(&death.player)));
        }
        for respawn in players::respawns() {
            let PlayerPoint { x, y, z } = respawn.at;
            let _ = players::play_sound_to(
                &respawn.player,
                "minecraft:entity.player.levelup",
                1.0,
                1.0,
            );
            let _ = world::spawn_particles(
                &respawn.dimension,
                &ParticleBurst {
                    name: "minecraft:happy_villager".into(),
                    at: point(x, y + 1.0, z),
                    delta: point(0.5, 1.0, 0.5),
                    speed: 0.0,
                    count: 30,
                    block: None,
                    dust: None,
                },
            );
            players::tell(&respawn.player, "welcome back");
        }
    }
    fn command(&mut self, p: &Player, _name: &str, args: &Args) -> Result<Option<String>, String> {
        let mut words = args.text().split_whitespace();
        let id = p.id.as_str();
        let dim = p.dimension.as_str();
        match words.next().unwrap_or("") {
            "vitals" => {
                let vitals = players::vitals(id).ok_or("no vitals")?;
                let effects: Vec<String> = players::effects(id)
                    .unwrap_or_default()
                    .iter()
                    .map(|e| format!("{} x{} for {} ticks", e.id, e.amplifier + 1, e.ticks))
                    .collect();
                Ok(Some(format!(
                    "food {} (sat {:.1}), level {} ({:.0}%, {} total), effects: {}",
                    vitals.food,
                    vitals.saturation,
                    vitals.xp_level,
                    vitals.xp_progress * 100.0,
                    vitals.xp_total,
                    if effects.is_empty() {
                        "none".to_owned()
                    } else {
                        effects.join(", ")
                    }
                )))
            }
            "health" => done(
                players::set_health(id, number(words.next(), "health")?),
                "health queued",
            ),
            "hurt" => done(
                players::hurt(id, number(words.next(), "amount")?, "minecraft:generic"),
                "damage queued",
            ),
            "feed" => done(players::set_food(id, 20, 5.0), "fed"),
            "xp" => done(
                players::give_experience(id, number(words.next(), "points")?),
                "experience queued",
            ),
            "levels" => done(
                players::give_levels(id, number(words.next(), "levels")?),
                "levels queued",
            ),
            "cleareffects" => done(players::clear_effects(id), "effects cleared"),
            "clear" => {
                let target = words.next().ok_or("clear <id or #tag> [max]")?;
                let max = match words.next() {
                    Some(word) => number(Some(word), "max")?,
                    None => u32::MAX,
                };
                let filter = match target.strip_prefix('#') {
                    Some(tag) => ItemFilter::Tag(tag.to_owned()),
                    None => ItemFilter::Item(target.to_owned()),
                };
                let removed = players::clear_items(id, &filter, max)?;
                Ok(Some(format!("removed {removed}")))
            }
            "boom" => done(
                world::explode(
                    dim,
                    point(p.x + 5.0, p.y, p.z),
                    3.0,
                    false,
                    BlastKind::Block,
                ),
                "boom queued",
            ),
            "fall" => {
                let pos = block_pos(&mut words)?;
                let state = world::get_block(dim, pos.x, pos.y, pos.z).ok_or("not loaded")?;
                done(
                    world::make_block_fall(dim, pos, &state, point(0.0, 0.4, 0.0), false),
                    "falling block queued",
                )
            }
            "sparkle" => {
                let at = point(p.x, p.y + 1.0, p.z);
                let burst = |name: &str| ParticleBurst {
                    name: name.into(),
                    at: at.clone(),
                    delta: point(0.5, 0.5, 0.5),
                    speed: 0.02,
                    count: 20,
                    block: None,
                    dust: None,
                };
                world::spawn_particles(dim, &burst("minecraft:flame"))?;
                world::spawn_particles(
                    dim,
                    &ParticleBurst {
                        dust: Some(DustColor {
                            rgb: 0xFF4000,
                            scale: 1.5,
                        }),
                        ..burst("minecraft:dust")
                    },
                )?;
                world::spawn_particles(
                    dim,
                    &ParticleBurst {
                        block: Some("minecraft:stone".into()),
                        ..burst("minecraft:block")
                    },
                )?;
                done(
                    players::play_sound_to(id, "minecraft:entity.player.levelup", 1.0, 1.0),
                    "sparkles queued",
                )
            }
            "flow" => {
                let pos = block_pos(&mut words)?;
                Ok(Some(match world::fluid_flow(dim, pos) {
                    Some(f) => format!("flow {:.3} {:.3} {:.3}", f.x, f.y, f.z),
                    None => "flow unknown (unloaded chunk or dimension)".to_owned(),
                }))
            }
            _ => Err(
                "pcd vitals|health|hurt|feed|xp|levels|cleareffects|clear|boom|fall|sparkle|flow"
                    .into(),
            ),
        }
    }
}

horizon_mod_sdk::export_server_mod!(Demo);
