# Phase D: API level 33 atmosphere (fog, sky and particle effects)

API level **33** adds interface `atmosphere`, granted by the new experimental capability
`atmosphere` (client mods only; `experimental = ["atmosphere"]`). Seven imports, no export, no
change to any earlier record, function or callback. A mod that does not import them pays nothing;
one that does is refused on an older Horizon with "update Horizon".

The split: the mod decides *what* (which profiles, how much of each) a few times a second; the game
samples, blends and cross-fades every frame on its own side, so the mod needs no `frame` hook.

## Profiles

`define-profile(profile) -> result<u32, string>` stores a profile and returns its id (0, 1, ...).
Profiles live as long as the mod runs; there is no removal.

| Part | Content | Checks |
|---|---|---|
| `fog-keys` | `day-time`, `color` (0xRRGGBB), `start`, `end`, `relative` | at most 32; day-time 0..=24000 (24000 is 0); no two keys at one time; distances finite, `0..4096` blocks or `0..2` render distances when `relative`; start <= end |
| `sky-keys` | `day-time`, `zenith`, `horizon`, `cloud`, `cloud-far` | at most 32, same time rules, colours <= 0xFFFFFF |
| `water-fog` | `color`, `start`, `end` (optional) | blocks 0..4096, start <= end |

A profile needs at least one part, and a mod at most **64** profiles. Errors leave nothing stored.
The game sorts keys by time and interpolates between the two around the frame's day time in linear
RGB, wrapping from the last key to the first over midnight; one key is constant. Relative distances
are multiplied by the render distance in blocks at sampling time.

## Mix

`set-mix(weights: list<tuple<u32, f32>>, transition-seconds: f32) -> result<_, string>`: up to **4**
distinct profile ids of this mod, weights finite and >= 0 with a positive sum (normalised to 1; zero
weights dropped); the transition is clamped to 0..30 s and must be finite. An empty list means
vanilla. Errors keep the previous mix. More than **8** accepted calls in 20 ticks are ignored (logged
once, `Ok`), so a mod re-deciding at tick rate cannot churn the cross-fade.

The game cross-fades from the weights it shows now to the new ones over the transition, so a mix
changed mid-fade does not jump; vanilla takes the weight the profiles leave (fading in from or out
to vanilla). A profile that leaves a part out (no sky keys, say) counts as vanilla for that part.
With several mods, the first in load order with a non-empty mix wins; otherwise the first that set
an empty one (so its fade to vanilla plays out).

## Where it applies

Per frame (`crates/horizon-client/src/game/mod_atmosphere.rs`):

1. **Sky**, after `environment::apply_weather` and before the End flash and lightning flash:
   `zenith` -> `Sky.sky_color`, `horizon` -> `Sky.fog_color` (the colour vanilla fog fades to),
   `cloud` -> `Sky.cloud_color` (alpha kept). `cloud-far` is carried for a later level; all clouds use
   `cloud` today.
2. **Fog**, after `fog::setup_fog` (`render_tick.rs` `frame_fog`), before Horizon's Fog option
   (`clear_distance_fog`) and native mods' `native_compute_fog_color` / `native_render_fog`, so the
   player's option and native mods keep the last word. In the open the profiles' air fog replaces the
   colour and the environmental start/end; under water their water fog does. The render-distance fog,
   sky end and cloud end stay vanilla.
3. **Skipped**: on the title screen, with the camera in lava or powder snow, under blindness or
   darkness and with a boss's world fog. Override is final: vanilla rain darkening is replaced, so a
   mod picks rain/thunder profiles itself (`conditions`).

Blending is in linear RGB: `out = vanilla * (1 - sum w) + sum w_i * part_i`, then back to sRGB.

## Conditions

`conditions() -> option<scene>`, read from the tick's snapshot (filled only while a running mod has
the capability; none on the title or with no player): `rain`, `thunder` (0..1, as the frame draws
them, PvP & HUD's weather included), `day-time` (0..24000), `moon-phase` (0..7), the camera's
`camera-x/y/z`, `yaw`, `pitch` (last frame's), `eye` (`air|water|lava|powder-snow`), `sky-light` and
`block-light` at the camera's block, and `sky-visible` (no motion-blocking block above the camera's
column).

## Particle effects

`define-effect(effect) -> result<u32, string>` stores an effect (at most **256** per mod, never
removed); `start-emitter(effect, anchor, density, tint) -> result<u32, string>` starts one,
`move-emitter(id, anchor)` moves it and `stop-emitter(id, fade)` ends it (with `fade` its particles
live out their lives, without they go at once). The game simulates every particle host-side in the
client particle engine through a `CurveController` (a `ParticleController`, as native mods use), so
the mod needs no `frame` hook. The record is a compiled subset of the `generated/effects.json` shape
of the Immersive VFX port:

| Part | Content | Checks |
|---|---|---|
| emitter | `looping`, `duration` (s), `rate` (/s), `density-scaled`, `max`, `burst`, `shape` (`point`, `box`, `disc` (horizontal), `sphere`), `offset` (span3), `half-size`, `radius` (span), `surface`, `direction` (span3, none: outwards from the shape's centre) | duration 0.05..600, rate 0..2048, max 1..512, burst 0..512, offsets, sizes and directions within 64 blocks |
| motion | `lifetime` (s), `speed` along the direction, `velocity` (span3), `drag` (`v *= exp(-drag dt)`), `gravity` (down), `acceleration` (a curve per axis) | lifetime 0.05..60, speeds and accelerations within 64, drag 0..20, gravity -40..40 |
| look | `size` and `alpha` curves, `color` or up to 8 `color-keys` (linear), `shade` (random darkening), `spin` and `spin-rate` (degrees), `fade-near` / `fade-far` (camera distance), `emissive`, `translucent` | size 0..16, alpha 0..1, colours <= 0xFFFFFF, shade 0..1, fades 0..128 |
| rules | `sprite`, `frame` (`first`, `random`, `over-life`, `loop` at `frame-rate`), `only-in` (`anywhere`, `air`, `water`), `die-nearer-than` | frame-rate 0..60, die-nearer-than 0..16 |

A `span` is a `lo..=hi` range each particle draws once (lo <= hi). A `curve` is `base * keys(life) +
wave(age)`: the base is drawn per particle, up to **8** keys `(t 0..1, value)` are sorted and eased
(`linear`, `smooth`, `step`; none means 1), and the optional `wave` adds `amplitude * sin(2 pi (age /
period + phase))` with its three spans drawn per particle (period 0.01..600 s). Errors name the
field and store nothing. `horizon-mod-sdk`'s `atmosphere_fx` builds the records (`emitter`,
`particle`, `fade`, `flat`, `span`...).

**Sprites.** `sprite` names a sprite set the client particle engine has loaded:
`<namespace>:<name>` in a namespace of the mod (its id or a manifest `namespaces` entry), or
`minecraft:<type>` for a vanilla particle type's sprites (the fallback for a mod with no content).
The open check of the plan, answered: the engine reads `assets/<ns>/particles/<name>.json` (and the
atlas packs `assets/<ns>/textures/particle/*.png`) from every namespace of the resource root, and a
mod's content enters that root only with capability `registries` (`api = "0.2"`, the mod then ships
`assets/` beside `mod.wasm`). So a client mod ships its own sprites as a content mod; a plain
`api = "0.1"` client mod uses vanilla's. The game hands the loaded set names to the mods'
snapshot before `init` and after every resource reload; an effect naming a set that is not loaded
is refused ("not loaded").

**Emitters and caps.** `anchor` is `camera` (followed each tick, world axes) or `point(x, y, z)`.
`density` 0..4 multiplies a density-scaled rate; `tint` multiplies the colour. Per mod: **48** live
emitters (ids reused after a stop, each start a new serial for the game), **3072** live particles
(inside the engine's 16384) and **512** spawns per particle tick (20 per second; ticks rather than
frames because the engine adds particles on its tick), and one emitter's `max`. A one-shot emitter's
id frees when its duration ends. Particles: Decreased halves rates, Minimal spawns none. Nothing
spawns on the title screen, while the game is paused (the particle tick does not run), farther than
**64** blocks from the camera, or from an anchor beyond 192 blocks; a particle dies on leaving its
`only-in` medium, when its emitter is stopped without fade, or when the mod stops or reloads.

**Drawing.** Every particle is a camera-facing billboard on the cut-out or translucent particle
layer, lit by the world or full bright. Not in this level: `facing` modes other than the camera,
additive blending (use `translucent` + `emissive`), collisions, events and sprite sub-grids; the
compiler maps those effects to the nearest supported look.

## Cost

Host only: sampling at most 4 profiles is a few dozen multiplies per frame; when no mod has a mix the
frame does one check. The guest calls `set-mix` when its decision changes (the demo: twice a second,
only when rain moves by a tenth). Particles cost the engine's own per-particle work plus a curve
sample or two per tick; with no emitter the particle tick does one empty check. The guest pays only
for its `define-effect` and emitter calls.

## Tests and tools

- `crates/horizon-mods/src/atmosphere.rs`: key wrap over midnight, relative distances, missing parts,
  validation (NaN, ranges, duplicates, caps), mix normalisation, blending, the cross-fade retargeting.
- `crates/horizon-mods/src/atmosphere_fx.rs`: curve sampling (eases, sorting, ends), waves, effect
  validation per field, the spawn clock (rate, density, burst, one-shot end), shapes and seeded
  determinism, motion (gravity, drag, expiry) and looks (curves, colour keys, camera fades, frames).
- `crates/horizon-client-render/src/particles/native.rs`: curve particles reach 64 blocks and name
  vanilla sets; `crates/horizon-client/src/game/mod_particles.rs` is the controller and emitter glue.
- `crates/horizon-client/src/game/mod_atmosphere.rs`: fog/water/sky application and the skip rules.
- `crates/horizon-mods/tests/sandbox.rs` `api33_*`: profiles, caps, mix rate limit, conditions, effects
  (sprite sets loaded and owned, field checks, 256 cap), emitters (start, move, stop, fades, id
  reuse, 48 cap) and the demo mod across the sandbox.
- Demo: `mods/official/atmosphere-demo` (`/atmo auto|misty|storm|crimson|off|fog RRGGBB S E|conditions|emit [DENSITY|stop]`).
- Live: `tools/live_atmosphere.py` loads the demo in a scratch game, sets a distinctive fog colour and
  checks it in a screenshot, then starts the motes and reads their live count from `horizon-ctl
  status` (`mod_particles`).

Later part of level 33 (plan `api-d-plan.md`): sky sprites (M3).
