# Phase E: API level 35 client observations (entities, food, HUD bars)

API level **35** adds five imports to existing interfaces: `client-world.entities-near`,
`client-world.entity-kinds`, `game.food-stats`, `game.food-value`, and `render.hud-bars`.
Client mods only, no exports or changes to earlier records, functions or callbacks. Existing
capabilities grant them; a mod importing these functions needs Horizon level 35 or newer.
The SDK exports the generated `Result` wrappers through `client_world`, `game`, and `render`.

## Nearby entities

Capability `world-read` (`experimental = ["world-read"]`):

`entities-near(radius: f32, max: u32) -> result<list<entity-dot>, string>` reads loaded living
entities in a **3D sphere** around the local player's feet, nearest first, excluding the local
player and dead/removed entities. It works on any server with only client-loaded data and sends
no traffic. The adapter includes ordinary mobs, content mobs, other players and the dragon.

| Field / input | Meaning | Checks / limits |
|---|---|---|
| `radius` | blocks from the player's feet | finite, 0..64 inclusive; negative, NaN, infinity and larger values are errors |
| `max` | most entries returned | 0..256 inclusive; larger is an error, zero returns an empty list |
| `x`, `y`, `z` | entity feet, world coordinates | f64, preserving precision far from spawn |
| `yaw` | entity yaw in degrees | f32 |
| `kind` | index into `entity-kinds()` | stable for the current loaded resources |
| `category` | `hostile`, `passive`, `neutral`, `water`, `ambient`, `player`, `own-tamed`, `other-tamed`, `other` | ownership takes precedence over species category; player takes precedence over ownership |

`entity-kinds() -> result<list<string>, string>` returns sorted namespaced entity type ids.
Read once and cache; refresh after mods/resources reload (`table-version()` changes on a resource reload). Empty outside a world. The adapter
keeps the table alongside the mod state, invalidated on resource reload.

Vanilla categories `monster`, `creature`, `ambient` map to hostile, passive, ambient. Axolotls,
underground water creatures, water creatures and water ambient map to water; miscellaneous
entities fall back to other. Neutral species templates are bee, wolf, polar bear, iron golem,
enderman, zombified piglin, piglin, dolphin, llama, trader llama, panda, goat, spider, cave spider,
and pufferfish. Content mobs inherit their template's neutral trait. Tamed entities compare the
client-known owner UUID with the local player's UUID; missing owner data means other-tamed.
A neutral species remains neutral while angry; this API does not reveal an attack target.

**Budget.** Four accepted queries per mod per mod-frame, shared across commands, tick and frame
hooks; the fifth returns an error and the mod keeps running. Invalid numeric inputs do not
consume this budget. Changing the tick or snapshot does not reset it. Each `Mods::frame` resets
all running mods' counters, including mods without a frame hook. Empty/out-of-world reads still
consume a query. The game prepares only the nearest 256 within 64 blocks once per snapshot,
only while a consumer runs. Each call scans at most 256 cached entries; no world traversal or
strings per entity cross the guest boundary. Prefer 2–4 Hz queries and cached projection per frame.

## Food stats and item values

Capability `game-read`:

`food-stats() -> result<option<food-state>, string>` returns none outside a world, otherwise:

| Field | Meaning |
|---|---|
| `food` | client food level (0..20) |
| `saturation` | client-known saturation points |
| `exhaustion` | optional authoritative exhaustion; single player / LAN host only |
| `health`, `max-health`, `absorption` | current hit points, computed maximum health and absorption points |

Vanilla synchronizes saturation, but not exhaustion. The integrated server publishes its owner's
exhaustion after a tick in a small shared cache, read without a server task or network packet.
It is none before that owner has a live body, and for remote servers and LAN guests. A stale
client-side FoodData exhaustion is never presented as authoritative on a remote connection.

`food-value(item: string) -> result<option<food-info>, string>` reads the **default** food component
from the Items data already loaded by the client. It returns `nutrition`, restored `saturation`
points and `always-edible`. Saturation is already in points in this data: do not multiply by
nutrition or by two again. Unknown and non-food items return none. Invalid namespaced ids
(including traversal, uppercase, missing namespace/path or more than 256 bytes) return an error
before looking up the item. Stack-specific components and tooltip callbacks are deferred.

## Vanilla HUD bars

Capability `render` (`experimental = ["render"]`):

`hud-bars() -> result<list<hud-bar>, string>`, where `hud-bar { kind, bounds: rect }` and `kind`
is `health`, `food`, `armor`, `air` or `mount-health`. Rectangles are top-left `x/y` plus `w/h`
in GUI pixels. Only bars actually rendered are returned; hidden HUD/creative/spectator bars are
absent. A mounted health bar replaces food when the vanilla layout does so.

The game captures bounds from the **last rendered vanilla sprite rectangles**, including the
health/absorption rows, armor above them, air, mount rows, regeneration offsets and hunger jitter.
The shared renderer uses the same bar layout with iOS touch controls; no desktop offsets are
assumed. Overlays align with the currently displayed HUD; a new frame updates the capture.
Before a HUD frame, after a resize, outside the world, or when a native mod supplies the health
layout, the host returns `vanilla HUD layout unavailable` instead of guessing. Bounds are unions,
not individual heart/food slots, and resource-pack transparent margins remain sprite bounds.

## Cost

No consumer: one entity-consumer check, no entity snapshot. With one: one loaded-entity scan and
bounded nearest selection per snapshot, sort at most 256 entries, share with all mods. Each query
filters at most 256 entries and clones at most `max` small records. Kind strings cross only on
`entity-kinds`. Food stats clone one small record; food value is a validated item lookup. HUD
capture groups the vanilla sprite rectangles during the existing render pass. No guest call
per frame is required; the official demo updates at 4 Hz.

## Tests and tools

- `crates/horizon-mods/src/client_queries.rs`: radius and count boundaries, NaN/infinities,
  four-call budget/reset, bounded nearest selection, vanilla category mapping and ownership precedence, food component
  points/non-food/invalid data, HUD rectangle unions including rows and touch coordinates.
- `crates/horizon-client/src/game/mod_observations.rs`: real vanilla draw layouts for health,
  food, armor, air and a mount replacing food.
- `crates/horizon-mods/tests/api35/mod.rs` (`sandbox`): real `hud-demo` SDK guest checks numeric
  validation, 256 cap, nearest order, category lifting, shared per-frame budget, ids checked
  before food lookup, food values/non-food, optional exhaustion, unavailable/hidden/mount bars,
  and the demo's actual saturation fill and clearing. Tests are named `api35_*`.
- `mods/official/hud-demo`: `/hud35 on|off|show|stats|food ID|entities R MAX|kinds|bars`.
  A gold line below the food bar shows saturation plus the held food's saturation preview;
  `/hud35 budget` deliberately exceeds the query budget for the sandbox test.
- Reference: `HORIZON_WRITE_DOCS=1 cargo test --offline -p horizon-mods --features pack --test api_reference`.
- Offline checks: `cargo test --offline -p horizon-mods --features pack`,
  `cargo test --offline -p horizon-mods --features runtime,pack --test sandbox api35_`,
  `cargo check --offline -p horizon-client --features mods`.
- Live checks (user-run): nearby zombie/cow/wolf/player and own/other tames; eat/hold foods;
  single-player/LAN-host exhaustion versus LAN guest/remote none; horse mount, absorption,
  underwater air, GUI resize, touch layout and hidden HUD. No live game/socket tests in this change.

## Scope relative to the design plan

The requested 64-block cap replaces the draft plan's 128. Radius is f32 so invalid negative and
nonfinite input is explicitly rejected; positions use f64 to match existing client coordinates.
`entity-kinds` is the companion table required by the plan's indexed `kind`. The requested food
stats fields are exposed; natural-regeneration/difficulty/effect flags and inventory tooltip
hooks remain deferred. HUD output is a list of visible typed rectangles including armor, air
and mount health, rather than the draft's two-rectangle record. Bounds are the displayed frame's
actual layout, not predictions for a frame that has not been drawn yet.
