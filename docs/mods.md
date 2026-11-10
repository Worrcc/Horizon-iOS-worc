# Horizon mods

This fork keeps the WebAssembly mod runtime and SDK alongside upstream Horizon’s content and native mod systems. Three kinds of mod run in Horizon:

| Kind | File | What it is | Where it runs |
|---|---|---|---|
| **WebAssembly mod** | `.hmod` with `mod.wasm` | Sandboxed code using the versioned WIT API and server-mod hooks. | Runs in the game’s WebAssembly runtime |
| **Content mod** | `.hmod` | Data and assets: blocks, items, recipes, loot, tags, models, textures, structures. No code. | Read when the game starts |
| **Native mod** | `.hnmod` | A [horizon-mixins](https://github.com/horizon-rust/horizon-mixins) bundle: Rust that changes the game's own source before it is compiled. | In a game the launcher builds for the player ([native-mods.md](native-mods.md)) |

Fabric and NeoForge jars do **not** run as they are: their code calls and patches Minecraft's Java classes, and
Horizon has none. What carries over is the data and assets inside a jar ([modpacks.md](modpacks.md)).

## Design goals

1. **Safe to install.** A mod is untrusted code. It runs in a WebAssembly
   sandbox with no file, network, process or clock access, a memory cap and a
   time budget per call. It can only do what its capabilities grant, and the
   player sees those before turning it on.
2. **One file for every platform.** The same `.hmod` runs on Windows, macOS
   and Linux, x86-64 and ARM: authors build once, players never meet "wrong
   build for my OS".
3. **Free when unused.** No mod on: no engine, no thread, no compile, one
   branch per hook. Measured below.
4. **Vanilla stays vanilla.** Mods add; they cannot patch the game's code the
   way Mixins do. What they may change is a list of hooks the game calls at
   fixed points, so the parity harnesses keep testing the same game.
5. **Stable for authors.** The contract is a versioned WIT package; a mod
   built for API 0.1 keeps loading on later 0.1.x games.

## How it works

```
 mod crate (Rust)                         Horizon
 ───────────────                          ───────
 impl Mod for MyMod { ... }               horizon-mods::Mods
 export_mod!(MyMod)                         ├─ Engine (cranelift), made on first mod
        │ cargo horizon-mod build           ├─ one Store per mod: 64 MiB, fuel per call
        ▼                                   ├─ Linker: log, game, chat, hud (WIT imports)
 my_mod.hmod = zip {                        └─ calls the events export: init, tick,
   horizon-mod.toml,                             frame, command, chat
   mod.wasm  (a WebAssembly component)
 }
```

- **WebAssembly components.** Mods compile to `wasm32-unknown-unknown`;
  `cargo horizon-mod build` wraps the module into a
  [component](https://component-model.bytecodealliance.org/) for the
  `horizon:mods/horizon-mod` world. The WIT file is the whole API: host
  functions the mod imports (`log`, `game`, `chat`, `hud`) and the `events`
  interface it exports. No WASI: the component cannot even name a file or a
  socket.
- **Capabilities are imports.** Every imported interface except `log` is a
  capability. The manifest declares them; the game reads the component's
  actual imports and refuses a mod whose code imports one its manifest does not
  declare (`uses capability chat, missing from horizon-mod.toml`). So the list
  the player reads is the truth, not a promise.
- **Hooks on request.** `init` returns a `registration`: the commands the mod
  owns and which of `tick`, `frame` and `chat` it wants. The game calls only
  those. A mod that wants nothing per frame costs nothing per frame.
- **Retained HUD.** A mod sets its panel (corner and coloured lines) when it
  changes; the game draws it every frame with the extras' overlay code. Most
  HUD mods need no frame hook at all.
- **Snapshots, not callbacks into the world.** Once per tick (and before a
  command) the game copies what mods may read (player position, look, health,
  food, dimension, world clock) into a snapshot shared by all mods. Mods never
  hold references into the game, so they cannot stall or corrupt it.

### What a mod can do (API 0.1)

| Interface | Capability | Functions |
|---|---|---|
| `log` | always | `write(level, message)`, in the game log as `[mod <id>]` |
| `game` | `game-read` | `get-player()`: x y z, yaw, pitch, health, food, dimension; `get-clock()`: game time, day time; `raycast(distance)`: the first block along the view (fluids pass), up to 1024 blocks and across the distant terrain, with the point hit and the block's id; `raycast-entity(distance)` (API level 16): the nearest living entity before a block, its display name and current/max health; `world-id()` (API level 17): `sp:<save folder>` or `mp:<address>`, for what a mod keeps per world |
| `chat` | `chat` | `show(text)`: a line only the local player sees, after the mod's name (8 per call) |
| `hud` | `hud` | `set-panel(corner, lines)`, `clear()`: up to 16 lines of 256 characters |
| `storage` | `storage` | `get(key)`, `set(key, value)`, `delete(key)`: a per-mod folder of up to 64 files, 4 MiB total, at most 1 MiB written per call |
| `command` | `command` | `run(line)`: a vanilla command run as the player, with the player's permissions, after the hook returns (8 per call); mods' own commands are not run. `run-quiet(lines)`: up to 256 a call, the same rights, nothing in the chat (single player and Horizon's servers run them 64 a tick and log one line a batch; elsewhere they go as chat commands and their feedback shows) |
| `ui` | `ui` | `open-screen`, `close-screen`, `set-settings`: a screen the mod owns and a page in Options > Mods, of text, button, item and list widgets the game lays out and draws with its own menus' code; `browser.set-items`, `browser.poll`: a recipe viewer's panel of items right of the inventory and container screens (not the creative one), which the game pages and draws with its own slots and search box (Ctrl+F, or Cmd+F on macOS, focuses it while the panel is shown), and the mod hears its clicks, its searches and the mod's own keys (`input.register-key`) pressed over any item on the screen; `browser.set-recipes`: recipes in the panel's place (crafting grids, cooking, stonecutting, smithing and brewing layouts, their slots cycling through their stacks, a note and an action button each), paged by the game, with clicks on their slots, action buttons, page turns and a back button. `browser-ext` (level 19) adds a paged bookmark panel left of the window, hidden while the recipe book is open, and `keep-empty` to keep the browser and search box visible with no results; `set-item-lines` adds grey tooltip lines to items in either panel. **Experimental:** the manifest needs `experimental = ["ui"]` |
| `render` | `render` | `set-hud(shapes)`: up to 1024 fills, four-cornered shapes, GUI sprites, items and text, kept on the HUD (over the hotbar, under chat and screens, hidden with F1) until replaced; `set-corner(corner, w, h, shapes)`: a box the game places in a corner, stacked with its overlays and other panels, as `hud`'s panels are; `text-width(text)`; `frame-stats()`: the game's FPS, min and 1% low; `gui-size()`; `shake(strength, seconds)`: the view turns, the player doesn't, scaled by Distortion Effects; `set-zoom(zoom)`: the field of view divided by up to 10, eased or at once, with slower turning, while playing and not through a spyglass (the strongest of several mods' zooms counts); `set-minimap(minimap)`: a map of the world around the player in a corner, as a map item colours it (the floor underground and under roofs), square or round, turning with the player or north up, with the player's arrow; the game keeps its tiles within its own frame budget (one map at a time, the first loaded mod's); `set-far-terrain(far-terrain)`: the world past the render distance as distant terrain, out to 64 to 1024 chunks, built by the game's own workers (the player's LOD Distance, when on, wins; the first loaded mod's). The game's own HUD renderer draws them: a mod never supplies shader code. **Experimental:** `experimental = ["render"]` |
| `canvas` | `render` | API level 17. The mod's own pixels: `create-image(w, h)` (each side 1 to 1024, 1024 × 1024 texels per mod in all) on one atlas page the mods share; `update-image(image, x, y, w, h, rgba)` (straight-alpha RGBA, at most 1 MiB a frame per mod), shown from the next frame; `drop-image(image)`; `set-corner(corner, w, h, shapes)`: a box stacked with the other corner panels, holding images (a `src` rectangle of texels stretched over `dst`, tinted, square or clipped round) beside `render`'s fills, quads, sprites and text; `request-frame-budget(µs)`: up to 500 µs a `frame` call (100 by default); `work-left()`: how much of this call's budget is left, about 1000000 at its start, so a mod works `while work-left() > reserve` with no clock. The game measures how fast the mod burns fuel on this device (Pulley on iOS is slower) and sets each call's budget from it. **Experimental:** `experimental = ["render"]` |
| `client-world` | `world-read` | API level 17. What this game has loaded, in single player and on any server (only what the server sent): `columns(chunks, layer, depth)`: up to 16 chunks a call, each column's top block in the layer (the surface, or the floor below a height as the game's minimap maps caves and the Nether) and up to 16 state ids down from it, its water depth, the light above it and its biome's grass, foliage and water colours; `changes(since, radius)`: the chunks changed, loaded or unloaded since a counter, nearest first; `block-table()`: every block state's block, facts (air, water, see-through, ...), the texture its model shows up and south with its tint source, and its map colour, from the loaded packs (its `version` changes on a resource reload; `table-version()` reads it alone, cheap enough every tick); `texture(name)`: a block texture's RGBA from the loaded packs, so a mod averages its own colours; `dimension()`, `light(x, y, z)`, `biome(x, y, z)`. **Experimental:** `experimental = ["world-read"]` |
| `input` | `input` | `claim-items(ids)`: the use button on those items is the mod's (`use-start`, `use-stop` with the ticks held; the game doesn't place, eat or swing); `register-key(name, label, key)`: up to 16 keys (`key.keyboard.r`), heard while playing as `key-down` / `key-up` (the game's own mapping on that key still runs; a screen opening lets held keys up), mouse buttons as `key.mouse.4`. The player rebinds them on the mod's page in Options > Mods, where the game lists them first as Key Binds rows (click, then press a key or mouse button; Esc binds none), even for a mod without `ui`; the choice is kept in `horizon-mods.txt` (`key.<mod>.<name>:key.keyboard.z`) and wins over `register-key`'s default; `claim-wheel(on)`: the wheel's steps become `wheel` events and the hotbar doesn't move (the one input a mod takes from the game; a mod's keys never do); `held()`; `mapping(name)`: a movement or click mapping's key label and whether it is held; `clicks-per-second()`; `poll()` returns the events, at most 256 kept. **Experimental:** `experimental = ["input"]` |
| `recipes` | `recipes` | `items()`: every item in the registry's order, with its name in the player's language and its tags; `all()`: every recipe (crafting, smelting, blasting, smoking, campfire, stonecutting, smithing, brewing), its slots as the stacks that fit each, its results, cooking time and experience, and the recipe book's id for it; `known()`: the book ids the player's recipe book holds; `place(book-id, all)`: the recipe book's click on a recipe, in the open inventory, crafting table, furnace, blast furnace or smoker (furnaces not yet on Horizon's own server, single player included; refused for a recipe the player's book doesn't hold, as vanilla's server refuses it); `give(stack)`: in creative mode, the stack into the player's inventory, as from the creative inventory. The recipes are the world's in single player and on Horizon's servers, vanilla 26.3's on any other server (a client is sent no others), while `known` follows that server's book. The game makes the tables once, when a mod first reads them. **Experimental:** `experimental = ["recipes"]` |

| Hook (export) | When |
|---|---|
| `init() -> result<registration, string>` | once, after loading; an error keeps the mod off and is shown |
| `tick(tick)` | each game tick while in a world (20 per second) |
| `frame(seconds)` | each rendered frame |
| `command(name, args) -> result<option<string>, string>` | `/name args` typed in chat, for a command the mod registered |
| `chat(text)` | each chat line the player receives, as plain text |

Commands: vanilla's command names win. A mod asking for `/give` does not get
it (logged), and two mods cannot share a name (the first one loaded keeps it).
The chat box colours and Tab-completes a mod's commands like vanilla's, for
every player whatever their permission level, and takes any text after the
name (the mod reads it as `args`; API 0.1 has no way to declare argument types).

### Planned interfaces

Each is a new capability, added in a 0.x minor version so older mods keep
loading. Ordered by what the first native ports need:

| Interface | For | Notes |
|---|---|---|
| `keys` | keybinds | registered binds appear in Controls; press/release events |
| `target` | the crosshair's block or entity, with its state and NBT summary | Jade/WTHIT-style tooltips |
| `inventory-read` | the player's inventory, held item, open container | AppleSkin, recipe viewers |
| `render` (world) | world-space lines, boxes and billboards, beside the HUD shapes `render` has | Litematica-style placement, waypoints |
| `registries` | new blocks, items, entities and recipes, mostly data: JSON models, textures, loot and recipes in the `.hmod`'s `assets/` and `data/` | server side; see [Multiplayer](#multiplayer) |
| `server` | world events (entity spawn/death), game rules, world read/write, typed server commands | after [server mods 0.3](#server-mods-api-03-experimental)'s players, chat, blocks and commands |
| `net` | a mod's own client↔server channel (custom payloads) | only between the two halves of the same mod |

There is no file, socket, process or raw-input interface, on purpose.

## Choosing the runtime

| Option | Sandbox | One file for all OSes | Typed API | Cost | Verdict |
|---|---|---|---|---|---|
| **wasmtime + component model** | yes: memory, fuel, no imports but ours | yes | WIT → Rust on both sides | 85 ns per empty hook call; 50 ms to compile a 64 KB mod on load | **chosen** |
| wasmtime, core modules with a hand ABI | yes | yes | no: pointers and lengths by hand | a few ns less per call | not worth the fragile ABI |
| wasmi (interpreter) | yes | yes | no components | no compile time, execution ~10-50x slower than cranelift code | kept in mind for a platform without JIT |
| Lua (mlua) or another script | partial (strip the stdlib, hope) | yes | no | fast to write | not Rust; weak sandbox |
| native `cdylib` plugins | none | no: per OS and arch, unstable Rust ABI | yes | none | unsafe for players |
| an embedded JVM | none per mod | yes | Java | 100+ MB, seconds of startup | see the research |

wasmtime 48 is a long-term-support line (security fixes until August 2028)
and needs Rust 1.95. It reads WebAssembly with the same wasm-tools release
(0.254) as the `wit-component` that packs mods and the SDK's `wit-bindgen`, so
the build carries one copy of them. Only its `runtime`, `cranelift`,
`component-model` and `std` features are on: no WASI, no async, no GC
proposal, no profiling. Its cranelift backend generates native code for x86-64
and ARM64 on Windows, macOS and Linux, which is why one `.hmod` serves them
all.

The game links it only with its `mods` feature: every release build turns it
on (`release.yml`), and a build from source needs
`cargo build --release -p horizon-client --features mods` to run code mods.
Without it (the tests and probes) wasmtime and cranelift aren't built, a
quarter of a clean build; content mods still load, and the game logs why the
code mods don't start.

### Performance

Measured on the Mac mini (M4 Pro), release build, `cargo test -p
horizon-mods --features runtime,pack --release --test sandbox -- --ignored --nocapture`:

| | per call |
|---|---|
| tick with no mod loaded | 1.2 ns (one check) |
| empty `tick` hook (call into the sandbox and back, fuel on) | 85.5 ns |
| empty `frame` hook | 87.5 ns |
| coords-hud's `tick` (reads the player and clock, formats 3 lines, compares) | 2.3 µs |
| loading coords-hud (read, compile with cranelift, instantiate, `init`) | 50 ms, once |

In the game, same world and view, `horizon-ctl bench 10` three times each
(4 other sessions shared the machine, so the tails are noisy; the median is
the stable figure):

| | median frame | mean FPS |
|---|---|---|
| no `horizon-mods` folder | 2.62-2.71 ms | 149-163 |
| coords-hud on | 2.72-2.78 ms | 151-254 |

At 20 ticks per second coords-hud costs 46 µs per second of game time: well
under 0.01 ms per frame. The frame-time medians differ by less than the
run-to-run spread.

**Budgets.** Each call gets fuel (about one unit per WebAssembly instruction):
2 billion for `init`, 50 million for `tick`, 10 million for `frame`, 100
million for a command or chat line. A runaway loop ends in a fraction of a
second (the sandbox test's endless `/spin` is cut well within its 5 s limit)
instead of hanging the game. The target budgets a mod should stay under are
tighter: **1 ms per tick and 0.1 ms per frame**; a mod with heavy `frame` work
(a minimap baking chunks) asks for up to 0.5 ms with
`canvas.request-frame-budget` and stops when `canvas.work-left` says it is
spent (`the_frame_budget_converges` in the sandbox tests: a 300 µs budget
holds to within a few percent, with Cranelift and on Pulley,
`--features force-pulley`, as iOS runs mods). The runtime keeps each mod's
average call time (`Entry::cost`); the Mods page will show it, and a mod over
budget for long will be flagged, then paused.

### Crash isolation

A trap (panic, out-of-bounds access, `unreachable`), running out of fuel or
hitting the 64 MiB memory cap stops **that mod only**. The game says so in
chat (`Mod Test Guest stopped: it took too long (an endless loop?)`), marks it
on the Mods page and carries on; the other mods keep running. Host functions
clip what a mod hands back (chat lines per call, HUD lines and lengths). Tests:
`crates/horizon-mods/tests/sandbox.rs` (an endless loop, a panic and a 1 GiB
allocation, next to a healthy mod; `cargo test -p horizon-mods --features
runtime,pack`).

## Playing with mods

1. Install a mod from the launcher's Mods page ([mod-browser.md](mod-browser.md)), or put its files by hand in the
   game folder (the one holding `options.txt`): a content mod's `.hmod` (or a folder holding `horizon-mod.toml`) in
   `horizon-mods/`. Native mods go in `horizon-native-mods/`, and the launcher builds them
   ([native-mods.md](native-mods.md)).
2. Options > **Mods** lists them with what each may do ("May add blocks, items, mob effects and entity types to your
   worlds") and a switch. They start off. The choice is kept in `horizon-mods.txt` (`mod.<id>:true`).
3. A content mod's blocks, items, mob effects and entity types load once, when the game starts: restart the game after
   a switch.
4. A mod that can't load shows why, in red, on its row.

Two turned-on content mods never own the same namespace: the first by file name loads, and the other stays out with
the reason in the log.

Benchmarks (`--benchmark`) never load mods.

## Making a content mod

```sh
# A prebuilt binary for Linux x86_64/aarch64, macOS x86_64/arm64 or Windows x86_64 (#2120):
cargo binstall --git https://github.com/horizon-rust/horizon cargo-horizon-mod
# Or build it from source, without cargo-binstall:
cargo install --git https://github.com/horizon-rust/horizon cargo-horizon-mod
cargo horizon-mod new my-mod        # a content mod: manifest, one block with its model, loot and recipe
cd my-mod
cargo horizon-mod run               # build my-mod.hmod, then start the game with it turned on
cargo horizon-mod build             # only build: target/horizon-mod/my_mod.hmod
cargo horizon-mod check target/horizon-mod/my_mod.hmod   # the registry's checks; --json for JSON
```

- `new <path> [--name "Name"]` writes the template. `--template native [--horizon <commit>]` writes a native trait
  mod's bundle instead: `horizon-mixin.toml` (pinned to that Horizon commit, else unpinned) and an `[api]` crate with
  one hook, built with `horizon-launcher native-build <path> --trust-native-code`
  ([native-mods.md](native-mods.md#trait-mods-draft)). Any other `--template` is refused.
- `build` packs `horizon-mod.toml`, `assets/` and `data/` into `target/horizon-mod/<id>.hmod`, with the same checks
  the game makes when it reads the content. It refuses a folder with a `Cargo.toml` (code is native now) and a
  manifest without capability `registries`.
- `run` builds, then starts `horizon-client` (on `PATH`, or `--game PATH`, or `HORIZON_CLIENT`) with
  `--dev-mod target/horizon-mod/<id>.hmod`, always on. Arguments after `--` go to the game. The game reads content once
  at start, so rebuild and run again after an edit.
- `check <file>` runs what the registry runs: the file's size and SHA-256, its manifest and content, and a signature
  with `--key <public key> --signature <file>.minisig`. It reads `.hmod`, `.hnmod`, `.hpack`, resource pack `.zip`
  and `.hshader` files.
- `registry build|review|check|list|release|page` is the registry repository's CI ([api/mod-index.md](api/mod-index.md)).

### The manifest

```toml
id = "my_mod"                # [a-z0-9_]{1,64}: settings key, log name
name = "My mod"
version = "0.1.0"
description = "..."
authors = ["..."]
api = "0.2"                  # content mods
side = "both"
capabilities = ["registries"]
namespaces = []              # optional: more namespaces the mod's files use (below)
```

Unknown keys are refused, so a typo is an error, not a silently ignored permission.

### Content mods (API 0.2)

A content mod has capability `registries` and carries `assets/` and `data/` in the `.hmod`. It adds blocks and items
by declaring them in `data/<id>/horizon/block/*.json` and `data/<id>/horizon/item/*.json`, each copying a vanilla
template ([modpacks.md](modpacks.md#registries-02-data-driven-blocks-items-and-mob-effects)), and creative tabs, mob
effects, entity types and menu types (each below) in `creative_tab/`, `mob_effect/`, `entity_type/` and `menu/` beside
them. LAN and dedicated server players play with them when they run the same mods: a client whose mods add different
content is refused when it joins, with the reason.

A server that has content mods on also checks them one by one (`horizon_protocol::mod_list`, channel
`horizon:mod_list`): in the configuration phase it sends the id, version and SHA-256 of each `.hmod` with
`side = "both"`, the client answers with its own, and a client that differs is refused with the mods named: missing,
not on this server, or another version or file. Only `side = "both"` mods are compared and sent: a `client` mod (a
HUD, a minimap) never blocks a join and is never told to a server, and a `server` mod stays private to it. A manifest
with no `side` line is `client`, so a content mod meant for both halves must say `side = "both"`, or this check
ignores it. A mod run from a folder (`--dev-mod`) has no file hash and matches by version. Native mods are not in the
list yet: their manifest has no side.

A block names its template and what differs from it:

```json
{ "template": "minecraft:iron_block",
  "properties": { "hardness": 4.0, "light": 3, "sounds": "minecraft:amethyst_block" },
  "item": true }
```

`random_ticks` makes a block tick randomly whatever its template does, as vanilla's
`isRandomlyTicking(state)` does for its own blocks, so a crop, sapling or soil on a template that never
ticks still grows: `"random_ticks": true` for every state (or `false`, to stop a template that ticks), or
`{ "property": "grown", "values": ["false"] }` for the states whose own state property (declared in
`state_properties`) is one of the values. The block's native `native_random_tick` hook then fires as it does
for a vanilla block that ticks, and its sections count it.

`render_shape` says how a block is drawn whatever its template does, as vanilla's `getRenderShape`:
`"MODEL"` puts its block model in the chunk mesh, `"ENTITYBLOCK_ANIMATED"` leaves the model out for a block whose
moving parts its block entity draws every frame (a turning shaft, a machine's arm), and `"INVISIBLE"` draws nothing.
Its outline, collision, light and occlusion stay as declared: on a template that occludes (stone), the faces it
covers stay hidden unless it also sets `occludes`.

`occludes: false` makes a block occlude nothing whatever its template does, as vanilla's `noOcclusion()`: the faces
of the blocks beside it are drawn, so a thin panel or a block drawn by its block entity on a full-cube template
doesn't leave a hole in the floor under it. It is no longer a solid render either, so light passes it as it passes
leaves: dampened by 1, or not at all if its template lets skylight down. Unset or `true`, it occludes as its template
does.

A block copies its template's states. It can also declare state properties of
its own, which its template does not have — how a pipe says which sides it
connects to, or a machine whether it is running. Its states then become its
template's states times the combinations of the ones it declares, laid out as
vanilla lays out a block's states (`StateDefinition`): every property, the
template's and the declared ones, sorted by name with the first name varying
slowest, and each declared property's default value first. The order is fixed,
so ids do not move between loads, and a declared name may sort before one of
the template's (`bites` on a carved pumpkin, whose `facing` comes after it):

```json
{ "template": "minecraft:stone",
  "state_properties": {
    "facing": { "type": "enum", "values": ["north", "south", "east", "west"] },
    "lit": { "type": "boolean" } },
  "default_state": { "lit": "true" } }
```

A `boolean` is `false` and `true`, as vanilla spells them; an `enum` is a list
of values, and a name or value is lower case letters, digits and underscores.
A declared name may not be one the template already has, and `default_state`
takes one value per declared property too, so a block's default is exactly one
state of its own. Every combination has to fit in the states a world holds
(2^20), so a declaration that would not is refused when the mods load.

A block whose template is one of vanilla's signs is a sign, whatever its name: it has vanilla's sign block
entity, so the text, the editor, dye, glow ink, wax and saving all work as they do on an oak sign, and the
client draws its text. A wooden sign is two blocks and a hanging sign two more, one form per template, and the
one with the item names its wall form:

```json
{ "template": "minecraft:oak_sign", "wood_type": "ruby:elm", "wall_block": "ruby:elm_wall_sign" }
```

```json
{ "template": "minecraft:oak_wall_sign", "wood_type": "ruby:elm", "item": false }
```

The same goes for `minecraft:oak_hanging_sign` and `minecraft:oak_wall_hanging_sign`. `wall_block` is what the
sign's item places on a wall, a block of the same mod, declared in its files when the sign is; without it the
item places the template's wall sign. The wall form has no item of its own, and its sign's item is what picking
it gives. `wood_type` names the editor's background:
`ruby:elm` is `assets/ruby/textures/gui/signs/elm.png` (24x26) and `assets/ruby/textures/gui/hanging_signs/elm.png`
(16x16), and a bare name is vanilla's, such as `cherry`; without it the template's wood is used. The board is
the block's model: `minecraft:block/template_sign_rot_0` to `_rot_3` and `template_wall_sign`, and for hanging
signs `template_hanging_sign_rot_0` to `_rot_3`, `template_attached_hanging_sign_rot_0` to `_rot_3` and
`template_wall_hanging_sign`, with `all` and `particle` textures. List your blocks in vanilla's tags as well:
`minecraft:standing_signs`, `minecraft:wall_signs`, `minecraft:ceiling_hanging_signs` and
`minecraft:wall_hanging_signs`, which vanilla's other sign tags and `minecraft:mineable/axe` include. Give the
wall form a loot table of its own, or name it `<x>_wall_sign` so that it drops what `<x>_sign`'s table does.

A block can override its targeting outline with `shapes` and independently override movement
collision with `collision_shapes`. Both take the same table format. Boxes are
`[minX,minY,minZ,maxX,maxY,maxZ]` in block units (one block is 1), at most 16 per shape.
Coordinates must be finite and each minimum must not exceed its maximum. `[]` is an empty shape.

```json
{
  "template": "minecraft:stone",
  "state_properties": {
    "facing": { "type": "enum", "values": ["north", "south"] },
    "count": { "type": "enum", "values": ["1", "2"] }
  },
  "shapes": {
    "properties": ["facing", "count"],
    "values": { "north,2": [[0,0,0,1,0.5,1]] },
    "default": [[0,0,0,1,1,1]]
  },
  "collision_shapes": { "default": [] }
}
```

`properties` names an ordered tuple of template and/or declared state properties; each `values` key
lists their values in that order, separated by commas without spaces. A nonmatching tuple uses
`default`. Without `default`, every tuple must be covered. A default-only table gives every state the
same shape. The existing `{"property":"axis","values":{"x":[...],"y":[...],"z":[...]}}`
format still works; use either `property` or `properties`, not both. Unknown properties, unreachable
value tuples, malformed boxes and duplicate property names reject the mod with a load error rather
than silently ignoring a typo. Omitted `shapes` preserves the template outline (or the connecting
block's `thickness` outline). Omitted `collision_shapes` preserves the existing behavior: a declared
outline also becomes collision, otherwise the template collision is retained. An explicit collision
table can be used without an outline override, and overrides `thickness` collision too.

An item copies its template's data components, with `max_stack_size` and
`rarity` changed, and any other 26.3 component set or removed in
`components`, written as a data component patch is: `minecraft:<type>` sets
one, `!minecraft:<type>` with `{}` removes it. That is how a modded sword
gets its own damage and durability, or a tool its mining speed:

```json
{ "template": "minecraft:iron_sword",
  "components": {
    "minecraft:max_damage": 900,
    "minecraft:attribute_modifiers": [{ "type": "minecraft:attack_damage",
      "id": "minecraft:base_attack_damage", "amount": 7.0,
      "operation": "add_value", "slot": "mainhand" }],
    "!minecraft:repairable": {} } }
```

Native mods can register component types before content loads. A content item
can set a registered type with a JSON value, such as `"sample:charge": {"charge": 42}`.
JSON values become NBT values. Values must fit the network NBT limits and
round-trip through saved NBT; negative zero is not supported. A key such as
`"!sample:charge": {}` removes the template's default. Unknown types fail when Horizon builds the content root;
`cargo horizon-mod check` cannot check native registrations.

`crafting_remainder` names what the item leaves in the crafting grid when a
recipe uses it, as a milk bucket leaves a bucket (vanilla's
`Item.Properties.craftRemainder`, not a data component): a vanilla item or
one of the mod's own, refused at load otherwise. It holds on the crafting
table, the inventory grid, the crafter and as furnace fuel (the last of the stack leaves it in
the fuel slot, a stack with more left drops it), not yet in a brewing stand. What the item leaves once eaten
or drunk is the `minecraft:use_remainder` component instead:

```json
{ "template": "minecraft:mushroom_stew", "crafting_remainder": "minecraft:bowl" }
```

A block's item takes the same `components`, over the components of its
template block's item; a block with `"item": false` can't have them. Block files
have no `max_stack_size` or `rarity` keys, so those two go in `components` here
(an integer 1 to 99, and one of `common`, `uncommon`, `rare`, `epic`):

```json
{ "template": "minecraft:iron_block",
  "components": {
    "minecraft:max_stack_size": 16,
    "minecraft:damage_resistant": { "types": "#minecraft:is_fire" } } }
```

A block that joins its neighbours in six directions (a cable, a pipe) copies
`minecraft:chorus_plant`, vanilla's `PipeBlock`, and names the block tag it
connects to besides itself:

```json
{ "template": "minecraft:chorus_plant", "connects_to": "#mymod:cable_connects" }
```

It keeps the chorus plant's six sides but none of its rules: it needs no
support, and each side is joined when the block there is the same block or in
the tag, on placement and whenever that neighbour changes. Its shapes are the
chorus plant's (10 pixels across) unless it sets `"thickness"`, 2 to 14
pixels: its outline and collision are then built as `PipeBlock` builds its
own, a core and an arm to each joined side.

A mod's creative inventory tabs are `data/<id>/horizon/creative_tab/*.json`:
a title, the item on the tab's button, and its items in order (items the game
doesn't have are left out):

```json
{ "title": "Ruby", "icon": "ruby:ruby", "items": ["ruby:ruby", "ruby:ruby_block"] }
```

They come after vanilla's tabs, ten to a page as NeoForge pages them; the
buttons above the inventory turn the pages. The title is a lang key or English
text: `"title": "itemGroup.ruby"` uses that key from the mod's lang files, and a
lang file's `itemGroup.<mod id>.<file name>` (`creative_tab/gems.json` →
`itemGroup.ruby.gems`) translates any title. A mod's items join the search tab
under their names.

A component type no vanilla item has is refused, so a misspelt one fails
the load instead of doing nothing. Behaviour the game ties to a vanilla
item's id doesn't come along yet: a copy of the trident doesn't throw
([#3305](https://github.com/horizon-rust/horizon/issues/3305)).

A mod effect is declared in `data/<id>/horizon/mob_effect/<name>.json` with what vanilla's `MobEffect` takes: its
category (`beneficial`, `harmful` or `neutral`) and its RGB colour (0 to `0xFFFFFF`), which its particles take:

```json
{ "category": "harmful", "color": 9154528 }
```

It is the effect `<id>:<name>`: `/effect give @s ruby:glow 30` gives it, saves and mobs keep it, and it reaches LAN and
dedicated server players who run the same mods (its network id follows vanilla's). Its name is the translation
`effect.<id>.<name>` from the mod's `assets/<id>/lang/`, its icon `assets/<id>/textures/mob_effect/<name>.png` (18x18,
as vanilla's; without one it draws as the missing texture). It does nothing on its own yet: no tick behaviour and no
attribute modifiers. A world opened without the mod drops the effect from its players and mobs. Clients join a server
only with the same mods' effects, as with its blocks.

A mod entity type is declared in `data/<id>/horizon/entity_type/<name>.json`. It copies a vanilla type:
`minecraft:snowball`, a thrown item projectile, or a living mob (below). A thrown type's `item` is one of the mod's own
items (one type per item), and `damage` (0 by default) is what a hit deals to anything it hits, as `thrown` damage from
the thrower (the snowball's 3 to blazes is not copied):

```json
{ "template": "minecraft:snowball", "item": "ruby:ember", "damage": 2.0 }
```

It is the entity type `<id>:<name>`, with its network id after vanilla's. Using its item throws it as a snowball is
thrown: same speed, spread, sound and cooldown, and one item used outside creative. It flies as a snowball, breaks on
what it hits with its item's crumbs, and is drawn as its item's model (`ThrownItemRenderer`). Its name is the translation
`entity.<id>.<name>`. It works in single player and on LAN with the same mods. Dedicated servers share that code,
untested so far. Clients join a server only with the same mods' entity types, as with its blocks. It is saved with its
chunk, `/summon <id>:<name>` makes one, and it has its template's row in `entity-types.json`, with `template` naming
it. A world opened without the
mod keeps the saved ones as they are, frozen in place. Not yet: a dispenser doesn't throw it, and a vanilla client on a
server with it may be disconnected when one spawns.

A living type copies a mob, `minecraft:zombie`, `minecraft:villager` or `minecraft:wandering_trader`, the way NeoForge's
`EntityType.Builder.of(Zombie::new, ...)` makes a type of a vanilla class. The wandering trader is the neutral one whose
behaviour is all goals, so a native mod's [`register_goals`](native-mods.md#mob-goals) can replace all of it (a citizen,
a guard, an NPC); its trades on a right click are still a wandering trader's until a native mod answers the
interaction (`native_interact_entity`). A villager's behaviour is its brain, which stays. It has no `item` or `damage`. `attributes`
sets default attribute values that differ from its template's (`EntityAttributeCreationEvent`), by attribute id, and
only for attributes the template has; one it lacks stops the content root's build:

```json
{ "template": "minecraft:zombie", "attributes": { "minecraft:max_health": 40.0, "minecraft:movement_speed": 0.3 } }
```

It is a mob of its template's class under its own id, `<id>:<name>`: the template's size, model, texture, sounds and
behaviour, with its own attributes. It saves what its template's class saves under its own id (a zombie's `IsBaby`,
`CanBreakDoors` and conversion times) and loads back as itself. Its entity data is its template's class's. Its loot
table is its own, `<id>:entities/<name>` (vanilla's `EntityType.getDefaultLootTable`). It is not in vanilla's entity type
tags (`#minecraft:undead`, `#minecraft:zombies`...) unless the mod adds it. Its row in `entity-types.json` is its
template's, with `template` added and its attributes in place of the template's. Unit tests check all this; it has
not yet been tried in a running game. Not yet as data: its own model or texture (a native mod draws one with
`native_mob_submit`) and its own goals (a native mod gives it them with
[`register_goals`](native-mods.md#mob-goals)).

#### Menu types

A mod menu type (vanilla's `MenuType`) is declared in `data/<id>/horizon/menu/<name>.json`: its container's slot count
(1 to 256), the data slots its menu syncs (`ContainerData`, 0 to 256, 0 by default), and the slots players only take
from (`results`, as a furnace's result slot):

```json
{ "slots": 9, "data_slots": 2, "results": [7, 8] }
```

It is the menu type `<id>:<name>`, with its network id after vanilla's. A mod block's container opens it when its
`menu` names it with as many slots ([native-mods.md](native-mods.md), `native_block_entity_container`). The menu holds
its slots, then the player's 36. A shift-click from the inventory goes to the first slots with room, never a result
slot, and one on the container goes to the inventory, as a chest's does. `results` fence players only: hoppers follow the
container's faces and `inputs`, so leave result slots out of `inputs`. The data slots are the block entity's `data`
(16-bit on the network, as vanilla's), all sent when it opens, then each change. A Horizon client shows it through the
mod's own screen (`native_screen`); without one it closes again. Clients join a server only with the same mods' menu
types, as with its blocks, and a client that isn't Horizon's is never sent one: the block opens nothing for it. Its
clicks run on the server's vanilla menu engine (`menu::Kind::Mod`), shift-clicks routed as `ChestMenu`'s, and a native
mod can decide what a slot takes in code with `native_menu_slot_rules` ([native-mods.md](native-mods.md#menu-slot-rules)).
A native mod can hand the screen bytes of its own when the block opens it
([native-mods.md](native-mods.md#menu-extra-data)). Not yet: declared per-slot rules beyond `results`, hooks for
shift-click routing and `stillValid`, and client prediction of its clicks.

#### Namespaces and limits

The fence: files go in the mod's own namespaces (`assets/<id>/`,
`data/<id>/`, and those its manifest lists), plus additions to vanilla tags
(`data/minecraft/tags/`), to the convention tags NeoForge and Fabric mods
share (`data/c/tags/`), and sources added to vanilla's atlases
(`assets/minecraft/atlases/`), which follow vanilla's own as a reload appends them.
Templates and `sounds` name vanilla blocks; unknown keys are refused.

Some Java mods ship their files under other names than their mod id (Towns and Towers, mod id `t_and_t`, under
`kaisyn` and `towns_and_towers`). A content mod lists those in its manifest:

```toml
id = "t_and_t"
api = "0.2"
capabilities = ["registries"]
namespaces = ["kaisyn", "towns_and_towers"]
```

A listed namespace is the mod's as its id is: its `assets/` and `data/` files load, and the game reads its tags,
recipes, names and loot. Blocks, items, creative tabs, mob effects, entity types and menu types are declared in
`data/<id>/horizon/` only. `minecraft`, `c` and
`horizon` cannot be listed. Two mods that are on may not own the same namespace (one's id or listed namespace being
the other's): the game keeps the first in load order and leaves the other's content out, with "mods A and B both use
the namespace N; turn one of them off" in the log. Paths use vanilla's resource characters, at most 65,536 files, 16
MiB each and 512 MiB in all. A mod whose content is refused is left out, with the reason in the log. The game reads
this content into its block and item tables (`horizon_client::mod_content`, [modpacks.md](modpacks.md)). A tag entry
naming an id nobody registers (a block a port left out, another mod's item) is dropped, so the rest of the tag still
loads; vanilla would drop the whole tag.

Tags merge as a data pack's do. Every mod's file for one tag (vanilla's `data/minecraft/tags/`, the convention
`data/c/tags/`, or another mod's) adds its values after vanilla's and the mods' before it, in load order. A file with
`"replace": true` first drops what came before it, vanilla's entries and the earlier mods', and the mods after it
still add theirs, as a higher data pack does. Since the result depends on load order, the game says so in the log:
"mod towns replaces data/minecraft/tags/worldgen/biome/has_structure/village_plains.json: vanilla's entries and those
of adds are left out, ...". Tags are the only `minecraft` files a mod may hold: its other content stays in its own
namespaces.

Damage types use vanilla files at `data/<namespace>/damage_type/<name>.json`,
with required `message_id`, `scaling` and `exhaustion` fields, plus optional
`effects` and `death_message_type`. Resource-root files and enabled world packs
load when a world opens; an invalid definition refuses that load. Definitions
remain frozen for that world session. `/reload` rebuilds
`tags/damage_type/` against those definitions, so disabling a pack removes its
tag additions but keeps its registered types until the world is reopened.
Player damage, damage predicates and synchronized damage tags use that same
world snapshot; local catalog indices and network registry IDs are separate.

A mod adds to structure sets, template pools and processor lists that are not its own (vanilla's villages, another
mod's houses) with a set, pool or processor list file in its own namespace holding `horizon:add_to`, a key vanilla data
never has. Its entries are appended to each set, pool or list named, after every mod's own files, by
`horizon:priority` (lower first, 1000 when it has none), then in mod and file order; the file is no set, pool or list
itself. A name nobody defines is skipped.

```json
{ "horizon:add_to": "minecraft:villages",
  "structures": [ { "structure": "mymod:village_beach", "weight": 10 } ] }
```

A pool addition holds `"elements"` as a pool does, a processor list addition `"processors"`. This is what
Lithostitched's `add_structure_set_entries`, `add_template_pool_elements` and `add_processor_list_processors` modifiers
do on NeoForge and Fabric, and `horizon-port-pack carry` writes the last two from those.

A mod's own configured and placed features (`data/<ns>/worldgen/configured_feature/`, `placed_feature/`) and mob spawn
changes reach biomes it does not own (vanilla's) through NeoForge's biome modifiers, files in
`data/<ns>/neoforge/biome_modifier/` (older Forge mods: `data/<ns>/forge/biome_modifier/`, `forge:` types).
`add_features` appends its `features` to one generation `step` of every biome in `biomes`; `remove_features` takes them
out of its `steps` (every step when it lists none); `add_spawns` appends its `spawners` (one `{ "type", "weight",
"minCount", "maxCount" }` object or a list of them) to each entity type's category in
`minecraft:gameplay/natural_mob_spawns`, turning `minCount` and `maxCount` into 26.3's `count`; `remove_spawns` takes its
`entity_types` out of every category; `add_spawn_costs` sets `spawn_costs` (`{ "energy_budget", "charge" }`) for its
`entity_types`, and `remove_spawn_costs` removes them; `none` does nothing. `biomes`, `features` and `entity_types` are
an id, a `#tag`, a list of ids, or NeoForge's `any`, `or`, `and` or `not`. Every addition comes before every removal,
each in mod and file order, and a feature already in the step is not added twice.

```json
{ "type": "neoforge:add_features", "biomes": "#minecraft:is_overworld",
  "features": "zinc:ore_zinc", "step": "underground_ores" }
```

The modifiers rewrite the biomes when the content root builds, and the generator orders every biome's features as
vanilla's `FeatureSorter` does, so an added feature takes its place in that order (its random seed follows from it). A
biome no modifier names keeps its text, and a world without mods keeps vanilla's biomes byte for byte. A modifier
naming a biome, biome tag, feature, feature tag, entity type or entity type tag the game doesn't have (another mod's, a
`#c:` convention tag), or of a type not read yet (`add_carvers`, climate, a mod's own serializer), is skipped with a
warning naming the mod and file, as NeoForge skips what it can't resolve; the mods still load. A malformed modifier of a
type that is read (an unknown step, a missing field, `minCount > maxCount`) stops the build, and the game starts without
content mods ("content mods not loaded: ..." in the log). Feature tags are vanilla's for now (`#tag` in `features`
doesn't read a mod's `placed_feature` tags yet); a world's data pack that replaces a biome replaces the mods' changes to
it too.

A pool element may be one of Lithostitched's: `lithostitched:limited` (`delegate`, `limit`, `min_depth`) places its
`delegate` at most `limit` times in one structure start, and `lithostitched:guaranteed` (`delegate`, `count`,
`min_depth`) is tried before its pool's other elements at each jigsaw that reaches that pool (as target or fallback)
until `count` are placed. Neither goes below a piece shallower than `min_depth` (the start is at depth 0).

A structure (and any of its pool elements, in place of the structure's) may hold
`horizon:enhanced_terrain_adaptation`: MoogsStructureLib's terrain kernel, which carves or buries terrain around the
pieces on top of vanilla's `terrain_adaptation`. `kernel_size` and `kernel_distance` shape the kernel; `top` and
`bottom` (`carve`, `bury` or `none`) act above and below a piece's floor; `bottom_offset`, `padding` (`x`, `top`,
`bottom`, `z`) and MoogsStructureLib's `band` (`bottom`, `top`, `piece_heights`) move the box it measures from. On a
structure, `rigid_junctions` adds rigid pieces' junctions as well and `inflate` grows the start's box by the kernel
radius. `kernel_size` 0 is none.

```json
{ "horizon:enhanced_terrain_adaptation": { "kernel_size": 24, "kernel_distance": 24,
    "top": "carve", "bottom": "none", "rigid_junctions": true, "inflate": true } }
```

## Native mods

Behaviour that needs code (a new key, a HUD, a tool that changes how mining works) is a native mod. It is a bundle of
a `horizon-mixin.toml` and a few `.rs` helpers that hook into named functions of the game. Start from
[horizon-native-mod-template](https://github.com/horizon-rust/horizon-native-mod-template). How the launcher builds
one, what it pins and why it is not sandboxed: [native-mods.md](native-mods.md).

| Interface | Capability | Functions |
|---|---|---|
| `log` | always | as for client mods |
| `storage` | `storage` | as for client mods, in the server's `horizon-mods/` |
| `players` | `players` | `online()`: id, name, position, dimension, game mode, health of each player on; `tell(id, text)`, `broadcast(text)`: system messages after the mod's name, 8 per call |
| `world` | `world` | `get-block(dimension, x, y, z)`: the state in a loaded chunk, `minecraft:oak_log[axis=y]`; `get-biome(dimension, x, y, z)`: `minecraft:plains`, ...; `set-blocks(dimension, changes)`: up to 4096 per call, checked at once, made as `/setblock` makes them (neighbours, light, block entities, every player told) after the call, before that dimension's next tick |
| `entities` | `entities` (experimental) | `mobs()`: the content mods' mobs loaded on the server (id, kind, dimension, position, yaw, health, target, age, scale, animation, texture, rider, data), up to 1024; `near(dimension, at, radius)`: every mob within 64 blocks, nearest first, up to 256; `interactions()`: the last tick's right clicks on content mods' mobs (player UUID, mob id, kind, held item), in order, up to 256; `give-orders(orders)`: up to 1024 orders to content mods' mobs, checked at once, carried out before their dimension's next tick (below); `spawn(kind, dimension, at, yaw, data)`: a content mod's mob, persistent, with data on it, before that dimension's next tick, up to 64 per hook call |

| Hook (export `server-events`) | When |
|---|---|
| `init() -> result<registration, string>` | once, after loading |
| `tick(tick)` | each server tick |
| `player-joined(player)`, `player-left(player)` | `registration.joins` |
| `chat(player, text)` | `registration.chat`: each chat message a player sends |
| `command(player, name, args, values)` | `/name args` typed by a player, for a command it registered, with its arguments' values (below); `ok(some)` is that player's feedback, `err` their red error |
| `block-broken(player, pos, state)`, `block-placed(player, pos, state)` | `registration.blocks`, with the state as `minecraft:oak_log[axis=y]` |

Capabilities that act on a local player (`game-read`, `chat`, `hud`) are for
client mods; `players` is for server mods. In Rust: `impl
horizon_mod_sdk::server::ServerMod` and `export_server_mod!`; the example is
[`welcome`](../crates/horizon-mod-sdk/examples/welcome/src/lib.rs), and with
`world`, [`beacons`](../crates/horizon-mod-sdk/examples/beacons/src/lib.rs), and with
`entities`, [`mob-orders`](../crates/horizon-mod-sdk/examples/mob-orders/src/lib.rs).

**Typed commands.** Each command in `registration.commands` lists its
arguments, in order: a name and a kind, required or optional (optional ones
last). The server adds them to the command tree every player receives, after
vanilla's commands, so a player's chat box completes and colours them as it
does vanilla's (player names, coordinates, bounds); when a player sends one,
the server parses the line the way vanilla's parsers do and gives the mod the
values, by argument name, or answers with vanilla's error (`Integer must not
be more than 64, found 65`, `No player was found`) without calling the mod.

| Kind | Takes | Value | Chat box parser |
|---|---|---|---|
| `word` | one word: letters, digits, `_ - . +` | `word(string)` | `brigadier:string` (word) |
| `text` | the rest of the line, only last | `text(string)` | `brigadier:string` (greedy) |
| `integer(min, max)` | a whole number within the bounds given | `integer(s32)` | `brigadier:integer` |
| `number` | a number | `number(f64)` | `brigadier:double` |
| `boolean` | `true` or `false` | `boolean(bool)` | `brigadier:bool` |
| `player` | an online player's name, `@s` or `@p` (whoever typed it) | `player(player)` | `minecraft:entity` (one player) |
| `block-pos` | `x y z`, each a whole number, `~` or `~n` (from whoever typed it) | `block-pos(block-pos)` | `minecraft:block_pos` |

A command with no arguments takes nothing after its name. In Rust,
`Registration::command_with("beacon", [Argument::integer("height", Some(1),
Some(64)).optional()])`, read with `args.integer("height")`; `command(name)`
alone takes free text, an optional `text` argument named `args`
(`args.text()` is the line as typed either way).

**API level 10: feeding and specific players.** Capability `players` adds
`take-item(player, item, count) -> result<u32, string>` and
`give-item(player, item, count) -> result<_, string>`. Players are UUIDs;
items are registry ids (`minecraft:beef`, `ruby:scale`), validated against
vanilla and loaded content mods. Unknown items and offline players are errors.
Taking removes up to `count` by item id, main hand first, then the remaining
inventory slots, armor and off hand, and returns the actual number removed.
Creative removes nothing and reports `count`, as vanilla feeding does.
Calls share the real inventories: repeated takes, even from different mods
in the same tick, cannot consume the same item twice. Giving merges matching
plain stacks, fills empty inventory slots, and drops overflow at the player's
feet before that dimension's next tick. Each give accepts at most 100 stacks,
as `/give` does. Changed inventories are sent to their clients. These calls
work for the in-frame player and every connection on the server thread, LAN
and dedicated servers, including players in other dimensions.

`entities.interactions()` now reports `sneaking` at the click and `hand`
(`main_hand` or `off_hand`). `target(player(uuid))` chooses that online
player, even if another is nearer; normal targetability still applies.
Unknown/offline UUIDs reject the order batch; an order whose player leaves
or is in another dimension when applied is dropped. Target choices are
transient, like the existing target order. Rebuild experimental server mods
against the changed interaction and target types.

The `mob-orders` example demonstrates `/feed` (one beef for the nearest mob),
`/gift <item> <count>`, and `/attackme`. A sneak-click with a bone labels
`owner` and `tamed`; a click with beef consumes one beef with `take-item`
and heals the mob. It respects creative's free feeding.

The resource-free checker and script driver use shared inventory counts,
validate item identifier syntax, and have no slot capacity; the running game
checks registry membership, per-item stack limits and overflow drops.

**Capability `world`.** API level 9 adds read-only `game-rule(name) ->
option<string>` and `difficulty() -> string`: the server's current rule values
(`"true"`, `"false"`, or a decimal integer), and `peaceful`, `easy`, `normal`,
`hard`. Unknown rules return none. Use 26.3 names, optionally prefixed with
`minecraft:`; `mobGriefing` also reads `mob_griefing`. They are refreshed before
each hook or command call, including rules or difficulty changed by commands.

`time(dimension) -> world-time` reads the shared Overworld `day-time` (not
modulo 24000), `game-time`, and the dimension's `is-day`, `raining`, and
`thundering`. `is-day` follows `Level.isBrightOutside`: sky darkening below 4,
with weather taken into account, and false in fixed-time dimensions (Nether
and End). Weather flags use vanilla's rain and thunder level thresholds;
the Nether never reports thunder. Unknown dimensions return zero times and
false flags; known dimensions need no loaded chunks.
`can-see-sky(dimension, x, y, z) -> bool` reads raw sky light 15, independent
of time of day and block light. It respects dimension build heights and is
false for unknown/unloaded chunks and dimensions without skylight. Neither
query loads chunks or changes the world.

Reads are of the server's loaded chunks as they are
during the call: the game lends a server mod's call the levels' chunk maps
(moved, not copied, and only while a running mod has `world`), so a read
costs a map lookup and no copy of the world. A dimension with no chunk loaded
at a position reads as none. Writes go through the same code as `/setblock`,
so block updates, light, block entities and every client (LAN guests,
dedicated server players) see them as vanilla's; the parity harnesses test
that code. Changes for a dimension whose level isn't ticking wait for it
(in-frame single player ticks only the player's), up to 65,536 blocks; a
change in a chunk that isn't loaded when it is made is skipped, as `/setblock`
refuses one.

API level 11 adds `players.facing(player)` (where an online player looks: yaw and pitch)
and `world.generated-chunks()`: the chunks the server generated for the first time during
the last tick (never ones loaded from a save), at most 256, oldest first, loaded when the
mod reads them, so a mod places its own structures in new terrain once per chunk.

**API level 12: item behaviours, effects, containers and templates.** These are functions
mods call and new records; existing player and entity records and the hooks the game calls
keep their shapes. The `give-orders` host accepts earlier order variants by case name,
so adding `effect` preserves components compiled against the previous order list.

Capability `players` adds two lists, each capped at 256 entries per tick, oldest first.
`item-uses() -> list<item-use>` reports last tick's right clicks with an item, including
clicks in air and on blocks. An `item-use` contains `player` (UUID), `item` (registry id),
`hand` (`main_hand` or `off_hand`), `sneaking`, and `target: option<block-hit>`; a block
hit has integer `x`, `y`, `z` and `face`. `attacks() -> list<attack>` reports melee hits
players landed. An `attack` has `player`, `item: option<string>` (none for an empty hand),
`mob: option<u32>` (the id from `entities.mobs()`), `target-player: option<string>`
(UUID), and `damage: f32` (actual health lost). Polling does not consume a list. The game's own item use or
attack still happens. The lists include every server player, including single player's
connection to its server thread, LAN guests and dedicated server players.

`players.add-effect(player, id, seconds, amplifier) -> result<_, string>` adds a status
effect to an online player and sends it to that player's client. The entity order
`effect(effect-change)` applies the same operation to a content mod's mob;
`effect-change` contains `id: string`, `seconds: u32` and `amplifier: u8` (zero is level I).
Effect ids, for example `minecraft:slowness`, must exist in the game's effect registry.
Zero seconds removes that effect. Unknown ids reject the call or order batch.

Capability `world` adds `container(dimension, x, y, z) ->
option<list<option<item-stack>>>`: the slots of a loaded block container (chests,
barrels, furnaces, hoppers and other supported containers), or none when there is no
container there. Each chest block exposes its own 27 slots, including each half of a
double chest. Each `item-stack` has `item: string` and `count: u32`; none is an empty
slot. These are **plain items: components are not carried**. Reading does not expose
enchantments, names or other components, and writing replaces stacks with plain items.
`set-container(dimension, x, y, z, slots) -> result<_, string>` requires the container's
exact slot count and valid item ids and counts. Changes run after the tick's block
writes, and players viewing an open container receive its changed slots. If the
container is absent or its slot count differs when the queued write runs, the write
is skipped and logged.

`world.place-structure(dimension, id, x, y, z, rotation, mirror) -> result<_, string>`
loads a vanilla or loaded content mod's `data/<namespace>/structure/<path>.nbt`
template (`minecraft:igloo/top`, for example). `rotation` is 0 through 3 quarter turns;
`mirror` reflects the template. Placement reuses the game's template code after
the tick's block writes, including block entities and their loot tables: generated
chests retain `LootTable` and roll it as other generated chests do. Template entities
follow the game's existing template placement support. Only loaded chunks are written.

**API level 13: spawners.** `world.set-spawner(dimension, x, y, z, entity) -> result<_, string>`
sets the mob a monster spawner (`minecraft:spawner`) spawns, as a spawn egg used on it does
(`BaseSpawner.setEntityId`): vanilla's entity types or a content mod's. It runs after the
tick's block writes, so a mod can place a spawner with `set-blocks` and give it its mob in
the same tick, as a Java structure processor rewrites a template spawner's `SpawnData`. The
spawner keeps its delays, spawn count and ranges; players see the new mob in it. No spawner
there then, or an unknown type: the write is skipped and logged.

**API level 15: combat, players and world helpers.** All additions are functions;
existing hooks and records keep their ABI. `entities.strike(mob, hit)` extends hurt-area
with skipped entity types, extra knockback (0..4, applying resistance), and optional online
player attack/kill credit. It hits living mobs and players in the ball, sparing its source
(a content mob), skipped types and credited player. Radius <=16, damage <=1024, fire <=30 s;
at most 256 skipped type ids. WIT requires distinct function/type names: the record is
`strike-hit`. Knockback and fire also apply when damage is zero, as with hurt-area's fire.
`players.knockback(player, strength, dx, dz)` applies vanilla knockback with resistance;
the direction points away from the attacker. `players.push(player, velocity)` **adds**
velocity, each component -4..4 blocks/tick (the mob push order sets it).
`world.play-sound(dimension, at, name, volume, pitch)` plays a registered vanilla or
loaded content sound; positions, nonnegative volume and pitch must be finite. World
writes wait for their dimension; mob writes wait for their mob's level. Player calls run
as the server player and sync motion to clients. Capabilities are `entities`, `players`
and `world`, respectively; existing manifest rules apply. Each interface owns its
point record so importing `players` or `world` does not also require `entities`;
the Rust SDK re-exports these as `PlayerPoint` and `WorldPoint`.
`players.damage-item(player, hand, amount)` uses held-item durability with Unbreaking
and creative immunity; `hand` is `main_hand` or `off_hand`. Empty or nondamageable
items do nothing. Broken items disappear, with break sound and item particles, and
server inventories sync. Durability comes from the item's registered max_damage;
this function does not give durability to a plain stick template.
`entities.tame(mob, owner)` accepts any loaded tameable runtime species, including
content templates: equines, wolves, cats, parrots and nautiluses. An optional owner
is a UUID (need not be online); none keeps the current owner. Other mobs error.
`world.block-has-tag(block, tag)` reads the game's loaded block tag tables. A block
id or valid state string is accepted; unknown blocks/tags or a tag prefixed by #
return false.
`players.equipment(player)` reads authoritative armor, selected hand, off hand and
exactly nine hotbar slots as optional registry ids; offline players return none.
The new record is `equipment-slots` because WIT forbids sharing a function's name.
`world.spawn-item(dimension, at, item, count)` queues a dropped plain stack for that
level. The item must exist, count must be 1..its maximum stack size, and the point
must be finite. Each mod can queue at most 16 per game tick across all hooks;
invalid requests do not spend its budget. Drops use the normal pickup delay.
`entities.effect(mob, change)` and `entities.push(mob, velocity)` accept **any** loaded
mob id from `near`, including vanilla mobs. They use the effect/push order behavior:
registered effect ids, zero seconds removes, and push **sets** velocity (-4..4/axis).
`entities.hurt-events()` and `entities.deaths()` return shared, nonconsuming lists
from the last tick, at most 256 of each, with content mobs as victims only. Hurt
amount is actual health lost after armor and hurt cooldown; hurt events name the
victim's type and a causing mob id or attacking player's UUID. Player attribution
includes melee, projectiles, thorns and command damage; thrown and lingering potions
retain the thrower's UUID even after that player leaves. Sources without a native
owner return none. Death positions are
captured on lethal damage, before corpse movement; other native deaths are recorded
when the game's death dispatcher observes them. Discard is not a death.
`players.damage-taken()` similarly returns at most 256 actual health-plus-absorption
losses after armor/effects, with damage type id and causing mob when known. Events
include environmental damage and server guests; every mod sees the same tick.
There was no existing player heal import. `players.heal(player, amount)` reuses the
game's potion healing action, capped at max health and never reviving the dead;
amount must be finite, 0..1024. These are polling functions, with no new hooks.

**API level 14: mobs riding mobs.** `entities.ride(rider, vehicle) -> result<_, string>` seats a
content mod's mob on another loaded mob, vanilla's or a mod's (`Entity.startRiding`, as `/ride`
does), and `ride(rider, none)` lets it off (`stopRiding`). The call checks that both are loaded
in one dimension within 16 blocks, that they differ, and that the vehicle does not already ride
the rider (directly or through its own vehicles); at most 64 a call. It is carried out with the
tick's orders, unless by then the vehicle carries another mob or a player, or either is gone or
dead. The rider then sits on the vehicle as a jockey does: while the rider has a target (a
`target` order), its vehicle carries it there (`getControllingPassenger`); it is saved riding
(`Passengers`), never despawns while riding, and every player sees it seated.
`entities.vehicle(id) -> option<u32>` is the mob or boat a loaded mob rides, as at the start of
the tick.

The `mob-orders` example demonstrates `/uses` (last tick's item uses), `/burn` (ten
seconds of wither I on the nearest content mod's mob), and `/peek x y z` (the occupied
slots and slot count of a container).

API level 9 adds the texture, sound, particles, attribute and heal orders and the entity record's
`texture: option<string>`. The `mob-orders` example demonstrates `/paint
<texture|none>`, `/roar` (sound and flame particles), and `/info` (including
`mobGriefing`, time and sky visibility), `/attr <name> <value>`, and `/heal <n>`. This experimental interface's changed entity and order types
require rebuilding server mods against this SDK.

**Capability `entities`.** The behaviour of content mods' mobs
([modpacks.md](modpacks.md), entities): a server mod reads the mobs as they
were at the start of the tick and gives the content mods' own orders, which
the game carries out in order before that mob's level ticks again (orders to
a mob in another dimension wait for its level, up to 16,384; those to a mob
gone by then are dropped):

| Order | Does |
|---|---|
| `walk-to(walk)` | walks to a point at a multiple of its speed (0 to 4) by its path navigation, ahead of its own goals, until it arrives or has no path |
| `halt` | ends walking and looking orders: its own goals take over |
| `look-at(point)` | keeps its head turned to a point |
| `push(point)` | sets its motion, -4 to 4 blocks per tick on each axis: flight, leaps |
| `gravity(bool)` | whether gravity pulls it |
| `animate(option<string>)` | plays an animation of its own model's file (docs/modpacks.md) from its start, once, looping or held at its end as the file says; none goes back to idle, walk and attack. Players' games are told on the `horizon:entity_animation` channel, with how long it has played, so one that comes into view joins in where it is |
| `texture(option<string>)` | overrides the mob's texture with `<namespace>:entity/<path>` from a loaded mod's assets; none restores its type's. All variants under `textures/entity/` are available, including ones no entity JSON names. Saved as `horizon:texture` and synced on `horizon:entity_texture` when tracking starts and on changes |
| `sound(sound)` | plays a vanilla sound event or a loaded mod's `sounds.json` event at the mob (`name`, finite nonnegative `volume` and `pitch`), sent as vanilla's sound packet within `16 * max(1, volume)` blocks |
| `particles(particles)` | a vanilla particle type with no options (`name`, `at`, `delta`, finite nonnegative `speed`, `count` at most 256), sent as vanilla's level-particles packet within 32 blocks in the mob's dimension; zero count sends one with velocity `delta * speed`. Names are checked now; unknown sounds and particles, or particles needing options, reject the entire batch |
| `hurt-area(area)` | every living thing but the mob within a radius (up to 16) of a point takes up to 1024 hit points as the mob's attack (`mob_attack`) and burns up to 30 s: a breath, a stomp. Players too, LAN guests included |
| `discard` | removes it, with no death and no drops |
| `target(target)` | whom it attacks with its template's own attacks: `none`, `mob(id)`, `nearest-player` (the player it sees), or API level 10's `player(uuid)` |
| `scale(f32)` | its `minecraft:scale`, 0.0625 to 16: growth. Its box and model follow on every player's game, and it is kept in its save |
| `attribute(attribute-change)` | sets a base value (`name`, `value`, finite 0..1024): `max_health`, `movement_speed`, `flying_speed`, `attack_damage`, `attack_knockback`, `armor`, `armor_toughness`, `knockback_resistance`, `follow_range`, or `step_height`. Vanilla bounds still limit effective values (e.g. armor at most 30). Stored in vanilla's `attributes` save list and synced where vanilla syncs the attribute. Raising max health does not heal; lowering it clamps health |
| `heal(f32)` | `LivingEntity.heal`: adds a finite amount to positive health and clamps to 0..max health; never revives dead mobs. Negative amounts follow vanilla and reduce health |
| `mount(mount)` | seats an online player (`player` UUID, in its dimension, within 16 blocks, not riding) on it, with their feet `height` (-16 to 64) above its feet and `forward` (-32 to 32) ahead along its body's yaw; a later `mount` for the same rider moves the seat. The seat is synced on `horizon:entity_seat`. The player gets off by sneaking, as from any mount. The mob does not move for its rider: `mobs()` reports the `rider` (UUID, forward, strafe, jumping, sprinting, yaw, pitch) each tick, and the mod moves it (`push`, `gravity`) |
| `dismount` | lets its rider off beside it |
| `set-data(entries)` | strings kept on it and in its save: a key (1 to 64 characters) to a value (up to 1024), none removes it; 32 keys at most. `mobs()` reads them back, and a mod tells its spawns apart by them |

Walk and look orders are a goal of the highest priority on every content
mod's mob (`mobs/orders.rs`): while one stands, the mob's own goals that move
or look wait, and so does a brain (villagers, axolotls, frogs, breezes,
wardens, ...), which would steer the mob after the goals: it picks up again
when the order ends. Piglins' and hoglins' brains still steer them. The
animation an order plays and the data are kept in the mob's save
(`horizon:animation`, `horizon:data`). Taming and riding are not orders yet.
`interactions()` lets a mod build taming with `set-data` (`owner` = player UUID)
and `walk-to`: the game's own use of the click still happens, and reading
the interaction consumes no items; a mod uses `players.take-item` to feed it.

**Where they run.** The game that simulates the world starts the server mods
that are on, and only that one:

| Mode | Runs server mods | Players they see |
|---|---|---|
| Single player (default) | the server thread | the window's player, a connection like any other |
| Single player `--in-frame` | the window's own game, beside its client mods | the window's player |
| Open to LAN, Play with Friends | the host's server thread (or its window, in-frame) | the host and every guest |
| Dedicated server (`--dedicated`, `horizon-server`) | the server | every connected player |

A window that is a client of a server (single player's window, a LAN guest,
any multiplayer server) never runs them; its Mods page lists a server mod with
the others, and its switch takes effect the next time a world opens. The mods
live in `horizon-mods/` of the game folder, or of the server's folder on a
dedicated server, and are turned on as client mods are: Options > Mods, or
`mod.<id>:true` in `horizon-mods.txt`. `--dev-mod <file>` runs one always
on and reloads it when its file changes, on a server too.

**When hooks run.** At the end of each server tick the game tells the server
mods who joined and who left since the last one, then what players said,
broke and placed, in order, then calls `tick`. A command a player types
reaches its mod at once, whatever the player's permission level, and its
answer goes to that player. `tell` reaches one player, the window's own
chat for the in-frame player and a system message for a connection;
`broadcast` reaches every player and the server's log. With no server mod
on, each of these points costs one branch (measured in the PR that added
them).

**Not yet.** Block events are a player's own breaking and placing, not
explosions, pistons or other mods. Command arguments don't take entities,
items or block states yet, and `^` coordinates; in-frame single player's chat
box lists server mods' commands by name only (the server still parses their
arguments).

## Publish your mod

Horizon's registry lists mods, modpacks (`.hpack`) and resource packs made for Horizon. **A project is reviewed
once, when it is listed; its later releases are checked by machine, not read by a person.** Players find them at
[playhorizon.dev/mods](https://playhorizon.dev/mods) and in the launcher (Mods, then **Horizon** beside Modrinth),
which checks every file before it installs it.

1. Run `cargo horizon-mod check` on the file (above): the registry runs the same checks.
2. Put the source in a public GitHub repository, under a licence that lets players run it. A native mod's `.hnmod` is
   its source.
3. Publish a **GitHub release** in it with the file as an asset (`.hmod`, `.hnmod`, `.hpack`, `.hshader`, or a resource
   pack's `.zip`), and its `.minisig` beside it if you sign. The release notes are the changelog.
4. List the project once: open [playhorizon.dev/mods/submit](https://playhorizon.dev/mods/submit), sign in with GitHub
   and give the repository and the page (name, summary, licence, optionally a minisign public key). It opens a pull
   request in [horizon-rust/horizon-mods](https://github.com/horizon-rust/horizon-mods) adding `mods/<id>.toml`, with
   you as owner. A reviewer reads the repository and its source and merges it.

**Releasing a version is only step 3.** Publish a GitHub release (not a draft or a pre-release); within the hour the
registry reads it, runs the checks a submission runs (the file's own, the signature, the id and the version in the
file's manifest), and lists it. A release that fails is skipped and reported in an issue of the registry, never
published. Nobody opens a pull request and nobody reviews it. The version is the one in the file's manifest (the tag
when the file has none), the date and the changelog are the release's, the SHA-256 is computed by the registry, and
the launcher downloads the asset from your repository and refuses any other bytes. A published file never changes:
publish a new version. `revoked.toml` takes a bad one down.

Once it is published, its page and its card in the launcher show its rating: players signed in with GitHub on its
page give it 1 to 5 stars, one rating per account, which they can change. Its owners can't rate it, and nobody sees
who rated.

Your page can live in your repository too: a `horizon-registry.toml` at its root (on the newest release's tag) with
`name`, `summary`, `description`, `authors` and `license` fills what the registry's entry leaves out; the entry's own
words win. Its owners, key, kind and icon are the registry's, changed by a pull request. Projects listed before this
(`versions` in their entry) keep their hosted versions. The formats: [api/mod-index.md](api/mod-index.md).

### Integrity and signing

| Mod | Id | Replaces | Settings (storage key: values) |
|---|---|---|---|
| FPS Counter | `fps_overlay` | Extras > FPS counter | `corner`: `top_left` (default), `top_right`, `bottom_left`, `bottom_right`; `lows`: `false` (default), `true` |
| Keystrokes | `keystrokes` | Extras > Keystrokes | `corner`: as above, default `top_left`; `cps`: `true` (default), `false` |
| Zoom | `zoom` | Extras > Zoom | `level`: `2`, `3`, `4` (default), `6`, `8`, `10`; `smooth`: `true` (default), `false`; `sensitivity` (slower turning): `true` (default), `false`. The key is the game's: `key.zoom.zoom` in `horizon-mods.txt`, default `key.keyboard.z` |
| Minimap | `minimap` | Extras > Minimap | `corner`: as above, default `top_right`; `size`: `64`, `96` (default), `128`; `shape`: `square` (default), `round`; `zoom` (blocks per pixel): `0.5`, `1` (default), `2`, `4`; `rotate`: `false` (default), `true`. The game draws the map (`render.set-minimap`) |
| Far View | `far_view` | Graphics > LOD Distance and Far Detail | `distance` (chunks): `64`, `128`, `256` (default), `512`, `1024`; `detail`: `full` (default), `reduced`. The game builds and draws the distant terrain (`render.set-far-terrain`); the player's LOD Distance, when on, wins |
| Recipes | `recipes` | nothing (new) | `give` (a click on the item panel gives the item in creative mode: one, a stack with a right click): `false` (default), `true`. The keys are the game's: `key.recipes.recipe` (default `key.keyboard.r`) and `key.recipes.uses` (default `key.keyboard.u`) in `horizon-mods.txt`. Every item sits in the item panel (`browser`) with a search box (words in the name or id, `@namespace`, `#tag`); R or a click shows an item's recipes, U or a right click its uses, a tab per station, and + fills the grid with a recipe the player knows (`recipes.place`). On servers that are not Horizon it shows the game's own 26.3 recipes; known recipes and + follow the server's recipe book |
| Block Info | `block_info` | nothing (new) | `position`: `top` (default, centred), `top_left`, `top_right`; `distance` (blocks): `4.5` (default, reach), `16`, `64`; `source` (the game or mod the block comes from): `true` (default), `false`; `coords`: `false` (default), `true`. What the crosshair is on, as Jade and WTHIT show it: the block's icon, its name in the player's language (`recipes.items`) and its namespace, from `game.raycast`. Living entities (including players and content mobs) show their display name and current/max hit points through `game.raycast-entity`, in the same panel style |

**API level 16: the entity under the crosshair.** Capability `game-read` adds
`game.raycast-entity(distance) -> option<entity-hit>`. It reads the nearest loaded
living entity along the view, before any block, within the requested distance
(at most 1024 blocks). It returns none outside a world or when no entity is hit.
The new result contains the namespaced type id, display name (player name,
custom name, or localized type name), feet position and current/maximum hit
points. Mobs, animals, players and content mods' mobs use their live instances;
maximum health includes runtime attribute changes. On servers these are the
values synchronized to the client. Existing records and `game.raycast` are
unchanged. Block Info uses this call for entities and its existing block raycast
for blocks; its position, distance, source and coordinate settings apply to both.

**API level 17: pixels and the loaded world.** Capability `render` adds the
`canvas` interface (a mod's own images in a corner box, a frame budget and
`work-left`), and the new experimental capability `world-read` the
`client-world` interface (whole chunks as columns, change counters, the
packs' block table and textures); `game-read` adds `game.world-id`. They are
new interfaces and functions only: every earlier record, variant and world
is unchanged, so mods built before them load as they did, and a mod that
uses them is refused by an older Horizon with "made for a newer Horizon".
The SDK example `canvas-test` paints a gradient into a round box and says the
top block under the player. Images live on the HUD atlas's mod-image page
(2048 × 2048 RGBA, 16 MiB), which the game keeps while a client mod that is on imports
`canvas` (or a native mod is built in; the two share it, so a native mod's
`native_mod_image` may cover canvas images). A chunk's change counter moves
when a block in it is written, when it loads, is sent again or unloads; light
or biome updates alone don't move it.

**API level 18: placed boxes and canvas screens.** `canvas.set-placed(anchor,
dx, dy, width, height, shapes)` places the mod's box at GUI pixel offsets inward
from any corner, independently of the overlay stack. Offsets are clamped after
window resizes; oversized boxes are clipped to the window. `set-corner`,
`set-placed` and `clear-corner` replace or remove the same box (up to 1024 shapes,
hidden with F1). Both box calls keep sizes between 0 and 1024 GUI pixels.

With both `ui` and `render` declared (including their experimental opt-ins),
`canvas.open-canvas-screen(title, shapes)` opens a full-window surface;
`set-canvas-screen(shapes)` replaces its shapes and `close-canvas-screen()`
closes it. The screen holds up to 4096 shapes, in GUI pixels from the window's
top left, clipped to its bounds, over the usual dimmed menu background. Its
owner's HUD canvas box is hidden while it is open. It does not pause the game,
and the mod's registered `frame` hook keeps running. Escape closes it and calls
`ui-events.closed`; a programmatic close does not send that event.

Canvas screens receive optional `ui-events.pointer(kind, x, y, button)` (`down`,
`up`, `drag`, `move`; buttons 0 left, 1 right, 2 middle), `scroll(x, y, steps)`
(positive away from the player), and `screen-key(key, down)` (`key.keyboard.*`,
including `key.keyboard.left.shift`). Escape belongs to the game. The SDK's
`Mod` trait provides `ui_pointer`, `ui_scroll` and `ui_screen_key` defaults. Level-17
components may omit all three exports and retain their original UI callbacks.
iOS menu touches follow the existing mouse path as button 0; a held touch drags
with that button. The menu touch layer has no long-press right-click equivalent.
Images may now be 1024 texels per side, with a total of 1024 × 1024 texels per
mod and 1 MiB of updates per frame. The shared page remains shared with native
mods' `native_mod_image`.

`canvas-test` shows `set-placed` and opens a screen with M (or `/canvas`). The
scenario runner used by `cargo horizon-mod test` also accepts canvas input:

```toml
[[step]]
command = "canvas"
expect.canvas-screen = { open = true, title = "Canvas test", shapes = 1 }
expect.canvas = { placed = true, dx = 12.0, dy = 20.0, x = 347.0, y = 20.0 }

[[step]]
pointer = { kind = "down", x = 40.0, y = 50.0, button = 0 }
scroll = { x = 40.0, y = 50.0, steps = 1.0 }
screen-key = { key = "key.keyboard.left.shift", down = true }

[[step]]
screen-key = { key = "key.keyboard.escape", down = true }
expect.canvas-screen = { open = false }
```

Events run after a step's command, before assertions. `expect.canvas` can check
`placed`, inward `dx`/`dy` and clamped `x`/`y` (the runner's GUI is 427 × 240),
beside its existing corner, size, shape/image and round-image counts.
`expect.canvas-screen` checks `open`, `title`, shape count and `texts` (the text
shapes in drawing order).

## Horizon's official mods

Horizon's own mods are native. They live in
[horizon-rust/horizon-mods](https://github.com/horizon-rust/horizon-mods) under `native/<name>/`:

| Mod | Folder | Replaces |
|---|---|---|
| FPS Counter | `native/fps-overlay` | Extras > FPS counter |
| Keystrokes | `native/keystrokes` | Extras > Keystrokes |
| Zoom | `native/zoom` | Extras > Zoom |
| Minimap | `native/minimap` | Extras > Minimap |
| Recipes | `native/recipes` | The Recipes mod (item panel and recipe pages, on hook points version 2) |
| Coordinates HUD | `native/coords-hud` | A small example mod (position and facing, `/where`, `/coords`) |

The game keeps the parts that must be fast, such as drawing the minimap's map, and a native mod picks the options
([extras.md](extras.md)). Far View is not a mod any more: its distance and detail are Graphics > LOD Distance and Far
Detail ([graphics.md](graphics.md)).
