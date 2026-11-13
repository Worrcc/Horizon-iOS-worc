# Storage API (level 26)

Level 26 adds server block-use hooks, messages between the two halves of a
sandboxed mod, and component-preserving storage transfers. All additions keep
earlier WIT records and functions unchanged. See [mods.md](mods.md) and the
[reference](api/mods-reference.md) for manifests and signatures.

## Block use

A server mod with `world` calls
`server-hooks.set-block-uses(true, false)` during initialization. The separate
opt-in follows the optional-callback pattern: the original registration record
keeps its shape, and older components need not export the new callbacks.

`ServerMod::block_used(player, pos, state, sneaking)` runs on main-hand block
use, before the vanilla block or item action. It includes vanilla blocks and
content blocks. Reach, world limits, border, protection and spectator checks
precede it. The first mod returning true consumes the action; later mods do not
hear that use. The client packet is acknowledged normally. Follow-up item and
offhand packets in that server tick are suppressed after consumption.

Sneaking with any nonempty main-hand stack bypasses the hook by default, so a
player can place against a mod block. Pass true as the second opt-in argument
to receive those uses too. Each mod gets at most 32 hook calls per simulation
tick, shared across players. Over-budget uses fall through to normal behavior.

## Messages

Both halves declare `capabilities = ["channel"]` and
`experimental = ["channel"]`. The client uses `channel.send(name, data)`; the
server uses `channel.send-to-player(uuid-or-exact-name, name, data)`. Server
callbacks are `ServerMod::channel_message(player, name, data)`; the client
callback is `Mod::channel_message(name, data)`.

The host supplies the sender's manifest id. A mod cannot select another mod's
id. Each side advertises its running channel-capable mod ids; delivery requires
the same id on both sides, without requiring equal versions. Names are
`[a-z0-9_./-]{1,64}`. Payloads are arbitrary bytes, including empty payloads,
up to 65,536 bytes. Each mod can send 32 messages per tick and receive 32,
across all hooks and, on the server, all players. Excess incoming messages are
dropped. A global pending queue holds at most 256 complete incoming messages.

Sends return an error for an invalid name/size, an exhausted send budget, the
wrong side's function, an unknown/offline player, or a peer that has not
advertised the same running mod. Before negotiation completes, retry on a later
tick. A server without the mod never receives a fallback chat message. Success
means queued, not acknowledged by the other mod; disconnects, reloads and receive
limits can discard queued messages. Mods should include request ids and their own
replies when delivery matters. Clients are untrusted: validate requests and
player permissions in the server callback.

Transport is the existing play-state custom-payload path, on
`horizon:wasm_channel`. Logical messages are split into at most three 24,000-byte
fragments, preserving vanilla's 32,767-byte serverbound limit. Only complete,
ordered messages reach a guest; an incomplete message uses at most 64 KiB per
connection. The dedicated server, LAN host and integrated server share this
path; integrated connections use memory transport.

## Real stack transfers

Capability `world` adds:

- `container-stacks(dimension, pos)`: slots of one loaded block, or none.
- `container-move(dimension, from, from-slot, to, to-slot, count)`.
- `container-to-player(dimension, pos, slot, count, player)`.
- `player-to-container(player, inv-slot, dimension, pos, count)`.

Reads include item id, count, optional plain custom-name text and `display-key`.
The key is a deterministic SHA-256 over the item id and saved component patch,
including damage, names, enchantments, nested contents and component removals,
excluding the outer count. Tagged big-endian values, lossless UTF-16 strings,
ordered lists and sorted compound keys make it independent of process hash
seeds and platforms. It identifies saved data for display/grouping; it is not
an item handle, authorization token or a promise that hash equality establishes
stack compatibility. Transient components that are not saved are outside the
key. Item defaults/resource-pack names are not included. Custom names are plain
text; their full styled components remain on the actual stacks.

Writes currently support bulk storage containers represented by ordinary
container slots: chests (each half separately), barrels, hoppers, dispensers,
droppers and shulker boxes. Special machine/display slots and native mod
containers remain readable but return an error for moves, preserving their
item/slot rules. A missing/unloaded container or invalid slot is an error.

A move reserves `min(count, available)` items, checking full destination capacity
before changing the reservation view. Insufficient capacity, an empty source,
zero count or an identical explicit source/destination slot returns zero and
changes nothing. No destination slot means matching stacks first, then empty
slots, excluding the source slot. Main player inventory slots are 0..35, hotbar
first; armor/offhand are excluded. Both UUID and exact online name work.
Creative mode still moves real items. Overflow is never dropped.

Reservations are shared across mods in a hook batch, so successive storage
reads and moves see earlier reservations. At most 64 move requests per mod per
tick are allowed. The returned count is **reserved**, not a completion receipt:
after hooks return, the server thread compares both live endpoints with their
reserved preimages, checks capacity again using real component equality and
component-aware stack limits, then changes both endpoints atomically. A changed
endpoint (including another inventory API's write), unloaded/replaced container,
or offline player cancels the entire move and logs the conflict. Reread on the
next tick to observe completion. Earlier plain container/block writes can also
invalidate a reservation. No component data is reconstructed from a guest
string or hash. Item stacks are cloned/split from real source stacks, including
their transient components, and both counts change together. Modified chunks,
comparators, open container viewers and player inventories are updated.

There is no extraction handle: moving directly between authoritative endpoints
avoids abandoned handles losing items or replayed handles duplicating them.
