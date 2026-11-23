# Phase C demo (API 31, part 1)

Player events, vitals, setters and world effects. Build with the worktree's
release `cargo-horizon-mod`, setting `CARGO_TARGET_DIR` to the worktree's
`target` and `CARGO_NET_OFFLINE=true`:

```
cargo build --offline --release -p cargo-horizon-mod
cd mods/official/phase-c-demo
CARGO_TARGET_DIR=<worktree>/target CARGO_NET_OFFLINE=true <worktree>/target/release/cargo-horizon-mod build
<worktree>/target/release/cargo-horizon-mod check <worktree>/target/horizon-mod/phase_c_demo.hmod
```

Load `<worktree>/target/horizon-mod/phase_c_demo.hmod` as a `--dev-mod` in a fresh scratch
game with feature `mods`. The tick hook announces deaths and welcomes
respawned players with a sound and particles. `/pcd` commands:

| Command | Effect |
|---|---|
| `vitals` | food, experience and effects |
| `health <hp>` | set-health (never kills) |
| `hurt <amount>` | `minecraft:generic` damage (armour applies) |
| `feed` | food 20, saturation 5 |
| `xp <points>` / `levels <n>` | experience |
| `cleareffects` | clear the active effects |
| `clear <id or #tag> [max]` | clear-items; replies with the count |
| `boom` | radius 3 blast 5 blocks along +x |
| `fall <x> <y> <z>` | turn that block into a falling block |
| `sparkle` | flame, dust and block particles, then a private sound |
| `flow <x> <y> <z>` | the fluid current there |

Manual live checks: [docs/api-expansion-c.md](../../../docs/api-expansion-c.md).
