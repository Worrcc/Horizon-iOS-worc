# Machine demo (API 30)

Build the server guest here and the separate content mod in `content/` with the
worktree's release `cargo-horizon-mod`, setting `CARGO_TARGET_DIR` to the
worktree's `target` and `CARGO_NET_OFFLINE=true`. Load both `.hmod` artifacts as
`--dev-mod` arguments in a fresh scratch game with feature `mods` enabled.
Joining Horizon clients need the content package too.

`demo:crate`: 9 real slots, default chest screen. `demo:smelter`: every 40 ticks
moves one real item from slot 0 to result 1 and increments saved block data;
place/use rearms it. Top inserts input, bottom extracts result. Chunk unload
drops the timer; interact again after reload. Lectern interaction opens a
3-row display-only menu; all clicks log, left-click on the diamond-block icon
gives one diamond. `/demobreak x y z` uses iron-pickaxe engine drops and cracks.

Exact offline build, safe scratch run setup and manual checks:
[docs/api-expansion-b.md](../../../docs/api-expansion-b.md).
