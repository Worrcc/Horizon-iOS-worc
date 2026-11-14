# Phase D part 1: API level 33 atmosphere (fog and sky)

API level **33** adds interface `atmosphere`, granted by the new experimental capability
`atmosphere` (client mods only; `experimental = ["atmosphere"]`). Three imports, no export, no
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

## Cost

Host only: sampling at most 4 profiles is a few dozen multiplies per frame; when no mod has a mix the
frame does one check. The guest calls `set-mix` when its decision changes (the demo: twice a second,
only when rain moves by a tenth).

## Tests and tools

- `crates/horizon-mods/src/atmosphere.rs`: key wrap over midnight, relative distances, missing parts,
  validation (NaN, ranges, duplicates, caps), mix normalisation, blending, the cross-fade retargeting.
- `crates/horizon-client/src/game/mod_atmosphere.rs`: fog/water/sky application and the skip rules.
- `crates/horizon-mods/tests/sandbox.rs` `api33_*`: profiles, caps, mix rate limit, conditions and the
  demo mod across the sandbox.
- Demo: `mods/official/atmosphere-demo` (`/atmo auto|misty|storm|crimson|off|fog RRGGBB S E|conditions`).
- Live: `tools/live_atmosphere.py` loads the demo in a scratch game, sets a distinctive fog colour and
  checks it in a screenshot.

Later parts of level 33 (plan `api-d-plan.md`): client particle emitters (M2) and sky sprites (M3).
