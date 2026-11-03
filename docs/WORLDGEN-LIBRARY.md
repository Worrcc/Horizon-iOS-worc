# Content biome and world-generation library

Design, 2026-11-03. This is an MIT extension of `horizon-world` and the existing
content-root loader, under the repository's MIT licence. It needs no separate
crate, Java runtime, or WASM component. Stage 1 precedes the implementation below.
Existing-hook line references use starting revision `9f2b140e4`; references in
“Implemented slice” describe the resulting files.

## Contract and data

A content-only `.hmod` uses `api = "0.2"`, `side = "both"`, capability
`registries`, and no `mod.wasm` (existing contract: `docs/mods.md:377`). Its
owned namespaces may contain nested names:

* `data/<ns>/worldgen/biome/<path>.json`: a vanilla **26.3** biome definition.
  Temperature, downfall, precipitation and temperature modifier describe the
  biome's weather; these are distinct from its multi-noise selection parameters.
  `effects` supplies water, grass, foliage, dry foliage colours and grass modifier.
  `attributes` supplies sky, fog and water-fog colours and
  `minecraft:gameplay/natural_mob_spawns` (categories, weighted entity/count
  entries, spawn costs), plus creature-generation probability. The existing
  reader handles plain natural-spawn values and overlay arguments
  (`crates/horizon-client/src/biome.rs:142`). Older `spawners`, `spawn_costs`,
  `effects.sky_color` and `effects.fog_color` files need conversion to the pinned
  schema; they must not silently be advertised as 26.3 definitions.
* `carvers` and ordered per-step `features` are vanilla biome generation settings,
  referring to existing vanilla or loaded content resources. Empty arrays are
  useful for the minimal slice. No custom noise settings are needed to put a new
  biome on vanilla terrain.
* Proposed Horizon extension:
  `data/<ns>/worldgen/biome_source_addition/<path>.json`. This is an additive
  declaration, **not** a vanilla registry or an override of a dimension file.
  Its source has vanilla multi-noise inline-list syntax plus `horizon:preset`,
  the existing preset whose list receives the entries:

```json
{
  "type": "minecraft:multi_noise",
  "horizon:preset": "minecraft:overworld",
  "biomes": [{
    "biome": "example:frost_grove",
    "parameters": {
      "temperature": -0.7, "humidity": 0.4, "continentalness": 0.3,
      "erosion": 0.1, "depth": 0.0, "weirdness": 0.2, "offset": 0.0
    }
  }]
}
```

Each of the six coordinates accepts a scalar, `[min,max]`, or `{min,max}` in
[-2,2]; offset is in [0,1]. Reuse the codec and its float quantization
(`crates/horizon-world/src/worldgen/climate/codec.rs:36`, `:41`). A biome can
have several selection points and occur in several presets. A biome without an
addition is registered but never automatically selected. Unknown biome/preset,
missing or empty inline list, wrong source type, reversed ranges and excessive
combined entries must fail with the declaration's resource name. Never fall back
to the preset when the inline entries are malformed.

Horizon **already implements** the needed source: `MultiNoiseBiomeSource::new`
accepts an arbitrary ordered parameter list
(`crates/horizon-world/src/worldgen/biome.rs:56`);
`BiomeSourceCatalog::decode` binds inline lists to known biomes
(`crates/horizon-world/src/worldgen/biome_source/codec.rs:123`), and
`BiomeSourceDefinition::resolve` builds either an inline or imported preset source
(`crates/horizon-world/src/worldgen/biome_source/runtime.rs:59`). Built-in preset
builders need not be ported: `GeneratorData::load` already imports quantized
tables or quantizes prepared biome-parameter reports
(`crates/horizon-world/src/worldgen/generator.rs:399`). Append to those tables
once while loading, before generators resolve their source.

An added point competes by vanilla squared distance, including offset. It wins
where its fitness is strictly smaller. Appending does not promise that a point
already covered at distance zero by vanilla will select the mod biome: ties use
the existing tree and previous-result semantics. Adding entries can also change
tree ordering on ties. Tests must use a strictly winning point and compare vanilla
results at strictly nearer vanilla points elsewhere, plus an exact empty-addition
regression. There is no arbitrary priority or random biome replacement.

## Existing hooks and changes

References in this section identify the starting implementation, before this
change; names allow navigation if subsequent edits move line numbers.

| Piece | Existing hook | Work |
|---|---|---|
| Content collection | `crates/horizon-client/src/mod_content.rs:805` (`WORLDGEN_BUNDLES`), `:1240` (`worldgen_entry`), `:1325` (bundle merge) | Include biome and source-addition bundles. An absent optional addition bundle must be created; never rewrite the base. |
| Biome tags | `crates/horizon-client/src/mod_content.rs:999` (`Known::new`), `:1264` (`known.keeps`), `:1301` (biome tag merge) | Add all collected biome names before filtering tags, so cross-mod references survive independent of file order. |
| Registry | `crates/horizon-world/src/worldgen/generator.rs:389`, `:424` | `biomes.json` already creates names, IDs, temperatures and carvers. Content must reach this bundle. Existing IDs are snapshot-local sorted indices, not portable network IDs. |
| Source | `crates/horizon-world/src/worldgen/generator.rs:416`, `:836` | Validate additions against the same biome catalog, append to imported preset tables, then reuse ordinary resolution. Inline/fixed/checkerboard/End sources remain explicit choices. |
| Chunk lookup | `crates/horizon-world/src/worldgen/generator.rs:937` (`biome_chunk`), `:595` (`biome_id_at`) | Keep quart-volume climate sampling and quart section storage. No per-column JSON or linear scan. Block lookup uses biome zoom where appropriate, not a new surface-only map. |
| Client identity | `crates/horizon-client/src/vanillagen.rs:146`, `:292` | Existing name-to-client-ID mapping already translates generator palettes; register content names on the client. |
| Colours/weather/spawns | `crates/horizon-client/src/biome.rs:114` (`Biomes::load`), `:185` (index) | Enumerate owned namespaces recursively; retain vanilla's current sorted order, append other namespaces deterministically; reuse all existing property parsing. Check the u16 ID bound. |
| Fog | `crates/horizon-client/src/fog.rs:40` (`FogTables::load`), `crates/horizon-client/src/environment.rs:240` (`sample`) | Fog-distance resource paths must use each biome's namespace. Existing interpolated colours consume `BiomeInfo` directly. |
| Natural spawns | `crates/horizon-client/src/game/natural_spawning.rs:232`, `:235`; `crates/horizon-client/src/mobs/spawn_selection.rs:27` | Existing level adapter reads biome categories/costs. Loading them enables selection; template-specific spawn rules still apply. Reading an entity ID is not proof that all content species can spawn. |
| Decoration | `crates/horizon-world/src/worldgen/generator_decoration.rs:184`, `:219`, `crates/horizon-world/src/worldgen/live.rs:152` | Consume the same bundled biome feature lists, resolve named placed features, sort steps and filter membership. No new per-mod decorator. |
| Startup | `crates/horizon-client/src/game.rs:14489`, `:14658` | Existing resource loading creates biome and fog tables from the overlay root; both server-thread and in-frame paths share it. |

## Terrain, noise and scope

The first slice uses vanilla dimension generators, noise settings, density graphs,
aquifers, surface rules and climate samplers. Biome identity can affect existing
surface biome conditions, carvers, decoration, temperature and spawns. A new biome
does not itself define mountain shape or a new ground material: vanilla surface
rules often test specific vanilla names. A mod requiring custom frost/ash topsoil
needs explicit surface-rule composition in a later slice, rather than a hardcoded
name check. Feature algorithms and structure types must already be supported.

A new `noise_settings` would need content collection of settings, normal-noise
and density-function registries, validation of referenced graphs and blocks,
surface rules/conditions, sea level and height/cell bounds, aquifer/ore switches,
router compilation, and an explicit dimension/world-preset generator binding.
Existing loaders include `crates/horizon-world/src/worldgen/generator.rs:169`
(resource maps), `:479` (world presets), and `:836` (source resolution);
`docs/density-functions.md`, `docs/noise-generation.md`, `docs/heightmaps.md`
describe independently tested primitives, not automatic mod registry integration.
Defer custom noise settings, dimensions and arbitrary density callbacks.

## API and compatibility

Everything above is immutable content loaded before world creation. No server-mod
function is necessary. Runtime registry mutation, per-position WASM callbacks and
changes to existing WIT/Rust record layouts are excluded. Existing
`world.get-biome` already returns names (`docs/mods.md:483`); a dynamic structure
can use `world.generated-chunks` and `world.place-structure`
(`docs/mods.md:591`, `:632`) when declarative features cannot express its logic.
These functions do not register biomes or change the source.

There is no mod API level bump for adding JSON handling or a Rust loader helper.
If a later feature really needs a host function, add a function with new result
types as needed, preserve all existing records and hooks, bind host and SDK,
enforce capability/bounds, assign the next level in
`crates/horizon-mods/src/api_level.rs` and raise `LEVEL`, update the generated API
reference, guide and example, and run compatibility plus host/SDK tests
(`docs/api/stability.md:28`, `:50`, `:62`, `:88`). The brief references a skill's
“Adding an API” checklist; that skill is not supplied in this worktree/session.
No WIT addition is proposed, so that missing checklist does not block this slice.

With no declarations, do not create an additions bundle or rebuild climate trees;
keep the vanilla source bytes, RNG order, client IDs and resource-root fast path
(`crates/horizon-client/src/mod_content.rs:1370`). Content caching already hashes
the executable and mod files (`:1398`); a new build invalidates an old overlay.

## Risks and iOS budget

`docs/biome-lookup.md` specifies quart storage, bulk climate sampling and a retained
searcher per worker; it supplies **no numeric iOS timing or memory budget**. Do not
invent one. Retain those mechanisms, immutable shared tables and existing zoom;
perform decoding/tree construction once at load, never on the frame/tick path.
The catalog caps biomes at 8,192 and the index caps climate entries at 65,536
(`crates/horizon-world/src/worldgen/biome_source/codec.rs:43`,
`crates/horizon-world/src/worldgen/climate/index.rs:222`). Bound combined additions
and ensure no allocation/work increase for unchanged mod sets without declarations.
Phone acceptance requires measurements of startup memory, generation throughput
and frame tails at a representative mod count; desktop tests cannot establish it.

Registry sync, save/load with removed mods, tag references, missing feature/carver
resources, overlapping climate regions and chunk seams after changing a mod set
need explicit follow-up coverage. Generator and client IDs differ by design;
never copy numeric IDs between snapshots. Existing vanilla clients cannot be
assumed compatible with new biome registry entries. Live content reload must not
silently regenerate existing palettes under different IDs.

The two external port/skeleton worktrees named in the brief were not inspected:
the brief's final rule confines work to this worktree. No external mod source or
Mojang material is introduced. The online contributor PR check cannot be completed
under the requested offline-only workflow.

## Delivery stages and acceptance

1. **Design (this document), before code.** Establish content schema, additive
   selection, reuse points, API scope, budgets and unresolved compatibility.
2. **Minimal slice.** Bundle content biomes and source additions; preserve biome
   tags; load nested namespace biome properties and fog paths; append validated
   climate entries to presets without changing records. Resource-free tests must
   prove package → overlay → client registry, properties/spawns, climate-point
   selection, vanilla winners elsewhere, no-declaration behaviour and malformed
   declarations. No new terrain settings or mod hooks.
3. **Transport and generation proof.** Fix the shared configuration registry loader
   described below, then with private prepared data verify actual generated
   quart palettes and features/carvers for a content biome, both single-player
   paths, dedicated/LAN, colours/fog and natural spawning. Run biome-source,
   generation, placement and natural-spawn parity harnesses with no declarations.
4. **Persistence and distribution.** Prove saved biome names,
   reopening worlds and missing-mod diagnostics before claiming multiplayer or
   stable removal/reload support.
5. **Terrain extensions and phone acceptance.** Define surface-rule additions,
   then only if needed new noise/settings/dimension support. Reviewer measures
   desktop and iOS before a performance or gameplay claim.

The maintainer should settle the extension path/name, climate overlap policy
(accept vanilla distance/ties or design a separately tested explicit policy), and
the numeric phone budget before treating the data contract as stable. The proposed
defaults above allow the minimal slice to proceed without those policy decisions.


## Implemented slice and remaining barrier

Stage 2 completes registry/source loading, with two new functions and tests:

* `BiomeSourceCatalog::with_preset_additions` in
  `crates/horizon-world/src/worldgen/biome_source/additions.rs:10` validates
  declarations, preserves the original entries and appends quantized points. It
  bounds the combined list and leaves search-tree creation to normal resolution.
  `GeneratorData::load` calls it after importing presets. There are no new WIT
  functions, dependencies, record fields or API-level changes.
* `biome_files` in `crates/horizon-client/src/biome.rs:118` discovers nested
  resources. `Biomes::load` uses the existing content namespace marker, keeps
  vanilla IDs first and parses the existing climate/colour/spawn fields unchanged.
  The content overlay now bundles biomes and additions and retains their tag
  references; fog-distance lookup uses the resource's actual namespace.

This is a tested **library slice**, not complete player-facing biome support.
Inspection found a concrete next barrier: `SyncData::load` enumerates only
`data/minecraft` (`crates/horizon-client/src/lan/data.rs:130`, `:136`), and
`SectionCoder` maps unknown biome names to network ID zero
(`crates/horizon-client/src/lan/chunks.rs:419`). This affects the default
single-player server thread as well as LAN/dedicated servers. The next change must
append content biome entries deterministically to the synchronized registry and
send their data even when the peer knows the vanilla core pack
(`crates/horizon-client/src/lan/data.rs:179`). It must test client decoding, palette
IDs and tag translation with and without a known core pack. Do not claim that this
slice makes mod biome colours appear in the default playing path. Expanding into
registry transport is left to a separate tested change under the brief's allowance
to stop after a complete part.

Tests use original, synthetic biome/table/PNG fixtures, without prepared resources:
`content_climate_point_and_vanilla_elsewhere` checks point selection, unchanged
vanilla winners elsewhere, empty-addition bytes, preserved entries, accumulating
resources, invalid lists/references/ranges and the combined entry cap.
`content_biome_registry_and_climate_source` checks the actual content package and
cached overlay, nested biome identity, climate, colours, spawners/costs, biome tags,
preset resolution and sampled source, and an ordinary content mod with no biome
declarations. Existing `biome_data` and content-root tests provide regression
coverage. Full biome codecs, generated-world parity, game play and iOS are not
established by these tests.

Validation completed offline with `CARGO_TARGET_DIR=$PWD/.local/target-wglib`:

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy -p horizon-world -p horizon-client --all-targets --offline -- -D warnings` | Passed; narrow scope of changed crates, no changes to `horizon-mods` |
| `cargo test --offline -p horizon-world --lib content_climate_point_and_vanilla_elsewhere` | 1 passed |
| `cargo test --offline -p horizon-world --test biome_data` | 2 passed |
| `cargo test --offline -p horizon-client --lib mod_content::tests::` | 14 passed, 5 ignored |
| `cargo test --offline -p horizon-client --lib fog::tests::` | 8 passed |
| `cargo test --offline -p horizon-client --lib biome::blend_tests::` | 2 passed |
| `python3 tools/check_public_tree.py` | Passed |

The new tests always run without prepared resources. Some older content-root tests
return early without them; the five full-root tests remain ignored. No game,
launcher, Java oracle, iOS build, phone measurement, commit or push was performed.
The final no-commit rule in the brief takes precedence over its earlier conditional
commit instruction.
