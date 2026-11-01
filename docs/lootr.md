# Per-player loot (Lootr)

Horizon carries the core rule of [Lootr](https://github.com/noobanidus/Lootr) by Noobanidus
(MIT licence): a chest, trapped chest or barrel that still holds what its loot table rolled
gives every player their own loot.

- The container keeps the shared roll and its loot table. Each player who opens it gets a copy
  rolled from the same table with a seed of their own; what they take stays taken for them
  only. Copies are kept per dimension in `horizon-lootr.nbt` in the world.
- Such a container breaks only while sneaking.
- On by default. `enabled: 0b` in that file turns it off for that dimension.

Not carried over: Lootr's own blocks, models and textures, refreshing and decaying loot, chest
minecarts, double chests (they open as vanilla's), the config. While one player has their loot
open in a container, another player is told it is in use.

Code: `crates/horizon-client/src/game/lootr.rs` (open, close, save settling, break protection).
