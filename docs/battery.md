# Battery: what changed and how to measure it

Horizon on iOS (and, where noted, on every system) spends less power per frame. Items marked (iOS) only act on the phone.

## What changed

- `perf:` log line: whole-process CPU, context switches, wake-ups, GPU time and (arm64 iOS) energy per second.
- Frame limiter: Efficient and Precise pacing sleep once to an absolute deadline (`clock_nanosleep` on Linux and Android, `mach_wait_until` on macOS and iOS) instead of waking every millisecond; Efficient pacing spins only for the last 0.1 to 0.5 ms. Windows keeps its short steps. The frame-rate rules are unchanged.
- Background (iOS): the event loop sleeps up to one second at a time while the app is in the background, and wakes at once on resume.
- Far terrain (iOS): worker threads stay on the efficiency cores once near terrain has loaded; far terrain may load more slowly.
- Menu blur: runs at half size on iOS and on Apple GPUs (Apple-silicon Macs included), as it already did on software adapters.
- Heat governor (iOS): Fair now caps at 60 FPS (Serious still does, Critical 30). In Low Power Mode the resolution scale drops another 15 points, never below 50. Both follow the Thermal Throttling switch.
- Screens (iOS): the single-player pause screen is capped at 30 FPS, other screens at 60. Battery mode's own 30 FPS cap for menus stays. Gameplay is unchanged.
- Mods (iOS): the WASM mods' frame hook runs at 60 Hz (30 Hz in Low Power Mode) and not at all on the single-player pause screen. A mod's own screen and the inventory pages mods draw on keep running every frame.
- Phone defaults (iOS): on the title screen, once per install, an alert offers 75% resolution and 60 FPS to settings that still have 100% and 120 FPS (render 8 and simulation 6 too, if they are still 12 and 12).
- Lower Display Scale (iOS): an opt-in setting on the Power page that draws at 2x the screen's points instead of 3x. It applies at the next start.

## The new perf: fields

The `perf:` line is written every ten seconds in a world. The new fields come right after `resident_mib`:

- `proc_cpu_ms_s`: CPU time of the whole process per second of wall time; 1000 is one core busy.
- `ctx_switches_s`: context switches per second (voluntary and involuntary).
- `idle_wakeups_s`, `intr_wakeups_s`: package idle exits and interrupt wake-ups per second; fewer is better.
- `gpu_ms_s`: the kernel's GPU time for this task per second.
- `energy_mw`: the kernel's task energy estimate (arm64 iOS only). Compare runs with each other, not with a wattmeter.

A field the OS did not report is left out, never printed as n/a.

## How to test on the phone

Turn the FPS overlay on and play in the same world, at the same place, each time. Note the start time and the battery percentage at the start and end of each phase.

1. 10 minutes with Low Power Mode off and Max FPS 120.
2. 10 minutes with Low Power Mode on.
3. 2 minutes on the pause screen.
4. 2 minutes with the inventory open.

Then send `latest.log` (Files > Horizon > logs) with the times and percentages.

## Limiter measurement

The ignored test counts this thread's voluntary context switches over 60 frames at a 60 FPS cap with 2 ms of work per frame (Linux only):

```sh
cargo test -p horizon-client-input --lib efficient_pacing -- --ignored --nocapture
```

It prints `switches per frame` for Efficient pacing (`SleepingClock` with `end_frame_on_grid`) and for Precise pacing (`SystemClock` with `end_frame`), and fails if Efficient is 3 or more.

| Pacing | Before | After |
|---|---|---|
| Efficient | 19.0 | 1.0 |
| Precise | 14.0 | 1.0 |
