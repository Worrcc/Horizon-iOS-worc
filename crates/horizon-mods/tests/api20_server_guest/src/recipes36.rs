//! API 36 guest checks: paged recipe queries from the server.
use horizon_mod_sdk::server::{Args, Player, players, world};

pub fn run(player: &Player, args: &Args) -> Result<Option<String>, String> {
    let text = args.text().trim();
    let (item, start, count) = match text.split_whitespace().collect::<Vec<_>>()[..] {
        [item] => (item, 0, 128),
        [item, start, count] => (
            item,
            start.parse().map_err(|_| "bad start")?,
            count.parse().map_err(|_| "bad count")?,
        ),
        _ => return Err("usage: recipes36 <item> [start count]".into()),
    };
    let recipes = world::recipes_for_item(item, start, count)?;
    let ids = recipes
        .iter()
        .map(|r| format!("{}:{}:{}", r.id, r.result, r.result_count))
        .collect::<Vec<_>>()
        .join(",");
    players::tell(&player.id, &format!("recipes {item} [{ids}]"));
    Ok(Some("recipes checked".into()))
}
