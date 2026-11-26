//! API 35 client-only demo: `/hud35 on|off`, or stats, food ID, entities R MAX, bars.
use horizon_mod_sdk::{
    Mod, Registration, chat, client_world, game, input,
    render::{self, BarKind, FillShape, Shape},
};
struct Demo {
    active: bool,
}

fn number<T: std::str::FromStr>(word: Option<&str>) -> Result<T, String> {
    word.ok_or("missing number")?
        .parse()
        .map_err(|_| "invalid number".into())
}

fn draw() -> Result<String, String> {
    let stats = game::food_stats()?;
    let held_food = match input::held() {
        Some(held) => game::food_value(&held.id)?,
        None => None,
    };
    let entities = client_world::entities_near(48.0, 48)?;
    let kinds = client_world::entity_kinds()?;
    let bars = render::hud_bars()?;
    let shapes: Vec<Shape> = bars
        .iter()
        .filter_map(|bar| {
            if !matches!(bar.kind, BarKind::Food) {
                return None;
            }
            let stats = stats.as_ref()?;
            let mut rect = bar.bounds;
            rect.h = 2.0;
            rect.y += bar.bounds.h;
            let preview = held_food.as_ref().map_or(0.0, |food| food.saturation);
            rect.w *= ((stats.saturation + preview) / 20.0).clamp(0.0, 1.0);
            Some(Shape::Fill(FillShape {
                rect,
                color: 0xC0FFCC00,
            }))
        })
        .collect();
    render::set_hud(&shapes);
    Ok(format!(
        "entities {} kinds {} food {:?} held {:?} bars {}",
        entities.len(),
        kinds.len(),
        stats,
        held_food,
        bars.len()
    ))
}

impl Mod for Demo {
    fn init() -> Result<(Self, Registration), String> {
        Ok((
            Self { active: false },
            Registration::new().on_tick().command("hud35"),
        ))
    }
    fn tick(&mut self, tick: u64) {
        if self.active && tick.is_multiple_of(5) && draw().is_err() {
            render::set_hud(&[]);
        }
    }
    fn command(&mut self, _name: &str, args: &str) -> Result<Option<String>, String> {
        let mut words = args.split_whitespace();
        let output = match words.next().unwrap_or("show") {
            "on" => {
                self.active = true;
                draw()?
            }
            "off" => {
                self.active = false;
                render::set_hud(&[]);
                "off".into()
            }
            "show" => draw()?,
            "stats" => format!("{:?}", game::food_stats()?),
            "food" => format!(
                "{:?}",
                game::food_value(words.next().ok_or("missing item id")?)?
            ),
            "bars" => format!("{:?}", render::hud_bars()?),
            "kinds" => format!("{:?}", client_world::entity_kinds()?),
            "entities" => {
                let dots =
                    client_world::entities_near(number(words.next())?, number(words.next())?)?;
                let mut categories: Vec<String> = dots
                    .iter()
                    .map(|dot| format!("{:?}", dot.category))
                    .collect();
                categories.sort();
                categories.dedup();
                format!(
                    "entities {} first {} last {} categories {}",
                    dots.len(),
                    dots.first().map_or(0.0, |dot| dot.x),
                    dots.last().map_or(0.0, |dot| dot.x),
                    categories.join(",")
                )
            }
            // Deliberately exceed the host budget; used by the sandbox regression test.
            "budget" => {
                for _ in 0..5 {
                    client_world::entities_near(1.0, 0)?;
                }
                "unexpected budget acceptance".into()
            }
            _ => {
                return Err(
                    "hud35 on|off|show|stats|food ID|bars|entities R MAX|kinds|budget".into(),
                );
            }
        };
        chat::show(&output);
        Ok(None)
    }
}
horizon_mod_sdk::export_mod!(Demo);
