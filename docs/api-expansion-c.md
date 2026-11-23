# Phase C part 1: API level 31 players and world effects

API level **31** is part 1 of Phase C. It adds sixteen imports (twelve in
`players`, four in `world`) and no export. No existing WIT record, variant,
function or callback changes; levels 26-30 retain their contracts. Server
manifests use capability `players` or `world` with the matching `experimental`
entry (both are experimental). A mod that does not import a function pays
nothing, and a missing import stays loadable on an older Horizon (it is refused
with "update Horizon"). Scoreboards, teams, boss bars, the sidebar and tab list,
advancement triggers, tags/registries and loot come in part 2.

## Player events

`players.deaths()` and `players.respawns()` poll last tick's events, oldest
first, at most **256** each; reading does not consume them. A `player-death`
carries the player UUID, the dimension, the feet position and `cause`, the death
message's translation key (`death.attack.lava`; none when built from combat).
A `player-respawn` carries the place `PlayerList.respawn` put the player.
Heard are the server's players and, in single player, the window player through
the server thread. There are no killer fields.

## Reads

`players.vitals(player)` (food, saturation, experience level, progress, total)
and `players.effects(player)` (id, remaining ticks, amplifier; at most 64) are
the state at the start of the tick, none for an offline player. The snapshot is
filled only while a running mod has capability `players`. Health is in `player`.

## Setters

`set-health` (0 < h <= 1024, capped at max health; **never kills or revives**),
`hurt` (0 < amount <= 1024, a registered damage type id; armour applies, it can
kill, creative players are immune; an unknown type is logged and ignored),
`set-food` (food 0..20, saturation 0..food), `give-experience` (0..100000
points, as `Player.giveExperiencePoints`, no Mending), `give-levels`
(-1000..1000, never below level 0), `clear-effects` (the effects active in the
snapshot at call time, by id) and `play-sound-to`. All share **64 changes per
mod per tick**, are checked now and applied after the hooks; a player offline
at apply time means nothing happens. Zero points or levels succeed without a
change.

## clear-items

`clear-items(player, filter, max)` is synchronous and returns the count removed.
The filter is an item id, an item tag (without `#`) or the calling mod's custom
stack data (`some(v)` only that value, `none` any). It searches the main
inventory, armour and off hand, selected slot first, as `take-item` does. With
`max` 0 it only counts. It destroys items, creative players included; it
creates nothing, and stacks it keeps (or partly keeps) retain their components.
At most **64 calls per mod per tick**. The cursor and crafting grid are not
searched.

## play-sound-to

A private sound at the player's position, volume 0..4, pitch 0.5..2, registered
sound event names only.

## World effects

- `spawn-particles`: vanilla particle types without options; `minecraft:dust`
  with `dust` (rgb, scale 0.01..4); `block`, `block_marker`, `falling_dust`,
  `dust_pillar` and `block_crumble` with a block state. Count 0..256 (zero sends
  one moving particle), spread each axis <= 64, speed 0..16. **64 calls and 4096
  particles per mod per tick**; a call over the particle budget is an error and
  keeps its call charge. Sent to players near the position.
- `explode`: `Level.explode` with no source entity, 0 < radius <= 8, `keep`,
  `block` or `tnt` interaction, **4 per mod per tick**. With `mob_griefing`
  off blocks are kept and fire is not lit.
- `make-block-fall`: turns an existing block into a falling block. `expected-state`
  must equal `get-block` at call and again at apply (a preimage); air, fluids,
  waterlogged blocks, `minecraft:wither_immune` blocks and blocks with a block
  entity are refused. Velocity each axis <= 3.9. The falling block is spawned
  first and the original removed only on success, so no block is created or
  lost. **16 per mod per tick**; the level's 1024 falling-block cap still holds.
- `fluid-flow`: `FluidState.getFlow`, the normalised current; zero where there is
  no fluid, none for an unloaded chunk or unknown dimension.

## Not included

Cancellable death and damage (the hooks are hostless), killer attribution,
death-inventory loot, explosion sources, mod particle types and item particles.
Scoreboards, teams, boss bars, sidebar/tab list, advancement triggers,
tags/registries and loot are part 2.

## Demo and validation

`mods/official/phase-c-demo` exercises everything through `/pcd`. Build without
network:

```sh
wt=/path/to/storage-api
export CARGO_TARGET_DIR="$wt/target" CARGO_NET_OFFLINE=true
cd "$wt"
cargo build --offline --release -p cargo-horizon-mod
cd "$wt/mods/official/phase-c-demo"
"$wt/target/release/cargo-horizon-mod" build
"$wt/target/release/cargo-horizon-mod" check "$wt/target/horizon-mod/phase_c_demo.hmod"
```

Manual live steps (not run by the tests): start a game with feature `mods`, as
in [Phase B](api-expansion-b.md), loading the artifact as `--dev-mod`. Die and
respawn: the death is announced, and on respawn you hear a level-up and see
villager particles. `/pcd vitals`, `/pcd health 5`, `/pcd hurt 30` (can kill),
`/pcd feed`, `/pcd xp 100`, `/pcd levels 5`, `/pcd cleareffects`; `/pcd clear
minecraft:stone 10` then `/pcd clear #minecraft:logs`; `/pcd boom` with
`/gamerule mob_griefing false` and true; `/pcd fall x y z` on sand and on a chest
(refused); `/pcd sparkle`; `/pcd flow x y z` beside flowing water. Check from a
second player as well as the host.
