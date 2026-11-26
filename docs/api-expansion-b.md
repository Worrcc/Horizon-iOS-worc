# Phase B: API level 30 menus and machines

API level **30** adds eight imports and two optional server exports. No existing
WIT record, variant, function or callback changes; levels 26–29 retain their
contracts. Server manifests use capability `world` and `experimental = ["world"]`
for the new `menus` interface and machine/data functions. The SDK exports
`server::menus`, `MenuItem`, `MachineStack`, and default no-op `ServerMod`
`menu_clicked` / `menu_closed` methods. Missing exports remain loadable.

## Display menus (Q1, Q7)

`menus.open-menu(player, rows, title, items)` opens a stock chest screen with
**1..=6 rows**, replacing any previous screen. `set-menu-items` replaces its
copies; `close-menu` closes only the calling mod's menu. UUID and exact online
name work. Unknown, offline and dead players are errors. One display menu is
owned by one mod per player. Death, leaving, dimension changes, owner stop, and
replacement close it. No client mod is required; a client overlay can be added
in a later phase.

Items are `option<menu-item { item, count, name, lore }>`; omitted trailing slots
are empty. At most `rows * 9` entries; valid registered item and legal stack
count; title/name at most **256 UTF-8 bytes**; lore at most **8 lines**, each
**256 bytes**. Titles, names and lore accept translation keys (an unknown key
shows as its text). Display stacks carry only the requested presentation.

**These are copies. No click moves, drops, clones, swaps, or consumes a real
stack**, including clicks on the inventory underneath and the cursor. The
existing native menu path sends authoritative menu, inventory and cursor state
back after every click. The guest receives optional
`menu-clicked(player, slot, button, mode)` and `menu-closed(player)` exports,
only for its own open menu. Opening/setting/closing returns a deferred queue
receipt; the host rechecks the online player, ownership and current window when
applying it. Reads in the same hook do not establish an engine transaction.

Slots count the menu first, then 27 main inventory cells, then 9 hotbar cells;
`menus.inventory-slot(rows, slot)` returns the main inventory index **0..35** or
none. The native outside-click index **-999** is also delivered. Invalid slots,
buttons, modes, closed/stale windows and dead/off-dimension players are ignored.
Menus opened inside a block interaction remember that block: the player must
stay within **8 blocks of its centre**, in the opening dimension. Every mod has
**64 menu writes** and **64 delivered clicks per server tick**, shared across
players and dimensions. The hub also bounds pending native clicks at **256**;
excess clicks still get corrected but are not queued. Guest traps/overruns use
the ordinary sandbox stop path; they cannot make display copies real.

## Real block inventories (Q2, Q8, Q9, Q5)

A content block adds `container` to its existing block JSON, for example:

```json
{
  "template": "minecraft:stone",
  "container": {
    "slots": 9,
    "title": "block.demo.smelter",
    "faces": { "up": [0], "down": [1] },
    "insert": [0],
    "extract": [1],
    "results": [1]
  }
}
```

`slots` is **1..=54**. Title is at most **256 UTF-8 bytes**, a language key or
text. Face names are `down`, `up`, `north`, `south`, `west`, `east`; lists contain
unique in-range slot indexes. Omitting `faces` exposes all slots on every face;
an explicit map exposes only the listed faces. Omitting `insert`/`extract`
allows every slot for that operation. `results` always removes slots from
insertion and makes them take-only in the player menu. These are slot rules,
not item-id filters. `insert`/`extract` constrain automation; ordinary non-result
player slots use chest rules. Server storage calls are privileged machine logic,
so may write/move into result slots (as a smelter must).

The declaration feeds `native_block_entity_container` and the existing
`ModBlockEntity`/`ModContainer` tables, without native mod code. Right-click
opens a real stock chest-grid menu (any template works; the declaration's open
wins over the template's own use behaviour) unless the Phase A interaction hook cancels
it. Nonmultiples of nine use a rounded-up chest layout with inaccessible empty
padding; storage, persistence and automation still have exactly `slots` cells.
Saved inventories are hydrated to their declared size when loaded. Existing
custom menu-type files remain supported for native menus; they are unnecessary
for this stock layout.

Hoppers and droppers reuse their existing side/insert/extract path. Comparators
reuse chest fullness calculations and update notifications. Player clicks use
the authoritative engine menu with component-preserving real transfers and
server corrections. Breaking spills contents through the existing removal path.
`container`, `set-container`, `container-stacks`, `container-move`, player moves,
and Phase A stack-data calls work on these inventories. Machine logic, including
result-slot writes, uses those calls; there is no second inventory engine.

## Block data and scheduled work (Q3, Q4, Q10)

`world.block-data(dimension, pos)` returns only the calling manifest id's optional
saved UTF-8 string. `set-block-data(dimension, pos, data)` accepts **0..4096
bytes**; none deletes the key, an empty string remains present. Other mods' keys
and unrelated NBT are preserved under the block entity's `horizon:mod_data`
compound. Only loaded declared block inventories are supported; malformed data,
wrong dimensions, unavailable positions and oversize values are errors.

Writes share Phase A's **64 storage moves/data writes per mod per tick**. They
apply after hooks, checking exact state, entity incarnation and the calling
mod's old value. Replaced/unloaded entities and changed values cancel a write
and log the conflict. Independent inventory movements do not conflict with
block data. Success is a queue receipt, not a completed write; reread next tick.
The existing block-entity chunk NBT codec persists data with the inventory.

Use Phase A `world.schedule-tick` for machine work; **no per-block per-tick
callback is added**. Its opt-in, watch filter, delay **1..=72000**, **256 pending**
and **32 deliveries/mod/tick** remain unchanged. Chunk unload drops pending
ticks; timers are not persisted. `world.time(dimension).game-time` is the
catch-up clock. A mod should rearm when its block is used or through another
bounded discovery strategy. The demo rearms on interaction after an unload.

`block-data` is readable during cancellable `block-breaking`, before the entity
is removed. Breaking loses its block data. To customize a drop, read it there,
cancel the normal break and explicitly create the intended tagged drop using
Phase A stack-data creation and the world helpers. Stack data retains its own
**1024-byte** limit: copy a bounded subset/reference when block data is larger.
There is no new callback signature and no automatic data copying onto drops.

## Machine helpers (Q11)

`world.destroy-block(dimension, pos, tool, collect)` uses the native machine
helper: engine loot for a plain registered tool (none is an empty hand), break
particles/sound, block-entity contents, loot hooks and normal removal effects.
Unloaded/air/unbreakable positions, invalid tools and dimensions are errors.
At most **64 requests/mod/tick**. Without `collect`, real drops spawn and the
result is empty. With `collect`, loot and container contents are returned instead
of spawned, including all saved components as lossless network-NBT bytes in the
new additive `world.stack` record. These bytes are read-only; this phase adds no
arbitrary-NBT write or component reconstruction import.

Responses allow **256 stacks**, **65536 component bytes/stack**, **1048576 total
bytes**. If a response exceeds those bounds, the engine has already broken the
block: it spawns the original drops intact and returns an explicit error. This
avoids losing contents on an oversized result. As with other accepted host work,
a subsequent guest trap does not roll the world back. Collected outputs a mod
chooses to discard do not remain as item entities.

Destruction runs synchronously on the game-owning thread, using a scoped guest
worker and in-process rendezvous channels, following the client container
bridge. The world loan is restored only while servicing the request. No borrowed
Game pointer enters a Store and no WASM store is reentered. This is machine
work without a player: it does not synthesize cancellable player break events.
Already-loaded positions in the server's dimensions are supported. During a
player hook the actor is temporarily parked while machine work runs in its
target dimension, then restored before that hook resumes; no fake player is
created.

`world.set-break-progress(dimension, pos, stage)` accepts **-1 or 0..=9**, with
**64 requests/mod/tick**. It queues the native crack broadcast within **64
blocks**, using a distinct negative breaker id per loaded mod. -1 clears it;
one mod id represents one active crack at a time. Unrenewed cracks fade after
400 ticks. No guest selects an entity id. Changing stage does not break blocks.

Not included: fake-player use, fluids, energy, item pipes, client overlays,
custom code-rendered screens, persisted scheduled ticks, arbitrary NBT writes,
or a callback per loaded machine per tick.

## Demo and offline validation

`mods/official/machine-demo` is a server guest; `content/` is a separate content
mod providing `demo:crate` (9 real slots) and `demo:smelter` (input 0, result 1).
Build both from this worktree, without network:

```sh
wt=/path/to/storage-api
export CARGO_TARGET_DIR="$wt/target" CARGO_NET_OFFLINE=true
cd "$wt"
cargo build --offline --release -p cargo-horizon-mod
cd "$wt/mods/official/machine-demo"
"$wt/target/release/cargo-horizon-mod" build
cd content
"$wt/target/release/cargo-horizon-mod" build
```

Artifacts: `target/horizon-mod/machine_demo.hmod` and
`mods/official/machine-demo/content/target/horizon-mod/demo.hmod`.
For a manual live test, build a game with feature `mods` and prepared vanilla
resources. Create fresh scratch folders for `HORIZON_HOME`,
`HORIZON_LAUNCHER_HOME`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`; set
`HORIZON_LAUNCHER_NO_SELF_UPDATE=1`, `HORIZON_NO_AUDIO=1`. Run from a fresh game
folder with `--game-dir` pointing there, `--creative --world MachineDemo`,
`--resources "$wt/.local/horizon-client/26.3"`, and both artifacts as separate
`--dev-mod` arguments. Every joining Horizon client must load the content mod;
the server guest needs no client half. Stock menus use stock protocol types;
vanilla clients still cannot understand new content block/item registries.

Give/place `demo:crate` and verify chest-like transfers, hopper/dropper sides,
comparator fullness, save/reload and break spills. Place `demo:smelter`, insert
items into slot 0: every 40 ticks one real item moves to result 1 and its decimal
counter increments in block data. After chunk unload, use it to rearm. Interact
with a lectern: a display-only 3-row menu opens; clicks log, and left-clicking
the centre diamond-block icon grants one diamond through `players.give-item`.
`/demobreak x y z` queues cracks, breaks with an iron pickaxe and spawns drops,
then clears cracks. Check from another player as well as the acting client.

Automated checks cover all eight imports through a sandbox guest, validation,
per-tick caps, deferred preimages, owned click routing and close events, pure
validators, real component-preserving menu transfers, result/padding rules,
block-data NBT roundtrip and lossless/bounded collected component encoding.
Live networking/gameplay and phone rendering require a separate manual run.
