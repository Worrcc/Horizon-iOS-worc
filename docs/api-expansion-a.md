# Phase A-1: API level 29 behaviour hooks

API level 29 adds optional server exports and four host imports. Existing WIT
records, functions and export signatures are unchanged, including level 28's
`registration`, `set-block-uses` and `block-used`. Missing new exports mean allow;
the runtime resolves them individually. SDK `ServerMod` methods have allow/no-op
defaults. No new capability or `Output` variant is needed: opt-in uses `world`,
and mutations use existing bounded world/player interfaces.

## Opt-in and matching

Call `server_hooks::set_behaviour_hooks(BehaviourConfig { ...Default::default() })`
and install filters with `watch_blocks` / `watch_items`. Each call replaces the
corresponding configuration/list. All flags start false and both lists empty.
Lists accept up to 64 exact namespaced resource ids or `namespace:*`; global `*`,
partial wildcards, uppercase ids and state properties are rejected atomically.
Vanilla ids are allowed. Matching a state ignores its `[properties]` suffix.
The filter restricts delivery, rather than granting ownership of a block or item.

Block families use block ids, item families use item ids. Pickup and chat use
only flags, without id filters. Turning on behaviour callbacks does not enable
the old level-28 block-used callback, and does not require registration.on_blocks.
A mod with no matching enabled export receives no calls or world loans.

Each individual family gets at most **32 calls per mod per server tick**, shared
across players and dimensions. Neighbour/random observations reserve that budget
before enqueueing, keeping queues bounded to 64 events per mod. Scheduled
notifications share their own 32-call family budget; due notifications beyond it
are discarded. Budgets reset on the next tick. Exhaustion logs once per family
per loaded mod. Traps and fuel/time overruns stop the faulty mod using the normal
runtime error path, log once, and allow the action. All guest code runs on the
server thread with the existing event fuel allowance (100,000,000 in fuel mode).
Callbacks do not turn a sequence of host requests into a transaction: previously
accepted output from a guest is not rolled back if it later traps.

## Block changes and interaction

- `block-breaking(player, pos, state) -> decision`: the existing native destroy
  hook, after permission/item restrictions and before destruction or drops.
- `block-placing(player, pos, state, item) -> decision`: an engine-validated
  placement candidate, before any writes, including the preliminary writes for
  double-high blocks. State is the computed placement state, before item-stack
  block-state components. The engine now prepares and commits the same candidate;
  its native state-for-placement hook runs once. Cancelling discards the candidate.
- `block-interacted(player, pos, state, face, hit-x, hit-y, hit-z, hand, item,
  sneaking) -> decision`: the same validated packet ingress used by block-used,
  before vanilla block interaction. Both hands are supported; face is vanilla's
  0 down, 1 up, 2 north, 3 south, 4 west, 5 east. Hit coordinates are relative to
  the clicked block. An empty hand is `minecraft:air`.

`decision` is `allow | cancel`. Multiple mods run in load order; the first cancel
ends dispatch. Rich interaction runs before any level-28 block-used callback;
cancel skips block-used entirely. If allowed, block-used retains its original
main-hand and sneaking-item rules.

Cancel keeps the authoritative block and held item, prevents drops/placement/use,
and suppresses follow-up use packets for that connection for the rest of the
server tick, as level 28 already does for consumed interactions. Refused actions
award no block-use advancement. Existing break acknowledgements and use-on-block
updates correct the client's predicted blocks; a refused use also sends the full
inventory once the acting guest is restored. Existing post `block-broken` and
`block-placed` callbacks are unchanged and never report a cancelled change.

Native player-turn callbacks borrow the live loaded world for reads and queue
host mutations for the ordinary server-mod flush. No engine placement, mining,
interaction or RNG logic is copied into the WASM bridge. Native mods still run
at their original hook points; an earlier native cancellation is respected.

## Neighbours, random ticks and scheduling

`block-neighbour-changed(dimension, pos, state, from)` observes the engine's
native neighbour hook. **`from` is the changed block resource id**, matching the
native hook's `BlockId`; the engine does not supply a source position there.
`block-random-tick(dimension, pos, state)` observes the native random-tick hook.
The engine decides eligibility and random sampling, including content blocks'
`random_ticks` definitions. This also supports watched vanilla ticking blocks.
These notifications capture the state at the hook point and are delivered at the
server-mod flush after the engine turn. They do not cancel or replace vanilla
neighbour/random handling, and do not make extra random draws.

`world.schedule-tick(dimension, pos, delay-ticks) -> result<_, string>` schedules
an observation for the calling mod only. It requires the scheduled flag, a
watched block in a loaded chunk, a known dimension, and delay **1..=72000**.
Invalid positions/delays fail before queueing. A mod may have **256 pending**
entries total, across dimensions; duplicates count individually. Due time uses
the server tick counter, not the world's editable day time. No chunk is loaded.

At delivery the exact state must still match. The native block-change and chunk
unload hooks also invalidate pending entries, including a block changed away and
back, or a chunk unloaded and reloaded before delivery. Unavailable chunks are
silently discarded. Entries are removed before calling
`block-scheduled-tick(dimension, pos, state)`, so callbacks may schedule another
tick. Entries are **not persisted across restarts**, and are cleared on world
close and mod stop/reload. This queue is separate from vanilla's scheduled ticks;
it neither replaces nor reports vanilla scheduled ticks.

## Items and general events

`item-used-on-block(player, item, pos, state, face)` uses native use-item-on,
after the block passed the interaction and before the item acts.
`item-used(player, item)` uses native use-item, after cooldown checks.
`item-hit-entity(player, item, entity-kind, entity-id)` uses the native attack
hook, after target/reach checks and before the attack. The entity id is the
signed network id. All three return decision; cancel suppresses their action
without consuming the item. `item-use-finished(player, item)` observes the native
player finish-use hook with the original item, after its timed use completes.
It is not cancellable. Stack components/counts are not part of these arguments.

`item-picked-up(player, item) -> decision` uses the native pickup cancellation
hook. Cancel leaves the item on the ground with the native 10-tick retry delay.
`chat-sent-filter(player, text) -> chat-decision` runs after secure-chat chain
validation, server moderation and the native chat hook, before broadcasting.
`chat-decision` is allow, cancel, or replace(text). Valid replacements contain at
most 256 Unicode scalar values and only vanilla chat characters. Invalid output
allows the current message and logs once. Mods compose replacements in load
order; cancel ends the chain. Signed bodies/signatures/acknowledgements remain
intact; replacement is unsigned display content. Post-chat hooks/logs receive
the displayed text. Commands are not chat-filter events.

## Deliberately not included

Cancellable player damage is deferred: the native incoming-damage hook is
hostless, inside borrowed health/mob processing, with no safe game/WASM runtime
context. Cancellable item drops are deferred: the native drop callback only
observes a stack already removed from its origin. Refusal needs origin-aware
changes across hotbar, creative, menu and overflow paths to preserve components
and inventory conservation. Neither unsupported flag/export is advertised.

Per-stack custom data, mixins, vanilla scheduled-tick interception, synchronous
neighbour mutation/replacement, persistence for timers, menus, command filtering,
and later phases of the expansion plan are outside A-1.

## Demo and validation

`mods/official/behaviour-demo` is a server-only guest. It refuses breaking obsidian,
placing TNT and using ender pearls. Interacting with a lectern schedules a tick
20 later, sets gold above the still-unchanged lectern with world.set-blocks, and
logs it. Chat replaces the whole lowercase word `heck` with `hmm`.

Build offline with the worktree's release cargo-horizon-mod from that directory:

```sh
CARGO_TARGET_DIR=/path/to/worktree/target CARGO_NET_OFFLINE=true \
  /path/to/worktree/target/release/cargo-horizon-mod build
```

The package is `target/horizon-mod/behaviour_demo.hmod`. Load it as a server dev
mod in a scratch game directory. Verify obsidian/TNT and stack counts from both
the acting client and server, wait 20 ticks after a lectern click, try an ender
pearl, and send `oh heck!`. Do not use real player data. Live-game and phone tests
are manual; this implementation's automated checks use sandbox guests and pure
engine/validator tests without networking.
