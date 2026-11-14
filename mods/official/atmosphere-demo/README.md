# Atmosphere demo (API 33)

Capability `atmosphere`: three fog and sky profiles keyed by time of day (`misty`, `storm`,
`crimson`), mixed by the rain level twice a second. Build with the worktree's release
`cargo-horizon-mod` (offline, `CARGO_TARGET_DIR` at the worktree's `target`):

```
cargo build --offline --release -p cargo-horizon-mod
cd mods/official/atmosphere-demo
CARGO_TARGET_DIR=<worktree>/target CARGO_NET_OFFLINE=true <worktree>/target/release/cargo-horizon-mod build
```

Load `<worktree>/target/horizon-mod/atmosphere_demo.hmod` as a `--dev-mod` in a scratch game built
with feature `mods`. `tools/live_atmosphere.py` does this and checks the fog colour on screen.

| Command | Effect |
|---|---|
| `/atmo auto` | misty day cycle, cross-fading to `storm` as it rains (default) |
| `/atmo misty`, `storm`, `crimson` | one profile alone (add `now` for no cross-fade) |
| `/atmo off` | back to vanilla over 2 s |
| `/atmo fog RRGGBB START END` | defines a plain fog and sky profile and shows it at once |
| `/atmo conditions` | rain, thunder, day time, moon phase, camera height, light, sky |
| `/atmo emit [DENSITY]` | glowing motes around the camera (an API 33 particle effect on vanilla end-rod sprites); again replaces it, `stop` fades it |
| `/atmo sky` | an aurora of drifting veils low in the north, a meteor every few seconds, a dark second moon and vanilla's sun far off turning with the stars (API 33 sky sprites); `off` removes them |
