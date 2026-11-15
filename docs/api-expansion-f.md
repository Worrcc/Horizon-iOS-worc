# Phase F: API level 36 world seed and column tops, with three engine fixes

API level **36** adds two imports to the server `world` interface (capability `world`):
`world.seed` and `world.top-block`. Both are additive: no records, callbacks or earlier functions
change, and a mod built for level 35 or older links as before. A mod importing either needs
Horizon level 36 or newer. The SDK exposes the generated wrappers as `server::world::seed` and
`server::world::top_block`, plus `server::surface_block(dimension, x, z)` (top-block from the top of
the world). The same round fixes three engine gaps found while building the Wasteland mod.

## `world.seed() -> s64`

The world's seed, as level.dat holds it and `/seed` prints it (signed 64-bit). A mod that keys its
own storage per world (the plan's seed + spawn identity) no longer has to ask an operator for the
seed with a command. It is the server's seed: on a dedicated server, that world's; client mods do
not have the `world` interface.

## `world.top-block(dimension, x, z, below-y) -> option<s32>`

The Y of the highest block at or below `below-y` in block column `x`, `z` whose state is not air
(air, cave air, void air), read from the loaded chunk's blocks as of the call. Fluids, plants,
leaves, snow layers and glass count; `heightmap` (OCEAN_FLOOR) does not count fluids or non-opaque
blocks, so the two answer different questions. `below-y` above the world's top starts at the top
block row. None for an unloaded chunk, an unknown dimension, or air down to the bottom. Sections of
only air are skipped whole, so a call from the top of an ordinary column costs a few section
checks and at most one section's scan.

## `world.heightmap` and block edits

`heightmap` reads the live chunk: the chunk's cached WORLD_SURFACE column, which the OCEAN_FLOOR
scan starts from, is dropped by every block change that can move it (`Chunk::set`: a change at or
above the column's top - 1; whole-section replacements drop the map). `/setblock`, `/fill`,
players, explosions and earlier `set-blocks` therefore show at once. The one lag is the mod's own
queue: `set-blocks` changes are made after the hook call returns, before the dimension's next tick,
so a `heightmap` or `top-block` in the same call still sees the old blocks; read them from the next
call (the next tick, for a tick hook).

## Fix: biome modifiers keep content biomes

`mod_content::place_in_biomes` read the base `worldgen/biomes.json` instead of the overlay the
content biomes had just been written to, so when any mod shipped a NeoForge biome modifier, every
content-registered biome (`wasteland:*`) vanished from the bundle and world creation failed with
"unresolved tag entries". It now reads the overlay's biome, biome tag, placed feature and feature tag
bundles, so a modifier can also target a content biome or a tag holding one. Test:
`mod_content::tests::biome_modifiers_keep_content_biomes` (fails on the old read).

## Fix: server mods' log lines

`log.write` from a server mod reaches the server's `logs/latest.log` (the integrated server's is
the game's) as `[mod <id>] message`, as client mods' lines do. Two changes: the game's log keeps
info and above, so a `debug` line was dropped; it is now written at info, marked `(debug)`. And the
log's flood gate (100 warnings a minute per source) keyed every mod's line to one source (the host
function's line), so one noisy mod hid every other mod's warnings and errors; each mod is now its
own source. Test: `api36_server_mod_log_lines_reach_the_host_log_with_the_mod_id`.

## Fix: mod templates' connecting blocks join up

Vanilla's jigsaw pieces place with a known shape (`SinglePoolElement.getSettings` sets
`knownShape`), so a template's fences, panes, bars, walls and stairs keep the properties it was
saved with, and a generated template had to write every connection by hand. A jigsaw piece from a
template outside the `minecraft` namespace now gets a connection pass after placement
(`processor::connect_placed_shapes`): each placed block whose class is a cross-collision block
(fences, glass panes, iron and copper bars), a wall or a fence gate takes each side from that side's
neighbour (`updateShape` in `UPDATE_SHAPE_ORDER`, written with the piece's flags, neighbours not
notified); stairs take their shape from the stairs around them. Only neighbours inside the chunk
being decorated count: a side facing another chunk keeps the template's value (its blocks may not
be generated yet, and the result must not depend on chunk order), and stairs on a chunk edge are
left as saved. Vanilla templates are untouched, so vanilla worlds generate as before. Chains have no
connection properties (only `axis`) and are not affected. Tests:
`level::growth_tests::a_known_shape_piece_connects_its_connecting_blocks_inside_the_box` (fence,
bars, pane, stairs, the chunk-edge rule, and no change without the pass).

## Tests

- `crates/horizon-mods/tests/api36/mod.rs` (`api36_*`): the seed (signed extremes), top-block
  through the guest, None without a lent world, the guest's level-36 imports, log lines.
- `runtime::entity_tests::pre_level_36_components_still_link`: a level-35 component links.
- `mod_world::tests::top_block_reads_the_column_below_a_height_and_follows_edits`: the column scan,
  chunk coordinates of negative blocks, and heightmap/top-block after edits.
