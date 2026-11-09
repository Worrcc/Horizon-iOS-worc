# Apple-chip performance plan (Horizon-iOS)

Goal: make the game run steadier, cooler and faster on iPhones/iPads (test device: A16, ProMotion)
by using what Apple chips do differently from desktop PCs. Base branch: `claude/ios-horizon-66f3981`
(merge of upstream `66f3981f5`, not pushed yet).

Constraint that shapes everything: **nothing here can be measured from Linux.** Builds come from
the macOS CI `ios` workflow (a push starts it, pushes need the user's OK) and numbers come from the
player's `latest.log`. So step 0 makes the game log the numbers, and the GPU steps wait for them.

All changes must keep desktop identical: gate on `target_os = "ios"` (CPU/settings) or
`wgpu::Backend::Metal` + Apple-family GPU (renderer). Desktop checks per step:
`cargo clippy -p horizon-client --all-targets` (+ `--no-default-features`),
`cargo fmt --all -- --check`, `python3 tools/check_public_tree.py`, `ios/scripts/check-ios.sh`,
and the touched crates' tests.

## What already exists (reuse it, don't duplicate)

- `graphics.rs` `Graphics`: `resolution_scale` (50-200), `upscaling`, `workers`, `battery`
  (halves workers, caps 60 fps in a world / 30 in menus), presets, `present_mode`, `pacing`.
- `settings.rs`: `render_distance: 12`, `max_fps: 120` defaults.
- `horizon_ios::prefer_frame_rate(max)`: display link asking 80-120 Hz (commit `22e327920`).
- `horizon-threads`: `favour_current()` (USER_INTERACTIVE for render/server threads, Apple and
  Windows), `far_terrain_current()` (UTILITY on Apple), but `put_helpers_behind()` (lowering the
  streaming workers) is **Linux-only**, and worker count = all cores - 1, blind to P/E cores.
- Renderer: Metal already splits solid terrain (`split_solid`), skips multi-draw, uses
  `StoreOp::Discard` in places. wgpu 30's Metal backend makes `TextureUsages::TRANSIENT`
  attachments memoryless on Apple GPUs (`wgpu-hal/src/metal/device.rs:561`). **The game uses
  `TRANSIENT` nowhere.** No shader uses `f16`.
- Uploads: one reused staging belt (`gfx/upload.rs`).
- Mods run on wasmtime Pulley (interpreter) on iOS.

## Step 0: on-device performance log (do first)

- iOS-only, every 10 s in a world: avg fps, 1%/0.1% low frame time, CPU frame ms, GPU frame ms
  (from existing timing if present), render distance, resolution scale, effective fps cap,
  `NSProcessInfo.thermalState`, low-power mode, resident memory (`process_memory`). One line
  prefixed `perf:` in `latest.log`.
- `horizon_ios::thermal_state() -> ThermalState { Nominal, Fair, Serious, Critical }` and
  `low_power_mode() -> bool` (objc2 calls on `NSProcessInfo`).
- Ship as a baseline build; the player plays ~10 min, sends `latest.log`.
- Files: `crates/horizon-ios/src/lib.rs`, a small `perf_log` module in `horizon-client` (find the
  per-frame hook near `options.rs:651` frame limiting).

## Step 1: phone defaults and heat (low risk)

1. **iOS first-run defaults** (only when no options file exists; never overwrite a player's
   choices): render distance 8, simulation distance 6, max fps 60, resolution scale 75 with the
   existing upscaler at Native, workers per step 2. Desktop defaults unchanged.
2. **Display link follows the cap**: `prefer_frame_rate(max_fps)` whenever the cap changes, so a
   60 fps cap lets the screen drop to 60 Hz (saves power) and 120 still unlocks ProMotion.
3. **Thermal governor** (iOS, on by default, a "Thermal Throttling" toggle in Horizon options):
   - Fair: nothing. Serious: cap 60 fps, resolution scale -15 points (floor 50).
     Critical: cap 30 fps, resolution scale floor 50.
   - Back off only after 30 s at a lower state (hysteresis). Never writes the player's settings,
     applies on top of them. Low-power mode acts like battery mode.
   - Logged in the `perf:` line.
- Files: `settings.rs`, `graphics.rs`, `graphics_ui.rs`, `options.rs`, `horizon-ios`.
- Risk: resolution-scale changes may rebuild render targets; changing it mid-game must not hitch
  more than once per state change. Check how `resolution_scale` changes are applied today.

## Step 2: CPU (low risk)

1. **Workers at UTILITY QoS on Apple**: extend `put_helpers_behind()` / `behind()` to Apple with
   `pthread_set_qos_class_self_np(QOS_CLASS_UTILITY)`, so iOS runs streaming workers on the
   efficiency cores and keeps the performance cores for render and server threads.
   iOS only at first (macOS desktop behaviour unchanged unless measured).
2. **P/E-aware worker count on iOS**: read `hw.perflevel0.physicalcpu` / `hw.perflevel1.physicalcpu`
   (sysctlbyname). Auto workers = E cores + max(P cores - 2, 0), at least 2 (A16: 4).
   Falls back to today's rule if the sysctls fail.
3. **`favour_current()` on iOS**: it skips machines with <=4 cores; all iPhones since A11 have
   6, fine, but make sure the render thread actually calls it on iOS.
4. **Build flags (ios only)**: `-C target-cpu=apple-a14` in `ios/build.sh` and `ios.yml`, and
   `lto = "fat"` via an `ios-release` profile. Open question below on the CPU baseline.
- Files: `crates/horizon-threads/src/lib.rs`, `graphics.rs` `worker_threads`, `ios/build.sh`,
  `.github/workflows/ios.yml`, root `Cargo.toml`.

## Step 3: tile-based GPU (needs step 0 numbers; medium risk)

1. **Pass audit** (investigation, no code): list every render pass in a normal iOS frame with
   load/store ops and attachments (`gfx.rs`, `gfx/oit.rs`, `post_chain.rs`, `gfx/world_post.rs`,
   `gfx/native_post.rs`, `gfx/pack_post.rs`, `gfx/hdr.rs`). Output: a table in this file.
2. **Memoryless attachments**: mark render targets that are cleared and discarded inside one
   pass (depth for the world pass if nothing samples it later, MSAA, OIT accumulation if
   pass-local) `TRANSIENT` on Apple GPUs. A16 saves a full depth buffer of RAM and bandwidth.
3. **Clear instead of load**: switch `LoadOp::Load` to `Clear` where the earlier contents are
   overwritten anyway; switch end-of-pass `Store` to `Discard` for attachments no later pass reads.
4. **Merge passes**: where two passes write the same target back to back with no sampling between
   them (e.g. sky then terrain, overlays), draw them in one pass on Metal.
5. **f16 shaders**: request `Features::SHADER_F16` on Metal when the adapter has it; make
   fog/lighting/colour maths in the terrain and entity fragment shaders use `f16` through a
   shader define, with the f32 path unchanged on desktop. Watch for banding in fog and skies.
- Verification: desktop must render identically (screenshot compare via existing scene replay
  examples); on device compare `perf:` GPU ms before/after.

## Step 4: unified memory uploads (needs numbers; medium-high risk)

- On Apple GPUs (`UMA`), write chunk mesh data straight into mappable buffers instead of through
  the staging belt plus copy. Only if step 0 shows uploads/hitches on chunk loading matter.
- Files: `gfx/upload.rs` and the chunk mesh upload path.

## Step 5: startup (investigation first)

- Resource load is 12.7 s (20.6 s with Ice and Fire). Profile which stages dominate on desktop
  with a timing log, then cache the processed results (atlases, baked models) to disk keyed by
  resource hash on iOS. Plan the cache after the profile.

## Step 6: MetalFX upscaling (optional, last)

- The `Upscaling` setting exists. Add a MetalFX Spatial path on Apple GPUs through wgpu-hal's
  Metal interop (get the `MTLTexture`/command buffer, encode `MTLFXSpatialScaler`). Big and
  Apple-API-heavy: decide after steps 1-3 show whether resolution scale is the bottleneck.

## Mods (note, no step)

- Pulley is the bottleneck for heavy mods. Best lever is fewer host calls per tick (batch host
  APIs); the JIT route with StikDebug is already noted in `HANDOFF.md`.

## Order and workers

- Steps 0, 1, 2 now, each a Codex worker in its own worktree off the base branch
  (`~/Projects/iaf-swarm/perf-<step>`, branch `swarm/perf-<step>`). Claude reviews diffs, runs
  desktop checks and `check-ios.sh`, merges into `claude/apple-perf`.
- One push (with the user's OK) builds an IPA with 0+1+2 for the player's test.
- Step 3.1 (pass audit) can run in parallel as a read-only Codex investigation.
- Steps 3.2-6 after the device numbers come back.

## First device results (2026-11-09, A16, build a7e31eb)

- Low Power Mode was on the whole run (so battery mode: 60 cap, 2 workers), resolution 100%
  (saved options kept), render distance 8. Thermal stayed Nominal; the governor never acted.
- Steady state 58-60 FPS, CPU ~3 ms, GPU 8-10 ms per frame, 380-580 MB resident.
- First minute in a world: 37-48 FPS with GPU 21-26 ms while chunks and the Diorama map fill.
- **Stutter is the problem, not average FPS**: 1% lows 25-90 ms, spikes to 150 ms, around chunk
  loading and the iOS keyboard.
- The battery temperature is blocked by the sandbox (never shown): removed.
- Found: leaving the app ~30 s disconnects the player from their own single-player world.

New order:
1. Done next: perf-log fixes, slow-frame span breakdown in the `perf:` line, background disconnect fix.
2. Find and fix the stutter sources from the breakdown.
3. **Step 3 (GPU pass merging) is on hold**: GPU time has headroom at 60 FPS; revisit with
   Low Power Mode off at a 120 cap, where GPU time decides the frame rate.

## Open questions

1. CPU baseline for `target-cpu`: apple-a14 drops iPhone XS/XR/11 (A12/A13). apple-a12 is the
   safe choice, apple-a14 a bit faster. Default in this plan: **apple-a12**.
2. Phone defaults (render distance 8, 60 fps, 75% scale): fine as first-run defaults?
3. Thermal governor on by default?
