//! API 36: the world's seed and the top block of a column, through the real imports.
use horizon_mod_sdk::{
    log,
    server::{Player, surface_block, world},
};

pub fn run(player: &Player, args: &str) -> Result<Option<String>, String> {
    let dim = player.dimension.as_str();
    let mut words = args.split_whitespace();
    match words.next() {
        Some("seed") => Ok(Some(format!("seed {}", world::seed()))),
        Some("top") => {
            let mut n = || -> Result<i32, String> {
                words
                    .next()
                    .and_then(|w| w.parse().ok())
                    .ok_or_else(|| "top x z below".to_string())
            };
            let (x, z, below) = (n()?, n()?, n()?);
            Ok(Some(match world::top_block(dim, x, z, below) {
                Some(y) => format!("top {y}"),
                None => "top none".into(),
            }))
        }
        Some("surface") => Ok(Some(format!("{:?}", surface_block(dim, 0, 0)))),
        Some("unknown") => Ok(Some(format!(
            "{:?}",
            world::top_block("test:missing", 0, 0, 100)
        ))),
        Some("log") => {
            log::write(log::Level::Info, "w36 info line");
            log::write(log::Level::Warn, "w36 warn line");
            log::write(log::Level::Debug, "w36 debug line");
            Ok(Some("logged".into()))
        }
        _ => Err("w36 seed|top x z below|surface|unknown|log".into()),
    }
}
