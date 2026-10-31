//! Example Horizon server mod (`docs/mods.md`, "Server mods") with capability `entities`:
//! orders to the content mods' mobs near the player who types them. `/heel` walks them to
//! the player, `/halt` lets their own goals back, `/animate <name>` plays one of the nearest
//! one's own animations, `/breathe [amount]` turns the nearest one to the player and hurts
//! and burns what is around the player, `/hover` lifts it with gravity off, and `/land`
//! lets gravity back. `/raise <kind>` spawns one beside the player, `/grow <scale>` sizes the
//! nearest, `/attack` sets it on the nearest player, `/label <key> <value>` stores a string on it
//! (kept in its save) and `/info` says what it is. Right clicks tell the player what they
//! held; a bone stores their UUID as the mob's `owner`. `/paint <texture|none>` changes
//! its texture, `/roar` sends sound and flame particles, `/attr <name> <value>` changes
//! base attributes, `/heal <n>` heals, and `/info` reports mobGriefing, time and sky visibility.
use horizon_mod_sdk::server::{
    Area, Args, Argument, AttributeChange, Entity, Order, Particles, Player, Point, Registration,
    ServerMod, Sound, Target, Walk, entities, players, world,
};

/// How far the orders reach, in blocks.
const REACH: f64 = 32.0;

struct MobOrders;

fn at(player: &Player) -> Point {
    Point {
        x: player.x,
        y: player.y,
        z: player.z,
    }
}

/// The content mods' mobs within reach of `player`, nearest first.
fn near(player: &Player) -> Vec<Entity> {
    let own = |e: &Entity| !e.kind.starts_with("minecraft:");
    let mut near = entities::near(&player.dimension, at(player), REACH);
    near.retain(own);
    near
}

fn nearest(player: &Player) -> Result<Entity, String> {
    near(player)
        .into_iter()
        .next()
        .ok_or_else(|| "No content mod's mob within 32 blocks".to_string())
}

impl ServerMod for MobOrders {
    fn init() -> Result<(Self, Registration), String> {
        Ok((
            MobOrders,
            Registration::new()
                .on_tick()
                .command("heel")
                .command("halt")
                .command_with("animate", [Argument::word("name")])
                .command_with(
                    "breathe",
                    [Argument::integer("amount", Some(0), Some(100)).optional()],
                )
                .command("hover")
                .command("land")
                .command_with("raise", [Argument::text("kind")])
                .command_with("grow", [Argument::number("scale")])
                .command("attack")
                .command_with("label", [Argument::word("key"), Argument::word("value")])
                .command_with("paint", [Argument::text("texture")])
                .command("roar")
                .command_with("attr", [Argument::word("name"), Argument::number("value")])
                .command_with("heal", [Argument::number("amount")])
                .command("info"),
        ))
    }

    fn tick(&mut self, _tick: u64) {
        for click in entities::interactions() {
            let item = click.item.as_deref().unwrap_or("nothing");
            players::tell(
                &click.player,
                &format!("{} #{} noticed you (holding {item})", click.kind, click.mob),
            );
            if item == "minecraft:bone" {
                let _ = entities::give_orders(&[(
                    click.mob,
                    Order::SetData(vec![("owner".into(), Some(click.player))]),
                )]);
            }
        }
    }

    fn command(
        &mut self,
        player: &Player,
        name: &str,
        args: &Args,
    ) -> Result<Option<String>, String> {
        match name {
            "heel" | "halt" => {
                let mobs = near(player);
                let order = |_: &Entity| match name {
                    "heel" => Order::WalkTo(Walk {
                        to: at(player),
                        speed: 1.2,
                    }),
                    _ => Order::Halt,
                };
                let orders: Vec<(u32, Order)> = mobs.iter().map(|e| (e.id, order(e))).collect();
                entities::give_orders(&orders)?;
                Ok(Some(format!("{} mobs ordered", orders.len())))
            }
            "animate" => {
                let mob = nearest(player)?;
                let animation = args.string("name").unwrap_or_default().to_string();
                entities::give_orders(&[(mob.id, Order::Animate(Some(animation)))])?;
                Ok(Some(format!("{} plays it", mob.kind)))
            }
            "breathe" => {
                let mob = nearest(player)?;
                let amount = args.integer("amount").unwrap_or(4) as f32;
                let area = Area {
                    center: at(player),
                    radius: 2.0,
                    amount,
                    fire: 3,
                };
                entities::give_orders(&[
                    (mob.id, Order::LookAt(at(player))),
                    (mob.id, Order::HurtArea(area)),
                ])?;
                Ok(Some(format!("{} breathes", mob.kind)))
            }
            "hover" | "land" => {
                let mob = nearest(player)?;
                let orders = if name == "hover" {
                    vec![
                        (mob.id, Order::Gravity(false)),
                        (
                            mob.id,
                            Order::Push(Point {
                                x: 0.0,
                                y: 0.4,
                                z: 0.0,
                            }),
                        ),
                    ]
                } else {
                    vec![(mob.id, Order::Gravity(true))]
                };
                entities::give_orders(&orders)?;
                Ok(None)
            }
            "raise" => {
                let kind = args.string("kind").unwrap_or_default().to_string();
                let at = Point {
                    x: player.x + 3.0,
                    ..at(player)
                };
                let by = vec![("raised_by".to_string(), player.name.clone())];
                entities::spawn(&kind, &player.dimension, at, 0.0, &by)?;
                Ok(Some(format!("{kind} raised")))
            }
            "grow" => {
                let mob = nearest(player)?;
                let scale = args.number("scale").unwrap_or(1.0) as f32;
                entities::give_orders(&[(mob.id, Order::Scale(scale))])?;
                Ok(Some(format!("{} grows to {scale}", mob.kind)))
            }
            "attack" => {
                let mob = nearest(player)?;
                entities::give_orders(&[(mob.id, Order::Target(Target::NearestPlayer))])?;
                Ok(Some(format!("{} attacks", mob.kind)))
            }
            "label" => {
                let mob = nearest(player)?;
                let key = args.string("key").unwrap_or_default().to_string();
                let value = args.string("value").map(str::to_string);
                entities::give_orders(&[(mob.id, Order::SetData(vec![(key, value)]))])?;
                Ok(None)
            }
            "roar" => {
                let mob = nearest(player)?;
                let at = Point {
                    x: mob.x,
                    y: mob.y + 1.0,
                    z: mob.z,
                };
                entities::give_orders(&[
                    (
                        mob.id,
                        Order::Sound(Sound {
                            name: "minecraft:entity.ender_dragon.growl".into(),
                            volume: 1.0,
                            pitch: 1.0,
                        }),
                    ),
                    (
                        mob.id,
                        Order::Particles(Particles {
                            name: "minecraft:flame".into(),
                            at,
                            delta: Point {
                                x: 0.5,
                                y: 0.5,
                                z: 0.5,
                            },
                            speed: 0.05,
                            count: 32,
                        }),
                    ),
                ])?;
                Ok(Some(format!("{} roars", mob.kind)))
            }
            "paint" => {
                let mob = nearest(player)?;
                let texture = args.string("texture").unwrap_or_default();
                let texture = (texture != "none").then(|| texture.to_string());
                entities::give_orders(&[(mob.id, Order::Texture(texture))])?;
                Ok(Some(format!("{} painted", mob.kind)))
            }
            "attr" => {
                let mob = nearest(player)?;
                let name = args.string("name").unwrap_or_default().to_owned();
                let value = args.number("value").unwrap_or_default();
                entities::give_orders(&[(
                    mob.id,
                    Order::Attribute(AttributeChange { name, value }),
                )])?;
                Ok(None)
            }
            "heal" => {
                let mob = nearest(player)?;
                let amount = args.number("amount").unwrap_or_default() as f32;
                entities::give_orders(&[(mob.id, Order::Heal(amount))])?;
                Ok(None)
            }
            "info" => {
                let mob = nearest(player)?;
                let time = world::time(&mob.dimension);
                let sky = world::can_see_sky(
                    &mob.dimension,
                    mob.x.floor() as i32,
                    mob.y.floor() as i32,
                    mob.z.floor() as i32,
                );
                let data: Vec<String> = mob.data.iter().map(|(k, v)| format!("{k}={v}")).collect();
                Ok(Some(format!(
                    "{} #{}: scale {}, health {}/{}, data [{}], mobGriefing {}, time {}/{}, day {}, rain {}, thunder {}, sky {}",
                    mob.kind,
                    mob.id,
                    mob.scale,
                    mob.health,
                    mob.max_health,
                    data.join(", "),
                    world::game_rule("mobGriefing").unwrap_or_else(|| "unknown".into()),
                    time.day_time,
                    time.game_time,
                    time.is_day,
                    time.raining,
                    time.thundering,
                    sky
                )))
            }
            _ => Err("Unknown command".into()),
        }
    }
}

horizon_mod_sdk::export_server_mod!(MobOrders);
