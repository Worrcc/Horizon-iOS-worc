<!-- Generated from crates/horizon-mod-sdk/wit/horizon.wit and the manifest's capabilities by crates/horizon-mods/tests/api_reference.rs; a test fails when this file is out of date. Update it with: HORIZON_WRITE_DOCS=1 cargo test -p horizon-mods --features pack --test api_reference -->
# Mod API reference

The contract between Horizon and its mods: the WIT package `horizon:mods@0.1.0`, every interface, type, function and hook in it, and the capabilities a manifest may list. The guide is [mods.md](../mods.md); stability and versions: [stability.md](stability.md). Manifest `api` lines this game serves: `0.1`, `0.2`, `0.3`.

## Capabilities

| In `capabilities` | Grants | The player reads "May …" |
| --- | --- | --- |
| `game-read` | [`game`](#game) | see your position, health and the time |
| `chat` | [`chat`](#chat) | show you chat messages |
| `hud` | [`hud`](#hud) | draw text on your screen |
| `storage` | [`storage`](#storage) | save its own settings to disk |
| `command` | [`command`](#command) | run commands as you, with your permissions |
| `registries` | data files (`assets/`, `data/`), no interface | add blocks, items, mob effects and entity types to your worlds |
| `ui` | [`ui`](#ui) | show its own screens and a settings page |
| `render` | [`render`](#render) | draw on your screen, shake and zoom the camera |
| `input` | [`input`](#input) | see the keys you hold and your clicks, take over the use button on its own items and the mouse wheel, and add keys |
| `players` | [`players`](#players) | see who is on the server and message them |
| `world` | [`world`](#world) | read and change the world's blocks |
| `recipes` | [`recipes`](#recipes) | read the game's recipes and items and which recipes you know, place recipes in your crafting grid and, in creative mode, give you items |
| `entities` | [`entities`](#entities) | see the creatures around, and move, animate and remove its own, whose breath and stomps hurt what is near them |
| `world-read` | [`client-world`](#client-world) | see the blocks, biomes and light your game has loaded around you |
| `container` | [`container`](#container) | inspect and click your open inventory and container screens |
| `channel` | [`channel`](#channel) | exchange mod messages with the server and its players |
| `atmosphere` | [`atmosphere`](#atmosphere) | change the colours and distance of fog and the sky, and show particle effects |

## World `channel-host`

The game's additive channel imports and optional exports.

- Imports (what the game offers): `channel`, `server-hooks`
- Exports (what the mod provides): `channel-events`

## World `horizon-mod`

A client mod: runs in the player's game.

- Imports (what the game offers): `log`, `game`, `chat`, `hud`, `storage`, `command`
- Exports (what the mod provides): `events`

## World `horizon-server-mod`

A server mod (manifest `side = "server"`, `api = "0.3"`, experimental): runs where the world is simulated, on the dedicated server, a LAN host and single player's own server.

- Imports (what the game offers): `log`, `storage`, `players`, `world`, `menus`, `entities`, `server-hooks`, `channel`
- Exports (what the mod provides): `server-events`

## World `horizon-ui-mod`

A mod with widgets (capability `ui`): the SDK builds every mod against this world. Mods built for `horizon-mod` alone (no `ui-events` export) still load and simply hear nothing of their widgets.

- Imports (what the game offers): `ui`, `render`, `input`, `recipes`, `browser`, `browser-ext`, `canvas`, `client-world`, `container`, `channel`, `atmosphere`, `log`, `game`, `chat`, `hud`, `storage`, `command`
- Exports (what the mod provides): `ui-events`, `container-events`, `channel-events`, `events`

## World `ui-host`

The game's side of `horizon-ui-mod` beyond `horizon-mod`.

- Imports (what the game offers): `ui`
- Exports (what the mod provides): `ui-events`

## World `render-host`

The game's side of capability `render` (imports only).

- Imports (what the game offers): `render`, `input`, `canvas`
- Exports (what the mod provides): 

## World `world-read-host`

The game's side of capability `world-read` (imports only).

- Imports (what the game offers): `client-world`
- Exports (what the mod provides): 

## World `recipes-host`

The game's side of capability `recipes` (imports only).

- Imports (what the game offers): `recipes`, `browser`, `browser-ext`
- Exports (what the mod provides): 

## World `container-host`

The game's additive container imports and optional exports.

- Imports (what the game offers): `container`
- Exports (what the mod provides): `container-events`

## World `atmosphere-host`

The game's side of capability `atmosphere` (imports only).

- Imports (what the game offers): `atmosphere`
- Exports (what the mod provides): 

## `log`

Access: always granted.

The mod's log, in the game's log under the mod's id. Always granted.

### enum `level`

| Case | |
| --- | --- |
| `debug` |  |
| `info` |  |
| `warn` |  |
| `error` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `write` | `func(level: level, message: string)` | One line in the game's log as `[mod <id>] message`, at that level. |

## `game`

Access: capability `game-read`.

Capability `game-read`: the local player, the world clock and the block or living entity the player looks at.

### record `player`

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` | Degrees, as F3 shows them. |
| `pitch` | `f32` | Degrees: -90 looks straight up, 90 straight down. |
| `health` | `f32` | Hit points as vanilla counts them: 20 is full health without effects. |
| `food` | `u32` | 0 to 20, as the hunger bar. |
| `dimension` | `string` | `minecraft:overworld`, `minecraft:the_nether`, ... |

### record `clock`

| Field | Type | |
| --- | --- | --- |
| `game-time` | `u64` | Ticks since the world was created. |
| `day-time` | `u64` | 24000 ticks per day; 0 is 6:00 on the first day. |

### record `hit`

A block along the player's view.

| Field | Type | |
| --- | --- | --- |
| `block-x` | `s32` | The block hit. |
| `block-y` | `s32` |  |
| `block-z` | `s32` |  |
| `x` | `f64` | The point on its face where the view meets it. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `block` | `string` | `minecraft:stone`, ... |

### record `entity-hit`

A living entity under the crosshair, including players and content mobs.

| Field | Type | |
| --- | --- | --- |
| `kind` | `string` | The entity type's namespaced id. |
| `name` | `string` | Custom name, player name, or localized type name. |
| `health` | `f32` | Current and computed maximum hit points (20 is ten hearts). |
| `max-health` | `f32` |  |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `get-player` | `func() -> option<player>` | None on the title screen. |
| `get-clock` | `func() -> option<clock>` | None on the title screen. |
| `raycast` | `func(distance: f32) -> option<hit>` | The first block along the player's view (fluids pass), up to `distance` blocks (at most 1024; past the loaded chunks, across the distant terrain when LOD Distance is on), as of this tick. None when nothing is hit, and on the title screen. |
| `raycast-entity` | `func(distance: f32) -> option<entity-hit>` | API level 16: nearest loaded living entity along the view, before any block, within distance (at most 1024). None outside a world. Health is the real value known to this client as of this tick. |
| `world-id` | `func() -> option<string>` | API level 17: which world the player is in, for a mod that keeps something per world (waypoints): `sp:<save folder>` in single player, `mp:<address>` on a server (`mp:lan` for a LAN game found on the network). None on the title screen. |

## `chat`

Access: capability `chat`.

Capability `chat`: messages only the local player sees.

| Function | Signature | What it does |
| --- | --- | --- |
| `show` | `func(text: string)` | Shown in chat after the mod's name, like a system message. |
| `open` | `func(text: string)` | API level 21: opens the chat box with `text` typed in (at most 256 characters, cursor at the end), as if the player had opened chat and typed it: they edit it and send it, or close it. Nothing is sent by the mod. Closes the mod's own open screen first. Ignored when there is no world, or when chat is hidden in the player's settings (as the chat key is). |

## `hud`

Access: capability `hud`.

Capability `hud`: one panel of text lines per mod, drawn by the game every frame until the mod changes or clears it (no call per frame).

### enum `corner`

| Case | |
| --- | --- |
| `top-left` |  |
| `top-right` |  |
| `bottom-left` |  |
| `bottom-right` |  |

### record `line`

| Field | Type | |
| --- | --- | --- |
| `text` | `string` | Plain text, up to 256 characters. |
| `color` | `u32` | 0xRRGGBB. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-panel` | `func(corner: corner, lines: list<line>)` | Replaces the mod's panel: up to 16 lines in that corner of the screen. |
| `clear` | `func()` | Removes the mod's panel. |

## `storage`

Access: capability `storage`.

Capability `storage`: a per-mod folder of small key/value files, quota'd (docs/mods.md). Keys are `[a-z0-9_.-]{1,64}`, one path segment.

| Function | Signature | What it does |
| --- | --- | --- |
| `get` | `func(key: string) -> option<list<u8>>` | None if `key` has never been `set`, or the key is not well-formed. |
| `set` | `func(key: string, value: list<u8>) -> result<_, string>` | Errs on a malformed key or over quota (bytes for the whole mod). |
| `delete` | `func(key: string)` | No error if `key` was already absent. |

## `command`

Access: capability `command`.

Capability `command`: vanilla commands run as the player, with the player's permissions, after the hook call returns (8 per call). A mod's own commands are not run this way.

| Function | Signature | What it does |
| --- | --- | --- |
| `run` | `func(line: string)` | `line` without its slash; its feedback goes to the chat as if typed. |
| `run-quiet` | `func(lines: list<string>)` | `lines` (each without its slash, at most 256 a call) as `run` does, but with nothing in the chat: no feedback and no failure. In single player and on Horizon's servers, each runs exactly as the player typing it would (the same rights), in order, 64 a tick, and the server logs one line a batch; one that carries a chat message (`say`, `msg`) is refused. On any other server they go as chat commands, and there their feedback shows: a mod running many should say so. |
| `run-mod-quiet` | `func(lines: list<string>)` | API level 20: server mods' commands as the player typing them, with replies and failures discarded. At most 16 lines a call and 64 a tick per mod, without slashes. Vanilla names and unregistered commands are dropped. Works in single player and on supporting Horizon servers; other servers (including older Horizon) drop them silently. |

## `ui`

Access: capability `ui`.

Capability `ui`: widgets the game draws with its own menus' widgets, for a screen the mod owns and a settings page in Options > Mods. Layout and drawing are the game's: a mod says what its widgets hold, never where they sit or how they look, and the game clips everything it is handed.

### record `text-widget`

A line of text.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget; `ui-events` calls echo it. |
| `text` | `string` | Up to 256 characters. |
| `color` | `u32` | 0xRRGGBB; 0 is the menus' white. |

### record `item-widget`

An item's icon and a name beside it, as recipe viewers show them.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget. |
| `item` | `string` | `minecraft:diamond`, `<mod>:<path>`; an unknown id draws a "?". |
| `name` | `string` | The name shown beside the icon (empty: the item's own). |

### record `list-widget`

Rows the player picks one of, as an options page lists choices: a pick is `ui-events::select`, `selected` is the row to mark now.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget; `ui-events::select` echoes it. |
| `rows` | `list<string>` | The rows' texts, at most 512, each up to 256 characters. |
| `selected` | `option<u32>` | The row to mark as the current choice, if any. |

### record `button-widget`

A thing on a mod's screen or settings page. A button as the options pages have them (200 × 20): a click is `ui-events::select(id, 0)`. Mods cycle its label for a choice ("Corner: Top Left").

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mod's own name for the widget. |
| `label` | `string` | Up to 256 characters. |

### variant `widget`

| Case | Payload | |
| --- | --- | --- |
| `text` | `text-widget` | Text the player reads. |
| `button` | `button-widget` | A button. |
| `item` | `item-widget` | An item's icon and name. |
| `list` | `list-widget` | Rows the player picks from. |

### record `screen`

A screen the mod owns: a title over widgets stacked down the middle, the Done button the game adds, and Esc to leave it.

| Field | Type | |
| --- | --- | --- |
| `title` | `string` | Its title, up to 256 characters. |
| `widgets` | `list<widget>` | Its widgets, in order, at most 64. |

| Function | Signature | What it does |
| --- | --- | --- |
| `open-screen` | `func(screen: screen)` | Show `screen` over the game; a second call while it is open replaces it. Only the last call of one hook counts. |
| `close-screen` | `func()` | Close the mod's screen when it is open (the mod hears no `closed` for this: it was its own doing). |
| `set-settings` | `func(widgets: list<widget>)` | The mod's page in Options > Mods, reached from a settings button on its row: the widgets it shows, an empty list for no page. The player opening the page is `ui-events::settings-opened`: push fresh widgets then; later calls update the page while it shows. |

## `ui-events`

Access: exported by the mod: the game calls it.

What the game calls on a mod that shows widgets (capability `ui`): what the player did with them.

### enum `pointer-kind`

API level 18: pointer action on the mod's canvas screen.

| Case | |
| --- | --- |
| `down` |  |
| `up` |  |
| `drag` |  |
| `move` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `select` | `func(id: u32, row: u32)` | A row of one of the mod's `list` widgets was picked: the widget's `id` and the row, on the screen or the settings page that showed it. |
| `closed` | `func()` | The player left the mod's screen (Esc or Done); not for a screen the mod closed itself or one that went away with the mod. |
| `settings-opened` | `func()` | The player opened the mod's settings page in Options > Mods: call `ui::set-settings` now to fill it with fresh widgets. |
| `pointer` | `func(kind: pointer-kind, x: f32, y: f32, button: u32)` | GUI pixels from the window top left; buttons 0 left, 1 right, 2 middle. Drag is movement with a button held. Missing exports are ignored. |
| `scroll` | `func(x: f32, y: f32, steps: f32)` | Wheel at GUI coordinates; positive steps away from the player. |
| `screen-key` | `func(key: string, down: bool)` | A key.keyboard.* key pressed or released on the canvas screen. Escape closes the screen instead. Missing exports are ignored. |

## `container`

Access: capability `container`.

API level 22. Capability `container` (experimental: also list it in `experimental`): inspect and click the open survival/container screen. Creative inventory is excluded. Calls are synchronous: state after click includes client prediction, and the normal container-click packet is sent.

### enum `part`

| Case | |
| --- | --- |
| `player-main` |  |
| `hotbar` |  |
| `container` |  |
| `crafting-output` |  |
| `armor` |  |
| `offhand` |  |
| `other` |  |

### record `stack`

| Field | Type | |
| --- | --- | --- |
| `item` | `string` | Namespaced item id. Count is not part of same-key. |
| `count` | `u32` |  |
| `max-stack` | `u32` | The item's component-aware maximum stack size. |
| `same-key` | `u64` | Opaque item + components identity, equal for stacks that can merge. Valid during this game session; never persist it. |

### record `slot`

| Field | Type | |
| --- | --- | --- |
| `index` | `u32` | Opaque slot index for click and pointer/scroll events, stable while this menu is open. |
| `x` | `f32` | Top left of the 16x16 item box, in GUI pixels from the window top left. |
| `y` | `f32` |  |
| `item` | `string` | Empty string and count 0 for an empty slot; same-key is then 0. |
| `count` | `u32` |  |
| `max-stack` | `u32` | min(slot maximum for the item, item maximum). For an empty slot, uses the carried item, or the ordinary 64-stack limit with no carried item. |
| `same-key` | `u64` |  |
| `part` | `part` |  |
| `accepts-item` | `bool` | Whether the slot may accept the carried item (or its own item with an empty cursor; an ordinary stack for an empty slot). This is the slot's placement rule, not a test of remaining space. Output slots are false. |

### record `container-state`

| Field | Type | |
| --- | --- | --- |
| `slots` | `list<slot>` |  |
| `carried` | `option<stack>` |  |
| `x` | `f32` | Container background rectangle, in GUI pixels from the window top left. |
| `y` | `f32` |  |
| `width` | `f32` |  |
| `height` | `f32` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `state` | `func() -> option<container-state>` | None without an open container screen, including creative inventory. Reads the live menu, including clicks earlier in the same callback. |
| `click` | `func(slot: u32, button: u32, shift: bool)` | A real slot click: button 0 left or 1 right; shift selects QUICK_MOVE, otherwise PICKUP. Invalid slots/buttons and closed screens are ignored. At most 256 mutations per hook. Shift latches do not apply: `shift` alone decides. Uses the player's prediction and network path in single player, LAN and multiplayer alike. |
| `take-drag` | `func()` | Take over the press in progress (either button). Stop vanilla quick craft without sending it, and suppress that button's coming release click. Works in pointer(down), before vanilla starts the drag. No button down: no-op. Screen close clears the takeover and pending release suppression. |
| `set-shift-latch` | `func(on: bool)` | Treat the player's container clicks and the pointer events' shift as Shift-held while on (not `click`, whose `shift` is used as given). Per mod, ORed together; survives screen changes, cleared on unload/stop. |
| `set-screen-button` | `func(id: u32, dx: f32, dy: f32, icon: string, lit: bool, tooltip: string)` | Set/replace a 20x20 slot-style button, relative to the container background's top left in GUI pixels. At most 8 per mod; further ids are ignored. Finite offsets only. Item icon; lit adds a gold frame; tooltip on hover (256 chars). Click/tap fires container-events.button(id) and consumes press and release. Kept across screens until cleared or the mod stops. Unknown icons draw nothing. |
| `clear-screen-button` | `func(id: u32)` | Remove this mod's button; an unknown id is ignored. |

## `container-events`

Access: exported by the mod: the game calls it.

Optional API level 22 exports for capability `container`. Each missing function is ignored, including mods built before this interface existed.

### enum `pointer-kind`

| Case | |
| --- | --- |
| `down` |  |
| `up` |  |
| `drag` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `pointer` | `func(kind: pointer-kind, x: f32, y: f32, button: u32, slot: option<u32>, shift: bool)` | Mouse and touch, synchronously BEFORE vanilla handles the event. Window GUI pixels; button 0 left, 1 right, 2 middle. Slot uses state(). Shift includes physical Shift and every running mod's shift latch. |
| `scroll` | `func(x: f32, y: f32, steps: f32, slot: option<u32>) -> bool` | Wheel and two-finger touch scroll, before vanilla; positive away from the player. First mod returning true consumes; later mods are not called. |
| `button` | `func(id: u32)` | The player clicked/tapped one of this mod's screen buttons. |
| `opened` | `func()` | A survival or container screen opened (never creative inventory). |
| `closed` | `func()` | The container screen closed; state() is now None. |

## `render`

Access: capability `render`.

Capability `render` (experimental: the manifest needs `experimental = ["render"]`): shapes the game draws for the mod with its own renderer, kept until the mod replaces them (no call per frame). A mod never supplies shader code.

### record `world-line`

A world-space segment, in blocks; whole numbers are block corners.

| Field | Type | |
| --- | --- | --- |
| `x1` | `f64` |  |
| `y1` | `f64` |  |
| `z1` | `f64` |  |
| `x2` | `f64` |  |
| `y2` | `f64` |  |
| `z2` | `f64` |  |

### enum `corner`

A corner of the screen, as `hud`'s panels use them.

| Case | |
| --- | --- |
| `top-left` |  |
| `top-right` |  |
| `bottom-left` |  |
| `bottom-right` |  |

### record `rect`

A rectangle in GUI pixels: the top left corner and the size.

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |
| `w` | `f32` |  |
| `h` | `f32` |  |

### record `point`

A point in GUI pixels from the screen's top left.

| Field | Type | |
| --- | --- | --- |
| `x` | `f32` |  |
| `y` | `f32` |  |

### record `fill-shape`

| Field | Type | |
| --- | --- | --- |
| `rect` | `rect` |  |
| `color` | `u32` | 0xAARRGGBB. |

### record `quad-shape`

Any four-cornered shape (a wheel's segment, an arrow): the corners in order round it.

| Field | Type | |
| --- | --- | --- |
| `corners` | `tuple<point, point, point, point>` |  |
| `color` | `u32` | 0xAARRGGBB. |

### record `sprite-shape`

| Field | Type | |
| --- | --- | --- |
| `sprite` | `string` | One of the game's GUI sprites, as `gui/sprites/` names them (`hud/heart/full`, `widget/button`), the resource packs' versions included, drawn over `rect` as vanilla draws it: nine-sliced when its `gui.scaling` says so (`tooltip/frame`, `widget/button`: the borders keep their size), stretched otherwise. An unknown one draws nothing. |
| `rect` | `rect` |  |
| `color` | `u32` | A tint, 0xAARRGGBB: 0xFFFFFFFF draws it as it is. |

### record `item-shape`

| Field | Type | |
| --- | --- | --- |
| `item` | `string` | `minecraft:diamond`, `<mod>:<path>`; an unknown one draws nothing. |
| `at` | `point` | The icon's top left. |
| `scale` | `f32` | 1.0 is the hotbar's 16 × 16. |

### record `text-shape`

| Field | Type | |
| --- | --- | --- |
| `text` | `string` | Up to 256 characters, one line. |
| `at` | `point` |  |
| `color` | `u32` | 0xRRGGBB, as `hud`'s lines. |
| `centered` | `bool` | Centred on `at.x` instead of starting there. |
| `shadow` | `bool` |  |

### variant `shape`

| Case | Payload | |
| --- | --- | --- |
| `fill` | `fill-shape` |  |
| `quad` | `quad-shape` |  |
| `sprite` | `sprite-shape` |  |
| `item` | `item-shape` |  |
| `text` | `text-shape` |  |

### record `frame-rate`

| Field | Type | |
| --- | --- | --- |
| `fps` | `u32` | Frames drawn in the last second. |
| `min` | `u32` | The slowest frame's rate, and the 1% low (the 99th percentile frame time's rate), over the last 5 seconds. |
| `low` | `u32` |  |

### record `zoom`

| Field | Type | |
| --- | --- | --- |
| `factor` | `f32` | The field of view is divided by it: 1 is none, at most 10. |
| `smooth` | `bool` | Ease in and out through the game's field-of-view easing, as the spyglass does, instead of at once. |
| `turn` | `f32` | Mouse turning is multiplied by it while zoomed: 1 keeps the player's sensitivity, at least 0.05. |

### record `minimap`

| Field | Type | |
| --- | --- | --- |
| `corner` | `corner` |  |
| `size` | `f32` | Its side in GUI pixels, 16 to 256. |
| `blocks-per-pixel` | `f32` | World blocks per map pixel, 0.5 to 4; the map is 128 × 128 pixels. |
| `rotate` | `bool` | The map turns with the player, who faces up, and an N on its rim marks north. Otherwise north is up and the player's arrow turns. |
| `round` | `bool` | Round, the corners left clear; otherwise square. |

### record `far-terrain`

| Field | Type | |
| --- | --- | --- |
| `distance` | `u32` | How far the distant terrain reaches, in chunks: 64 to 1024. On integrated graphics other than Apple's the game caps it lower. |
| `reduced` | `bool` | Coarser far tiles, as the Far Detail setting's Reduced: 2 to 4 ms less GPU time per frame on integrated graphics. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-world-lines` | `func(lines: list<world-line>, color: u32, hidden-color: u32)` | API level 20: replace up to 4096 segments, at the block outline's width. ARGB color is depth tested; hidden-color draws only occluded parts (alpha zero hides). Hidden with F1; cleared on stop and leaving the world. Empty removes them. |
| `set-hud` | `func(shapes: list<shape>)` | Replaces the mod's HUD shapes: up to 1024, drawn every frame in order over the HUD (after the hotbar, under chat and screens), hidden with the HUD (F1). An empty list removes them. |
| `gui-size` | `func() -> tuple<f32, f32>` | The screen's size in GUI pixels (width, height), as of this tick. |
| `shake` | `func(strength: f32, seconds: f32)` | Shakes the camera: a jolt of `strength` (0 to 1, 1 the hardest) that fades out over `seconds` (at most 10). Jolts add up to 1. The view turns, the player doesn't, and the player's Distortion Effects option scales it (0 turns it off). |
| `set-corner` | `func(corner: corner, width: f32, height: f32, shapes: list<shape>)` | Replaces the mod's corner panel: shapes in GUI pixels from the top left of a `width` × `height` box that the game places in `corner`, stacked with the game's overlays and the other mods' panels (under the status effects at the top right), as `hud`'s panels are. |
| `clear-corner` | `func()` | Removes the mod's corner panel. |
| `text-width` | `func(text: string) -> f32` | The width of one line of text in GUI pixels, in the HUD's font: exact for Latin letters (U+0000 to U+024F), 6 per other character. |
| `frame-stats` | `func() -> frame-rate` | The game's frame counter, recomputed twice a second. |
| `set-zoom` | `func(zoom: zoom)` | Zooms the camera until the next call (`factor` 1 ends it), only while playing: a screen opening or the spyglass's own view (which wins) leave the camera vanilla's. With several mods, the strongest zoom counts. |
| `set-minimap` | `func(minimap: option<minimap>)` | Shows a map of the world around the player in `corner`, stacked with the game's overlays and the mods' panels, until the next call (none hides it). It looks like a map item: vanilla's map colours with their height shading and water depth, and the player's arrow. Underground, under a roof and in the Nether it maps the floor around the player's height. The game keeps the map's tiles and refreshes them within its own frame budget; hidden, they are dropped. One map shows at a time: the first loaded mod's. |
| `set-far-terrain` | `func(far-terrain: option<far-terrain>)` | Draws the world beyond the render distance as distant terrain, out to `distance` chunks, until the next call (none ends it). The game builds it on its own workers, from the chunks already loaded or saved and, where nobody has been, from the world's generator; the simulation distance doesn't change. Only worlds whose generator the game has (single player and a LAN world you host) show it. The player's own LOD Distance setting, when on, wins. One far terrain shows at a time: the first loaded mod's. |

## `input`

Access: capability `input`.

Capability `input` (experimental: the manifest needs `experimental = ["input"]`): the use button on items the mod claims, and the mod's own keys. Events wait for `poll`, at most 256.

### type `corner`

`type corner = corner`

### record `aim-assist`

API level 27: assistance for controller and touch look.

| Field | Type | |
| --- | --- | --- |
| `targets` | `list<string>` | Entity ids, "*" (every mob but players) or "#hostile" (monsters); at most 32 entries. |
| `range` | `f32` | Blocks, clamped by the host to 2..32. |
| `cone` | `f32` | Degrees from the look direction, clamped to 1..20. |
| `slowdown` | `f32` | 0..0.9 slowdown inside the cone. |
| `pull` | `f32` | Degrees per second toward the target, 0..60, only while the player turns. |
| `controller` | `bool` | Apply to controller look. |
| `touch` | `bool` | Apply to touch look (finger on the look area). |

### enum `look-device-kind`

| Case | |
| --- | --- |
| `mouse` |  |
| `controller` |  |
| `touch` |  |

### record `touch-button`

API level 20: a square button above the game's touch controls.

| Field | Type | |
| --- | --- | --- |
| `label` | `string` | Up to four characters; also the fallback for an unknown icon item. |
| `icon` | `option<string>` | A namespaced item id, drawn at 1x (16x16) instead of the label. Malformed ids are errors; an unknown item falls back to the label. |
| `anchor` | `corner` |  |
| `dx` | `f32` | Offset inward from the corner, in GUI pixels. |
| `dy` | `f32` |  |
| `size` | `f32` | Side in GUI pixels, clamped to 16..64. |
| `lit` | `bool` |  |

### record `touch-context`

API level 25: when a registered key's own or automatic touch button shows by default.

| Field | Type | |
| --- | --- | --- |
| `riding` | `list<string>` | Ride one of these entity ids: "*" any mount, "ns:*" any in a namespace. Empty means no riding condition. At most 32 validated resource ids or wildcards. |
| `holding` | `list<string>` | Hold one of these item ids in the main hand: "*" anything, "ns:*" a namespace. Empty means no holding condition. At most 32 validated resource ids or wildcards. |

### record `held-item`

The main hand's stack.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:spyglass`, `<mod>:<path>`. |
| `count` | `u32` |  |
| `slot` | `u32` | The hotbar slot, 0 to 8. |

### record `item-use`

| Field | Type | |
| --- | --- | --- |
| `item` | `held-item` |  |
| `ticks` | `u32` | Game ticks the button was held (0 on `use-start`). |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `use-start` | `item-use` | The use button went down with a claimed item in the main hand. The game does nothing else with the click: no placing, no eating, no swing. |
| `use-stop` | `item-use` | It came up again, or the item left the hand, or a screen opened. |
| `key-down` | `string` | A key `register-key` named went down or up while playing (no screen open). The game's own mapping on that key still runs. A screen opening lets every held key up. |
| `key-up` | `string` |  |
| `wheel` | `f32` | Mouse wheel steps while the mod holds the wheel (`claim-wheel`): positive away from the player, as the hotbar reads them. |

### record `mapping-state`

A mapping as the player has bound it.

| Field | Type | |
| --- | --- | --- |
| `label` | `string` | The bound key's name as Controls shows it: `W`, `Space`, `LMB`. |
| `down` | `bool` | Held down now (as of this frame for a mod with a `frame` hook, else this tick). |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-aim-assist` | `func(config: option<aim-assist>) -> result<_, string>` | API level 27: replaces this mod's aim assist; none turns it off. Several mods: each field takes its weakest value (enabled only if every mod that set it enabled it). Targets must match every mod's list. Errors leave the previous config intact. |
| `look-device` | `func() -> look-device-kind` | API level 27: the device that turned the view last frame, so a settings page can show the right preview text. Defaults to mouse; unchanged until another look movement. |
| `set-touch-button` | `func(name: string, button: option<touch-button>) -> result<_, string>` | API level 20: set/remove a button for a registered key (at most four per mod). A finger sends key-down/key-up and consumes the touch. Clamped fully on screen, including the small button's outer selection ring. Hit area at least 24x24, centred. Game-mode slot artwork, or a plain plate under 20 px; lit adds a gold frame. Player touch-layout overrides win over the anchor/offset. Removing an explicit button restores the key's automatic button. |
| `set-auto-button-shown` | `func(name: string, shown: bool) -> result<_, string>` | API level 24: whether a registered key's automatic touch button (one for a key with no `set-touch-button`) starts shown. Automatic buttons start hidden unless the mod asks; the player's own choice (the mod's settings page or Edit Touch Layout) always wins. |
| `set-touch-button-context` | `func(name: string, context: option<touch-context>) -> result<_, string>` | API level 25: set/clear the registered key's default touch visibility context. Both nonempty lists must match; none clears it. The player's Show choice (Always/Riding/Never) wins. Automatic buttons still start hidden unless opted in with set-auto-button-shown. Errors leave the previous context intact. |
| `touch-active` | `func() -> bool` | API level 20: whether the player is using the touch screen (the game's touch controls show while playing). It stays true on screens, so a mod can offer touch buttons on its own. |
| `claim-items` | `func(ids: list<string>)` | The items whose use the mod handles instead of the game, by id (at most 64). Replaces the previous list. |
| `register-key` | `func(name: string, label: string, key: string) -> result<_, string>` | A key of the mod's: `name` comes back in `key-down` and `key-up`, `label` says what it does, `key` is the key as options.txt writes it (`key.keyboard.r`), the default: the player can bind another one on the mod's page in Options > Mods, and that one wins. At most 16; an unknown key is an error. While touch controls are shown, keys without set-touch-button get automatic 20px slot buttons, in registration order down the right side. The player can move or hide them in Edit Touch Layout. Controller Settings > Bindings lists every registered key, unbound by default. A player-chosen button or two-button combination sends the same key-down/key-up events while playing, independently of the keyboard and touch bindings. |
| `held` | `func() -> option<held-item>` | The main hand's stack, as of this tick; none when it is empty. |
| `poll` | `func() -> list<event>` | The events since the last call, oldest first. |
| `claim-wheel` | `func(on: bool)` | Takes the mouse wheel while playing (true) or gives it back: while taken, its steps are the mod's `wheel` events and the hotbar doesn't move. The game gives it back when the mod stops. |
| `mapping` | `func(name: string) -> option<mapping-state>` | A movement or click mapping as the player has bound it: `key.forward`, `key.left`, `key.back`, `key.right`, `key.jump`, `key.sneak`, `key.sprint`, `key.attack` or `key.use`. None for another name. |
| `clicks-per-second` | `func() -> tuple<u32, u32>` | Clicks of the attack and use buttons in the last second. |

## `canvas`

Access: capability `render`.

Capability `render` (experimental), API level 17: the mod's own pixels on the HUD. Images live on one shared atlas page the game draws from; a mod fills them with `update-image` and shows them in a corner box with `set-corner`, beside the `render` shapes. Also the frame budget a mod with heavy `frame` work asks for, and how much of it is left.

The default epoch mode uses elapsed wall time (host calls included); `HORIZON_MODS_LIMIT=fuel` retains instruction accounting. Internally 1,000,000 epoch work units represent 1 ms, while `work-left` stays normalized to about 1,000,000 at call start in both modes. Frame grants retain microsecond precision; requests affect subsequent frame calls and never extend the current hard limit. See [the budget guide](../mods.md).

### type `corner`

`type corner = corner`

### type `rect`

`type rect = rect`

### type `point`

`type point = point`

### type `fill-shape`

`type fill-shape = fill-shape`

### type `quad-shape`

`type quad-shape = quad-shape`

### type `sprite-shape`

`type sprite-shape = sprite-shape`

### type `text-shape`

`type text-shape = text-shape`

### type `item-shape`

`type item-shape = item-shape`

### type `image`

An image of the mod's, from `create-image`.

`type image = u32`

### record `image-shape`

Part of an image, drawn stretched into a rectangle.

| Field | Type | |
| --- | --- | --- |
| `image` | `image` |  |
| `src` | `rect` | The texels of the image drawn (clipped to it). |
| `dst` | `rect` | Where, in GUI pixels from the box's top left. |
| `tint` | `u32` | A tint, 0xAARRGGBB: 0xFFFFFFFF draws the image as it is, a lower alpha fades it. |
| `round` | `bool` | Clipped to the circle inscribed in `dst` (a round minimap). |

### variant `shape`

What a canvas corner box holds.

| Case | Payload | |
| --- | --- | --- |
| `image` | `image-shape` |  |
| `fill` | `fill-shape` |  |
| `quad` | `quad-shape` |  |
| `sprite` | `sprite-shape` |  |
| `text` | `text-shape` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `create-image` | `func(width: u32, height: u32) -> result<image, string>` | A new image of `width` × `height` texels (each side 1 to 1024; at most 1024 × 1024 texels for all of a mod's images together), on the shared mod-image atlas page. It starts transparent. An error when a side is out of range, the mod's share is used up or the page is full. |
| `update-image` | `func(image: image, x: u32, y: u32, width: u32, height: u32, rgba: list<u8>) -> result<_, string>` | Replaces the texels from (`x`, `y`) to (`x + width`, `y + height`) of `image` with `rgba` (`width × height × 4` bytes, straight alpha, rows top first). Shown from the next frame. At most 1024 KiB a frame for each mod: past it, an error and nothing changes. An unknown image, a rectangle outside it or the wrong number of bytes is an error. |
| `drop-image` | `func(image: image)` | Frees `image` and its place on the page; an unknown one is ignored. A mod's images go when it stops. |
| `set-corner` | `func(corner: corner, width: f32, height: f32, shapes: list<shape>)` | Replaces the mod's canvas box: shapes in GUI pixels from the top left of a `width` × `height` box that the game places in `corner`, stacked with the game's overlays, the mods' panels and their `render` corner panels, under the status effects at the top right; hidden with the HUD (F1), and the top left gives way to the debug screen (F3). Up to 1024 shapes; an image shape of an unknown image draws nothing. |
| `set-placed` | `func(anchor: corner, dx: f32, dy: f32, width: f32, height: f32, shapes: list<shape>)` | API level 18: replaces the mod's canvas box, offset inward from the anchor in GUI pixels, clamped fully on screen. Same limits as set-corner; independent of corner stacking and hidden with F1. |
| `set-box-key` | `func(name: option<string>) -> result<_, string>` | API level 21: a finger on the drawn canvas box sends key-down for this input.register-key name, and lifting sends key-up. Only while playing with touch controls shown; consumes the touch. Game and mod touch buttons win overlaps. none restores the default of taking no touches. An unknown key name is an error. |
| `open-canvas-screen` | `func(title: string, shapes: list<shape>)` | API level 18: opens the mod's full-window canvas (capabilities ui and render). Up to 4096 shapes in GUI pixels from the window top left, clipped to the window. Hides its HUD box; Escape sends ui-events.closed. |
| `set-canvas-screen` | `func(shapes: list<shape>)` | Replaces the open canvas screen's shapes; ignored without a screen. |
| `set-screen-items` | `func(items: list<item-shape>)` | API level 28: replaces the open canvas screen's item layer (ui and render). Ignored without this mod's canvas screen. First 256 items; unknown ids draw nothing. render.item-shape: GUI-pixel top left, scale 1 = 16 x 16, scale clamped to 0..16 (nonfinite becomes 1); nonfinite positions become 0. When nonempty, draw non-text shapes in list order, then items in list order, then text shapes in list order. No count overlays; use text. set-canvas-screen retains items; opening/reopening or closing clears them. |
| `set-box-items` | `func(items: list<item-shape>)` | API level 28: same item layer for the existing corner/placed box, in GUI pixels from its top left. First 256; same sanitizing and draw order as set-screen-items. Placed boxes clip items too. Ignored without a box; set-corner, set-placed and clear-corner clear its items. Needs render only. |
| `close-canvas-screen` | `func()` | Closes the mod's canvas screen without a closed event. |
| `clear-corner` | `func()` | Removes the mod's canvas box. |
| `request-frame-budget` | `func(micros: u32) -> u32` | Asks for a CPU budget for each `frame` call, in microseconds: 100 by default, at most 500, or 4000 while this mod's canvas screen is open. Remembers the request: closing the screen immediately restores the 500 maximum; reopening reapplies a larger request. Thermal pressure may limit screens to 500 too. Returns what is granted now (the Mods page shows it). The frame hook's hard call limit always wins, with headroom to return. The mod has no clock: `work-left` says how much of the soft budget is left. |
| `work-left` | `func() -> u64` | How much of this hook call's budget is left, about 1000000 at its start and 0 once it is spent: a mod that works `while work-left() > reserve` keeps to its budget. In `frame` calls the budget is the granted one, measured on this device as the mod runs (host calls included); in other hooks it is the hook's own work limit. Spending past it is allowed up to the hook's hard limit, past which the mod is stopped. |

## `client-world`

Access: capability `world-read`.

Capability `world-read` (experimental), API level 17: the blocks, biomes and light the player's game has loaded, as the window shows them, in single player and on any server alike (nothing the server didn't send), read in whole chunks for minimaps; and the block table and textures of the loaded resource packs.

### record `dimension-info`

The player's dimension.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:overworld`, ... |
| `min-y` | `s32` | Its lowest block y and its height in blocks. |
| `height` | `u32` |  |
| `ceiling` | `bool` | It has a roof (the Nether's bedrock ceiling). |

### enum `tint-source`

Where a face's colour comes from, besides its texture.

| Case | |
| --- | --- |
| `none` |  |
| `grass` |  |
| `foliage` |  |
| `dry-foliage` |  |
| `water` |  |
| `constant` | The state's `constant-tint` (a lily pad, a redstone wire's power, a stem's age). |

### record `face`

What a block state's model draws on one side: its first face there.

| Field | Type | |
| --- | --- | --- |
| `sprite` | `u32` | Its texture, an index into `block-states.sprites`; 0xFFFFFFFF when the side draws nothing. |
| `tint` | `tint-source` |  |

### record `block-states`

Every block state the game knows, as the loaded resource packs draw them (`block-table`). The lists by state are indexed by state id (`columns`' ids).

| Field | Type | |
| --- | --- | --- |
| `version` | `u64` | Changes when the resource packs reload: build colours again then. |
| `blocks` | `list<string>` | The blocks, `minecraft:grass_block`, ... |
| `sprites` | `list<string>` | The texture names faces use, `block/grass_block_top` (a mod's with its namespace), for `texture`. |
| `state-block` | `list<u16>` | Each state's block, an index into `blocks`. |
| `state-flags` | `list<u16>` | Each state's facts, bits from the lowest: air (1), drawn as nothing (2: a barrier, light), water (4), lava (8), waterlogged (16), see-through like glass or leaves (32), no collision like a plant (64), a full face on top (128); and the light it gives (0 to 15) << 8. |
| `state-top` | `list<face>` | What each state draws facing up, and facing south; a model with nothing flat on either (a flower's crossed planes) gives its first face for both. |
| `state-side` | `list<face>` |  |
| `constant-tint` | `list<u32>` | Each state's tint for tint source `constant`, 0xRRGGBB (0 when none). |
| `map-color` | `list<u32>` | Each state's vanilla map colour, 0xRRGGBB (0 for none): a fallback. |

### record `texture-pixels`

A texture's pixels: its first frame when it is animated, at the size the game holds block textures (every one the same).

| Field | Type | |
| --- | --- | --- |
| `width` | `u32` |  |
| `height` | `u32` |  |
| `rgba` | `list<u8>` | `width × height × 4` bytes, straight alpha, rows top first. |

### variant `layer`

Which block of each column `columns` reads.

| Case | Payload | |
| --- | --- | --- |
| `surface` |  | The top block (what is under the open sky). |
| `below` | `s32` | The floor under this height: the first block going down from it (it included), as the game's own minimap maps caves and the Nether. |

### record `chunk-columns`

A chunk's 256 columns, x fastest: column `z * 16 + x`.

| Field | Type | |
| --- | --- | --- |
| `revision` | `u64` | The chunk's change counter, as `changes` counts: the same value means the same blocks. |
| `tops` | `list<s16>` | Each column's top block's y in the layer, water and lava included; -32768 for none (air all the way down). |
| `states` | `list<u32>` | For each column, `depth` state ids from its top block down (air below the world's bottom). |
| `water` | `list<u8>` | Each column's water depth from its top block down (0 when the top block isn't water), at most 255. |
| `light` | `list<u8>` | The light just above each top block: sky light << 4 \| block light. |
| `tints` | `list<u32>` | Each column's grass, foliage and water colours, 0xRRGGBB, three per column, at its top block's biome (as the game draws them, before the biome blend). |

### record `prepared-chunk`

API level 23: columns after skipping their leading see-through states.

| Field | Type | |
| --- | --- | --- |
| `columns` | `chunk-columns` | As `columns`, with only tops and states changed. Water, light, tints (including the original top's biome colours) and revision stay exactly as read; light is not resampled at the lowered top. |
| `skips` | `list<u8>` | 256 counts, x fastest. Zero for an empty column, a solid first state, or no solid within depth. Otherwise the first solid's index k. |
| `skipped-states` | `list<u32>` | Original leading k states for each nonzero skip, top down, in column order, concatenated without padding. The skips are their lengths: no per-column lists or allocations. Use these states with the mod's albedos to compute plant/glass overlay colours. |

### record `harvest-info`

API level 24: what mines a block, from its loaded tags (content mods' included).

| Field | Type | |
| --- | --- | --- |
| `tools` | `list<string>` | Tools that mine it faster than a hand: any of "pickaxe", "axe", "shovel", "hoe" (in that order), as the game's own tool rules (content mods' included) decide. |
| `tier` | `option<string>` | The lowest material whose tool drops it: "wooden", "stone", "iron", "diamond" or "netherite"; none when it needs no tool (or no such tool drops it). |
| `tier-tool` | `option<string>` | The tool of `tier` that drops it ("pickaxe"...), when there is one. |
| `needs-tool` | `bool` | It drops nothing unless broken with the right tool. |
| `held-drops` | `bool` | The item in the player's main hand would drop it (true when it needs no tool). |
| `hardness` | `f32` | Its destroy time (hardness); negative for unbreakable. |

| Function | Signature | What it does |
| --- | --- | --- |
| `block-at` | `func(x: s32, y: s32, z: s32) -> option<string>` | API level 20: the client-loaded block state, e.g. minecraft:oak_log[axis=y]. None for unloaded chunks and on the title screen. |
| `dimension` | `func() -> option<dimension-info>` | The player's dimension; none on the title screen. |
| `block-table` | `func() -> block-states` | The block table (about 30000 states: read it in `init` or when its version changes, not every frame). Empty lists on a game without block models (a dedicated server). |
| `table-version` | `func() -> u64` | The block table's `version` now, without reading the table: cheap enough to ask every tick, to know when the packs reloaded. |
| `texture` | `func(name: string) -> option<texture-pixels>` | A texture by its name in `block-states.sprites`, from the loaded resource packs (a higher pack's over a lower one's); none for one no block model uses. |
| `columns` | `func(chunks: list<tuple<s32, s32>>, layer: layer, depth: u8) -> result<list<option<chunk-columns>>, string>` | The chunks at these chunk coordinates (block x >> 4, z >> 4), each column's `depth` blocks (1 to 16) from the top in `layer`; none for a chunk the game hasn't loaded. At most 16 chunks a call: more, or a depth out of range, is an error. |
| `set-see-through` | `func(states: list<u32>) -> result<_, string>` | API level 23: replaces this mod's see-through state set (at most 65536 ids; duplicates allowed). Empty initially; unlisted ids are solid. Register again after a block-table/looks rebuild. Other mods' sets are independent. An error leaves the previous set intact. |
| `prepared-columns` | `func(chunks: list<tuple<s32, s32>>, layer: layer, depth: u8) -> result<list<option<prepared-chunk>>, string>` | API level 23: the same order, missing chunks and limits as `columns`. For each nonempty column, find the first state not in this mod's see-through set within depth. If its index k > 0, lower the top by k, shift states up by k and pad with the original deepest state. k == 0 or no solid found leaves the column unchanged (skip zero). Fixed-size data stays in flat primitive lists for cheap guest lifting. |
| `harvest` | `func(block: string) -> option<harvest-info>` | API level 24: a block id or state's harvest facts, for the player's held item; none for an unknown block or on the title screen. |
| `changes` | `func(since: u64, radius: u32) -> tuple<u64, list<tuple<s32, s32>>>` | The chunks within `radius` chunks of the player (at most 64) whose change counter moved past `since`: changed, loaded or unloaded since (an unloaded one now reads none), nearest first, at most 4096. Returns the counter now, for the next call; 0 asks for every loaded chunk. |
| `light` | `func(x: s32, y: s32, z: s32) -> option<u8>` | Light at a block, sky light << 4 \| block light; none where no chunk is loaded. |
| `biome` | `func(x: s32, y: s32, z: s32) -> option<string>` | The biome at a block (`minecraft:plains`), as the chunk stores it (one per 4 × 4 × 4 cell); none where no chunk is loaded. |

## `events`

Access: exported by the mod: the game calls it.

What the game calls. Only the hooks `init` asks for are ever called.

### record `registration`

| Field | Type | |
| --- | --- | --- |
| `commands` | `list<string>` | Chat commands, without the slash. A name vanilla already uses is refused (vanilla's command runs). |
| `tick` | `bool` | Call `tick` every game tick (20 per second while playing). |
| `frame` | `bool` | Call `frame` every rendered frame. Budget: 0.1 ms. |
| `chat` | `bool` | Call `chat` for each chat line the player receives. |

| Function | Signature | What it does |
| --- | --- | --- |
| `init` | `func() -> result<registration, string>` | Once, after loading. An error keeps the mod off and is shown. |
| `tick` | `func(tick: u64)` | Each game tick while in a world, with the game's tick counter. |
| `frame` | `func(seconds: f32)` | Each rendered frame, with the seconds since the last one. |
| `command` | `func(name: string, args: string) -> result<option<string>, string>` | `/name args`: ok(some(text)) is shown as feedback, err(text) in red. |
| `chat` | `func(text: string)` | Each chat line the player receives, as plain text. |

## `recipes`

Access: capability `recipes`.

Capability `recipes` (experimental): the recipes and items the game knows, for recipe viewers. In single player and on Horizon's servers they are the world's; on any other server they are vanilla 26.3's (a client is sent no others), while `known` follows that server's book.

### record `stack`

An item of `items`, by its index there, with a count and the components it has beyond the item's own, in vanilla's item syntax (`[potion_contents={potion:"minecraft:swiftness"}]`), or "".

| Field | Type | |
| --- | --- | --- |
| `item` | `u32` |  |
| `count` | `u32` |  |
| `components` | `string` |  |

### enum `kind`

The station a recipe is made at.

| Case | |
| --- | --- |
| `crafting-shaped` |  |
| `crafting-shapeless` |  |
| `smelting` |  |
| `blasting` |  |
| `smoking` |  |
| `campfire` |  |
| `stonecutting` |  |
| `smithing` |  |
| `brewing` |  |

### record `recipe`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:oak_planks`. |
| `kind` | `kind` |  |
| `category` | `string` | The recipe book's tab (`crafting_building_blocks`, `furnace_food`), or "". |
| `group` | `string` | Recipes the book shows on one button share a group, or "". |
| `width` | `u32` | A shaped recipe's grid: `slots` holds its cells row by row. 0 for the other kinds. |
| `height` | `u32` |  |
| `slots` | `list<list<stack>>` | Each slot's stacks, any one of which fits; an empty list is an empty cell. Cooking and stonecutting: the input. Smithing: the template, the base, the addition. Brewing: the bottle, the reagent. |
| `results` | `list<stack>` | What it makes; several when that varies (a trim on any armour). |
| `cooking-time` | `u32` | Cooking: game ticks and experience; 0 for the other kinds. |
| `experience` | `f32` |  |
| `book-id` | `s32` | The recipe book's id for it (one per way the book shows it), as `known` lists them; -1 for the kinds the book doesn't show (campfire, stonecutting, smithing, brewing). |

### record `item`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | `minecraft:diamond`, `<mod>:<path>`. |
| `name` | `string` | In the player's language. |
| `tags` | `list<string>` | Its item tags, without the `#`. |

| Function | Signature | What it does |
| --- | --- | --- |
| `items` | `func() -> list<item>` | Every item, in the registry's order: `stack.item` indexes this list. |
| `all` | `func() -> list<recipe>` | Every recipe: the book's in its order, then campfire, stonecutting, smithing and brewing. They don't change while a world is open. |
| `known` | `func() -> list<s32>` | The `book-id`s of the recipes the player's recipe book holds now. |
| `place` | `func(book-id: s32, all: bool) -> result<_, string>` | Places a recipe in the open screen's grid as a click in the recipe book does (`ServerboundPlaceRecipePacket`): the inventory's grid or a crafting table, furnace, blast furnace or smoker. `all` places as many as the items allow, as with Shift. Refused while no such screen is open, for spectators, for a recipe the player's book doesn't hold and for one that can't be placed; one missing items shows as a ghost in the grid, as in the book. |
| `give` | `func(stack: stack) -> result<_, string>` | Creative mode only: gives the player the stack, as taking it from the creative inventory does (`ServerboundSetCreativeModeSlot`): the selected slot first, then the hotbar, then the inventory. The count is cut to one stack of the item (64, 16 or 1). |

## `browser`

Access: capability `ui`.

Capability `ui` (experimental): a recipe viewer's panel beside the game's inventory and container screens (not the creative inventory), on the right of the screen. The game draws it with its own widgets and slots, pages it and keeps its search box; the mod says what it holds. The first loaded mod with a panel shows.

### record `stack`

An item to show: its id, a count (1 draws none) and its components beyond the item's own, in vanilla's item syntax, or "".

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |
| `components` | `string` |  |

### record `click`

A click on an item of the panel, by its index in `set-items`'s list.

| Field | Type | |
| --- | --- | --- |
| `index` | `u32` |  |
| `right` | `bool` |  |

### record `item-key`

A key the mod registered (`input.register-key`) went down while the pointer was on an item: a slot of the screen's, or the panel's.

| Field | Type | |
| --- | --- | --- |
| `key` | `string` |  |
| `stack` | `stack` |  |

### record `slot-click`

A click on a slot of a recipe the panel shows (`set-recipes`).

| Field | Type | |
| --- | --- | --- |
| `recipe` | `u32` | The recipe's index in `set-recipes`'s list and the slot's in it; the results come after the slots. |
| `slot` | `u32` |  |
| `stack` | `stack` | The stack the slot showed at that moment. |
| `right` | `bool` |  |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `clicked` | `click` |  |
| `search` | `string` | The search box's text, each time the player changes it. |
| `key` | `item-key` |  |
| `slot-clicked` | `slot-click` |  |
| `action` | `u32` | The action button of the recipe at this index in `set-recipes`'s list. |
| `page` | `u32` | The player turned the recipes' page (`set-recipes`): the index of the first recipe now shown. |
| `back` |  | The back button of `set-recipes`. |
| `tab` | `u32` | The player picked the tab at this index of `set-recipes`' tabs. |

### enum `layout`

How the game lays out a recipe of `set-recipes`.

| Case | |
| --- | --- |
| `grid` | A crafting grid of `width` x `height` slots, an arrow and the result. |
| `cooking` | One input over a flame, an arrow and the result (furnaces, smokers, campfires). |
| `single` | One input, an arrow and the result (the stonecutter). |
| `smithing` | Template, base and addition, an arrow and the result. |
| `brewing` | A potion and an ingredient, an arrow and the result. |

### record `recipe-view`

A recipe to show. Each slot, and the result, cycles through its stacks, one a second; an empty slot shows nothing.

| Field | Type | |
| --- | --- | --- |
| `layout` | `layout` |  |
| `width` | `u32` | The grid's size (`grid` only), at most 3 x 3. |
| `height` | `u32` |  |
| `slots` | `list<list<stack>>` |  |
| `results` | `list<stack>` |  |
| `note` | `string` | A short line under the recipe (a cooking time), or "". |
| `action` | `option<bool>` | The action button beside it: none, enabled, or shown greyed. |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-items` | `func(items: list<stack>)` | The panel's items, at most 8192 (an empty list hides the panel). |
| `set-recipes` | `func(title: string, tabs: list<stack>, tab: u32, recipes: list<recipe-view>)` | Shows these recipes in the panel instead of its items, as many as fit a page, under `title` and a back button, with a row of `tabs` (each an icon; `tab` is the selected one) between them; an empty list of recipes shows the items again. At most 256 recipes and 16 tabs. |
| `poll` | `func() -> list<event>` | What the player did on the panel since the last poll, at most 256 kept. |

## `browser-ext`

Access: capability `recipes` for data queries, `ui` for panel functions.

API level 19: browser extensions. Data queries need capability `recipes`; all other functions need capability `ui` (experimental).

### type `recipe`

`type recipe = recipe`

### type `stack`

`type stack = stack`

### type `click`

`type click = click`

### type `recipe-view`

`type recipe-view = recipe-view`

### record `item-fact`

What the game knows of an item beyond `recipes.items`: from its components, the same for vanilla and content-mod items.

| Field | Type | |
| --- | --- | --- |
| `item` | `u32` | The item's index in `recipes.items`. |
| `burn-ticks` | `u32` | `minecraft:cooking_fuel`: ticks it burns in a furnace, 0 when it is no fuel. |
| `compost-chance` | `f32` | `minecraft:compostable`: the chance one raises a composter's level (0 to 1), 0 when not. |
| `repaired-by` | `list<u32>` | `minecraft:repairable`: the items (indexes in `recipes.items`) that repair it in an anvil. |

### record `other-recipe`

A recipe whose type the game doesn't know (a mod's own, `iceandfire:dragonforge`), as its file.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` |  |
| `kind` | `string` |  |
| `json` | `string` |  |

### record `placed-slot`

A slot placed in a custom recipe's box, in GUI px from the box's top left; it cycles through its stacks like the standard layouts' slots.

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `stacks` | `list<stack>` |  |

### record `placed-text`

Text in the box (`color` 0xRRGGBB), drawn with the game's font and shadow.

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `text` | `string` |  |
| `color` | `u32` |  |

### record `placed-sprite`

One of the game's GUI sprites (`gui/sprites/...` names, e.g. `container/furnace/lit_progress`, `recipe_book/...`); an unknown name draws nothing.

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `sprite` | `string` |  |

### record `custom-view`

A recipe the mod lays out itself. At most 9 slots, 16 texts, 16 sprites; the box at most 160 x 120. The panel is 2 to 12 slots (36 to 216 GUI px) wide, about 108 at GUI scale 3 on a 1280 x 720 window: a wider box runs off the screen's edge, so keep boxes narrow.

| Field | Type | |
| --- | --- | --- |
| `width` | `u32` |  |
| `height` | `u32` |  |
| `slots` | `list<placed-slot>` |  |
| `result` | `option<placed-slot>` |  |
| `texts` | `list<placed-text>` |  |
| `sprites` | `list<placed-sprite>` |  |
| `action` | `option<bool>` | The action button beside it (as `browser.recipe-view.action`). |

### variant `recipe-entry`

| Case | Payload | |
| --- | --- | --- |
| `standard` | `recipe-view` |  |
| `custom` | `custom-view` |  |

### variant `event`

| Case | Payload | |
| --- | --- | --- |
| `bookmark-clicked` | `click` | A click on the bookmark panel's item at this index of `set-bookmarks`. |

### record `item-lines`

Extra tooltip lines for some of a panel's items.

| Field | Type | |
| --- | --- | --- |
| `index` | `u32` | The item's index in `browser.set-items` (or `set-bookmarks` for the bookmark panel). |
| `lines` | `list<string>` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `item-facts` | `func() -> list<item-fact>` | Capability `recipes`. One entry per item with any fact; others are left out. |
| `other-recipes` | `func() -> list<other-recipe>` | Capability `recipes`. The world's recipes of types `recipes.all` can't express, from its data and content mods. Empty on servers that are not Horizon (a client is sent no recipe files). At most 4096, each file at most 16 KiB. |
| `recipe-count` | `func() -> u32` | Capability `recipes`. How many recipes `recipes.all` holds. |
| `recipes-page` | `func(start: u32, count: u32) -> list<recipe>` | Capability `recipes`. `count` recipes of `recipes.all` from index `start`, in the same order (fewer at the end, none past it). At most 2048 a call. For big packs, where one `recipes.all` call costs more than a frame's fuel. |
| `set-recipes` | `func(title: string, tabs: list<stack>, tab: u32, recipes: list<recipe-entry>)` | `browser.set-recipes` with custom layouts mixed in. Same limits, same events (`browser.event.slot-clicked` counts a custom recipe's slots in order, then its result). |
| `set-bookmarks` | `func(items: list<stack>)` | A second panel left of the screen's window, as JEI's bookmarks (an empty list hides it). At most 1024. Hidden while the screen's recipe book is open. |
| `set-item-lines` | `func(bookmarks: bool, lines: list<item-lines>)` | Extra tooltip lines under these items' names, for the main panel (`bookmarks` false) or the bookmark panel. Replaces earlier ones for that panel. At most 8 lines of 128 characters per item, 8192 items. Dropped when that panel's items change (`set-items` / `set-bookmarks`). |
| `keep-empty` | `func(keep: bool)` | With true, an empty `browser.set-items` keeps the panel and its search box up. |
| `poll` | `func() -> list<event>` | What the player did on the bookmark panel since the last poll, at most 256 kept. |

## `players`

Access: capability `players`.

Capability `players`, server mods only (manifest `api = "0.3"`, experimental): the players on the server, and messages to them.

### record `player-damage`

| Field | Type | |
| --- | --- | --- |
| `player` | `string` |  |
| `amount` | `f32` | Health plus absorption actually lost, after armor/effects. |
| `source-kind` | `string` | Registered damage type id. |
| `attacker-mob` | `option<u32>` |  |

### record `point`

A vector or position in blocks, with finite components.

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |

### record `equipment-slots`

Registry ids, none for an empty slot. Distinct name from equipment (WIT).

| Field | Type | |
| --- | --- | --- |
| `head` | `option<string>` |  |
| `chest` | `option<string>` |  |
| `legs` | `option<string>` |  |
| `feet` | `option<string>` |  |
| `main-hand` | `option<string>` |  |
| `off-hand` | `option<string>` |  |
| `hotbar` | `list<option<string>>` | Exactly nine hotbar slots, in order. |

### record `teleport-target`

Yaw and pitch in degrees as F3 shows them; none keeps the player's current view.

| Field | Type | |
| --- | --- | --- |
| `dimension` | `string` |  |
| `at` | `point` |  |
| `yaw` | `option<f32>` |  |
| `pitch` | `option<f32>` |  |

### enum `game-mode`

| Case | |
| --- | --- |
| `survival` |  |
| `creative` |  |
| `adventure` |  |
| `spectator` |  |

### record `player`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | The player's UUID, `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`. |
| `name` | `string` |  |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `dimension` | `string` | `minecraft:overworld`, `minecraft:the_nether`, ... |
| `game-mode` | `game-mode` |  |
| `health` | `f32` | Hit points as vanilla counts them: 20 is full health without effects. |

### record `look`

Where a player looks.

| Field | Type | |
| --- | --- | --- |
| `yaw` | `f32` | Degrees, as F3 shows them (0 south, 90 west). |
| `pitch` | `f32` | Degrees: -90 looks straight up, 90 straight down. |

### record `block-hit`

The block a player clicked, and its face (down, up, north, south, west, east).

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |
| `face` | `string` |  |

### record `item-use`

| Field | Type | |
| --- | --- | --- |
| `player` | `string` |  |
| `item` | `string` |  |
| `hand` | `string` | main_hand or off_hand. |
| `sneaking` | `bool` |  |
| `target` | `option<block-hit>` |  |

### record `attack`

| Field | Type | |
| --- | --- | --- |
| `player` | `string` |  |
| `item` | `option<string>` |  |
| `mob` | `option<u32>` | The mob id used by entities.mobs and entities.near. |
| `target-player` | `option<string>` |  |
| `damage` | `f32` |  |

### record `player-death`

API 31: a player's death as the server saw it, polled like damage-taken.

| Field | Type | |
| --- | --- | --- |
| `player` | `string` | The player's UUID. |
| `dimension` | `string` | Dimension id where they died. |
| `at` | `point` | Feet position at death. |
| `cause` | `option<string>` | The death message's translation key (`death.attack.lava`); none when built from combat. |

### record `player-respawn`

API 31: a player placed by a respawn.

| Field | Type | |
| --- | --- | --- |
| `player` | `string` |  |
| `dimension` | `string` |  |
| `at` | `point` |  |

### record `player-vitals`

API 31: food and experience; health is in `player`.

| Field | Type | |
| --- | --- | --- |
| `food` | `u32` | 0..20. |
| `saturation` | `f32` | 0..food. |
| `xp-level` | `u32` |  |
| `xp-progress` | `f32` | Progress toward the next level, 0..1. |
| `xp-total` | `u32` |  |

### record `active-effect`

API 31: one active status effect.

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | Namespaced id, `minecraft:speed`. |
| `ticks` | `s32` | Remaining ticks; -1 is infinite. |
| `amplifier` | `u8` |  |

### variant `item-filter`

API 31: which stacks clear-items removes.

| Case | Payload | |
| --- | --- | --- |
| `item` | `string` | An exact item id. |
| `tag` | `string` | An item tag, without `#`. |
| `mod-data` | `option<string>` | Stacks with the calling mod's custom data: some(v) only value v, none any value. |

| Function | Signature | What it does |
| --- | --- | --- |
| `damage-taken` | `func() -> list<player-damage>` | Last tick's accepted damage, at most 256. Reading does not consume it. |
| `heal` | `func(player: string, amount: f32) -> result<_, string>` | Restore health (0..1024), capped by max health; never revive a dead player. |
| `equipment` | `func(player: string) -> option<equipment-slots>` | The online player's authoritative equipment, including changes by inventory calls. |
| `damage-item` | `func(player: string, hand: string, amount: u32) -> result<_, string>` | ItemStack.hurtAndBreak on main_hand or off_hand, including Unbreaking, creative immunity, inventory sync and the item's break sound/particles. |
| `knockback` | `func(player: string, strength: f32, dx: f64, dz: f64) -> result<_, string>` | LivingEntity knockback; dx/dz point away from the attacker. Strength 0..4. The game applies the player's knockback resistance. |
| `push` | `func(player: string, velocity: point) -> result<_, string>` | Add velocity in blocks per tick, each axis -4..4; clients receive the motion. |
| `teleport` | `func(player: string, target: teleport-target) -> result<_, string>` | Moves an online player like vanilla /tp, including across dimensions, resetting fall distance and notifying clients. Errors for an offline player, unknown dimension, non-finite coordinates or angles, or outside the world border. |
| `online` | `func() -> list<player>` | The players online, in the order they joined. |
| `permission-level` | `func(player: string) -> option<u8>` | Vanilla's op permission level, 0..4; none when offline. Single-player with cheats on counts as 4. |
| `facing` | `func(player: string) -> option<look>` | Where an online player looks, by UUID; none when no such player is online. |
| `item-uses` | `func() -> list<item-use>` | Item right clicks during the last tick, oldest first, at most 256. The game's own use of each item still happens. Reading does not consume them. |
| `attacks` | `func() -> list<attack>` | Melee hits players landed during the last tick, oldest first, at most 256. |
| `add-effect` | `func(player: string, id: string, seconds: u32, amplifier: u8) -> result<_, string>` | Apply a registered status effect; zero seconds removes it. Unknown effects or offline players are errors. Players' clients receive the change. |
| `tell` | `func(player: string, text: string) -> bool` | A system message to one player, after the mod's name; false when no player with that id is online. Up to 8 messages per call. |
| `broadcast` | `func(text: string)` | A system message to every player and the server's log, after the mod's name. Counts toward the same 8 per call. |
| `take-item` | `func(player: string, item: string, count: u32) -> result<u32, string>` | Remove up to count by registry id, main hand first, then the rest of the inventory (including armor and off hand). Creative consumes none but reports count. The return value and subsequent calls see the real inventory now; unknown items or offline players are errors. |
| `give-item` | `func(player: string, item: string, count: u32) -> result<_, string>` | Add plain items to a player's inventory; drop overflow at their feet before that dimension's next tick. At most 100 stacks per call, as /give. Unknown items or offline players are errors. |
| `deaths` | `func() -> list<player-death>` | API 31: players who died during the last tick, oldest first, at most 256. |
| `respawns` | `func() -> list<player-respawn>` | API 31: players placed by a respawn during the last tick, at most 256. |
| `vitals` | `func(player: string) -> option<player-vitals>` | API 31: an online player's food and experience as at the start of the tick; none when offline. |
| `effects` | `func(player: string) -> option<list<active-effect>>` | API 31: an online player's active effects (at most 64) at the start of the tick; none when offline. |
| `set-health` | `func(player: string, health: f32) -> result<_, string>` | API 31: health 0 < h <= 1024, capped at max health; never kills or revives. Deferred. |
| `hurt` | `func(player: string, amount: f32, damage-type: string) -> result<_, string>` | API 31: hurt as vanilla damage of a registered type, 0 < amount <= 1024; armour applies, can kill; creative immune. Deferred. |
| `set-food` | `func(player: string, food: u32, saturation: f32) -> result<_, string>` | API 31: food 0..20 and saturation 0..food. Deferred. |
| `give-experience` | `func(player: string, points: u32) -> result<_, string>` | API 31: Player.giveExperiencePoints, 0..100000 points (no Mending). Deferred. |
| `give-levels` | `func(player: string, levels: s32) -> result<_, string>` | API 31: Player.giveExperienceLevels, -1000..1000; never below level 0. Deferred. |
| `clear-effects` | `func(player: string) -> result<_, string>` | API 31: remove the effects active at the start of the tick. Deferred. |
| `clear-items` | `func(player: string, filter: item-filter, max: u32) -> result<u32, string>` | API 31: like /clear, in main inventory, armour and off hand (main hand first); the items are destroyed, also in creative. max 0 only counts. Returns the count removed (or matching for 0). |
| `play-sound-to` | `func(player: string, name: string, volume: f32, pitch: f32) -> result<_, string>` | API 31: a sound only this player hears, at their position; volume 0..4, pitch 0.5..2. |

## `world`

Access: capability `world`.

Capability `world`, server mods only (manifest `api = "0.3"`, experimental): the blocks of the server's loaded chunks, and changes to them through the server's own block updates.

### record `dust-color`

API 31: particle options: rgb 0xRRGGBB and scale 0.01..4.

| Field | Type | |
| --- | --- | --- |
| `rgb` | `u32` |  |
| `scale` | `f32` |  |

### enum `blast-kind`

API 31: what an explosion breaks.

| Case | |
| --- | --- |
| `keep` |  |
| `block` |  |
| `tnt` |  |

### record `stack`

API 30: a collected real stack, including its full saved component patch. Components are lossless network NBT, read-only; no guest-selected NBT is applied.

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |
| `components` | `option<list<u8>>` |  |

### record `stack-info`

API level 28: one slot, including the identity of all saved components.

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |
| `display-key` | `string` | Stable SHA-256 of item id and saved components, excluding count. A display/grouping key, never an item handle or proof of equality. |
| `custom-name` | `option<string>` | Plain text of the custom name, when present. |

### record `stack-info-with-data`

| Field | Type | |
| --- | --- | --- |
| `stack` | `stack-info` |  |
| `data` | `option<string>` |  |

### record `block-change-pos`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |

### record `point`

A vector or position in blocks, with finite components.

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |

### record `particle-burst`

API 31: a vanilla particle burst. `block` is required for minecraft:block, block_marker, falling_dust, dust_pillar and block_crumble; `dust` for minecraft:dust; others take neither.

| Field | Type | |
| --- | --- | --- |
| `name` | `string` |  |
| `at` | `point` |  |
| `delta` | `point` |  |
| `speed` | `f32` |  |
| `count` | `u32` | 0..256; zero sends one moving delta * speed. |
| `block` | `option<string>` |  |
| `dust` | `option<dust-color>` |  |

### record `break-options`

| Field | Type | |
| --- | --- | --- |
| `keep-tool` | `bool` | Stop before a break that could use the tool's last durability point. |
| `drops-at` | `option<point>` | Landing position for all drops and experience; none means each block. |
| `exhaustion` | `f32` | Food exhaustion per block, 0..1. |

### record `block-change`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |
| `state` | `string` | `minecraft:oak_log[axis=y]`; properties left out take the block's defaults, `minecraft:stone` alone is the default state. |

### record `world-time`

| Field | Type | |
| --- | --- | --- |
| `day-time` | `u64` | The server's shared Overworld dayTime (not reduced modulo 24000). |
| `game-time` | `u64` |  |
| `is-day` | `bool` | Level.isBrightOutside: not a fixed-time dimension, sky darkening < 4. |
| `raining` | `bool` |  |
| `thundering` | `bool` |  |

### record `chunk-pos`

A chunk column of a dimension, by chunk coordinates (block x >> 4).

| Field | Type | |
| --- | --- | --- |
| `dimension` | `string` |  |
| `x` | `s32` |  |
| `z` | `s32` |  |

### record `item-stack`

A plain item stack; components are not carried by this API.

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `spawn-particles` | `func(dimension: string, particles: particle-burst) -> result<_, string>` | API 31: 64 calls and 4096 particles/mod/tick, sent to players near `at`. Deferred. |
| `explode` | `func(dimension: string, center: point, radius: f32, fire: bool, blast: blast-kind) -> result<_, string>` | API 31: Level.explode with no source, 0 < radius <= 8, 4/mod/tick; mob_griefing off keeps blocks and fire. Deferred. |
| `make-block-fall` | `func(dimension: string, pos: block-change-pos, expected-state: string, velocity: point, hurts: bool) -> result<_, string>` | API 31: turn an existing block into a falling block. expected-state must equal get-block now and when applied; velocity each axis \|v\| <= 3.9; 16/mod/tick. Deferred. |
| `fluid-flow` | `func(dimension: string, pos: block-change-pos) -> option<point>` | API 31: FluidState.getFlow, the normalised current; none for unloaded chunks or unknown dimensions. |
| `block-data` | `func(dimension: string, pos: block-change-pos) -> result<option<string>, string>` | API 30: only the calling mod's saved UTF-8 value, at most 4096 bytes. |
| `set-block-data` | `func(dimension: string, pos: block-change-pos, data: option<string>) -> result<_, string>` | Deferred, preimage-checked; success is a queue receipt. |
| `destroy-block` | `func(dimension: string, pos: block-change-pos, tool: option<string>, collect: bool) -> result<list<stack>, string>` | Engine loot with a plain tool, 64 requests/mod/tick. Collect returns drops. At most 256 stacks, 64 KiB components/stack, 1 MiB total. On overflow, drops are spawned intact and an error is returned after the break. |
| `set-break-progress` | `func(dimension: string, pos: block-change-pos, stage: s32) -> result<_, string>` | API 30: -1 clears, 0..9 cracks; 64 requests/mod/tick, within 64 blocks. |
| `schedule-tick` | `func(dimension: string, pos: block-change-pos, delay-ticks: u32) -> result<_, string>` | API 29: loaded watched block only, delay 1..72000, 256 pending/mod. Ephemeral: discarded on state change, chunk unload, mod stop or restart. |
| `player-stack-data` | `func(player: string, inv-slot: u32) -> result<option<string>, string>` | API 29: only the calling manifest id's key in minecraft:custom_data. UTF-8, at most 1024 bytes; JSON/base64 may be encoded by the mod. Reads use the storage reservation snapshot. Empty slots return none. |
| `container-stack-data` | `func(dimension: string, pos: block-change-pos, slot: u32) -> result<option<string>, string>` | Read only this mod's value in one loaded container slot; invalid slots/containers error. |
| `player-held-data` | `func(player: string) -> result<option<string>, string>` | Main hand only; also usable inside item callbacks. No callback ABI change. |
| `set-player-stack-data` | `func(player: string, inv-slot: u32, data: option<string>) -> result<_, string>` | Deferred until hooks return; changed/empty slots cancel on apply. None removes only this mod's key, removing an empty component. Bulk containers only. 64 requests/mod/tick shared with storage moves and the two with-data creation functions. Success means queued. |
| `set-container-stack-data` | `func(dimension: string, pos: block-change-pos, slot: u32, data: option<string>) -> result<_, string>` | Queue this mod's value for a bulk container slot; None removes it. Shared 64 requests/mod/tick. |
| `set-player-held-data` | `func(player: string, data: option<string>) -> result<_, string>` | Queue a main-hand data write; selection or stack changes cancel on apply. Shared 64 requests/mod/tick. |
| `give-item-with-data` | `func(player: string, item: string, count: u32, data: option<string>) -> result<_, string>` | Same 100 stacks/call and overflow semantics as players.give-item; component equality prevents merging with plain/differently tagged stacks. |
| `spawn-item-with-data` | `func(dimension: string, at: point, item: string, count: u32, data: option<string>) -> result<_, string>` | Same limits as spawn-item, including the shared 16 spawns/mod/tick. |
| `player-stacks-with-data` | `func(player: string) -> result<list<option<stack-info-with-data>>, string>` | Read the 36 main inventory slots with only this mod's data; offline players error. |
| `container-stacks-with-data` | `func(dimension: string, pos: block-change-pos) -> result<list<option<stack-info-with-data>>, string>` | Read one loaded block's slots with only this mod's data; missing containers error. |
| `container-stacks` | `func(dimension: string, pos: block-change-pos) -> option<list<option<stack-info>>>` | API level 28: one loaded block's slots (one half of a double chest). |
| `player-stacks` | `func(player: string) -> option<list<option<stack-info>>>` | API level 28: read an online player's 36 main inventory slots, hotbar first (0..35, as player-to-container's inv-slot); no armor/offhand. UUID or exact online name; none for unknown/offline players. Same stack metadata as container-stacks. Server-thread reservation view at call time: includes earlier moves in this hook batch; read-only. |
| `container-move` | `func(dimension: string, source: block-change-pos, source-slot: u32, destination: block-change-pos, destination-slot: option<u32>, count: u32) -> result<u32, string>` | API level 28: reserve a component-preserving transfer after hooks return. Moves min(count, source count), all or nothing if capacity is insufficient. None selects matching stacks then empty slots. Same-slot moves return zero. Revalidated on apply; a changed endpoint cancels the entire transfer. Bulk storage containers only; special machine slots are read-only. The result is the reserved count; reread next tick to observe completion. |
| `container-to-player` | `func(dimension: string, pos: block-change-pos, slot: u32, count: u32, player: string) -> result<u32, string>` | API level 28: as container-move, into the online player's main inventory. No creative exemption, overflow drops, armor or offhand slots. |
| `player-to-container` | `func(player: string, inv-slot: u32, dimension: string, pos: block-change-pos, count: u32) -> result<u32, string>` | API level 28: main inventory slot 0..35 (hotbar first) into a container. |
| `break-blocks-as` | `func(player: string, blocks: list<block-change-pos>, options: break-options) -> result<_, string>` | API level 20: queue real player breaks, applied after hooks with the live game. Unknown/offline players or over 2048 positions/call or 4096/mod/tick error atomically. At apply time skip air, liquids, unloaded blocks, positions beyond 80 blocks, and unbreakable blocks in survival. Stop if the player leaves or keep-tool triggers. Uses the normal player path (protection, border, loot, XP, wear, stats and updates). Creative gives no drops/wear; the caller's block-broken hook is excluded. |
| `spawn-point` | `func() -> tuple<string, point, f32>` | The overworld's world spawn: dimension id, position, angle in degrees. |
| `spawn-item` | `func(dimension: string, at: point, item: string, count: u32) -> result<_, string>` | Drop a plain stack in this dimension, count 1..max stack size. At most 16 successful requests per tick per mod, across all hook calls. |
| `block-has-tag` | `func(block: string, tag: string) -> bool` | Whether a block id or state belongs to a loaded block tag (without #). Unknown blocks and tags return false. |
| `play-sound` | `func(dimension: string, at: point, name: string, volume: f32, pitch: f32) -> result<_, string>` | A registered vanilla or loaded content sound in this dimension. |
| `generated-chunks` | `func() -> list<chunk-pos>` | The chunks the server generated for the first time during the last tick (never ones loaded from the save), at most 256, oldest first: where a mod places its own structures, once per chunk. They are loaded now, so `get-block` reads them and `set-blocks` changes them. |
| `time` | `func(dimension: string) -> world-time` | Time and weather in a dimension. Unknown dimensions return zero times and false flags; known dimensions need not have loaded chunks. |
| `can-see-sky` | `func(dimension: string, x: s32, y: s32, z: s32) -> bool` | Level.canSeeSky: sky light 15, independent of time, sky darkening or block light. False for unknown/unloaded chunks and dimensions without skylight. |
| `get-block` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<string>` | The block state at a position in a dimension (`minecraft:overworld`, ...), as `minecraft:oak_log[axis=y]`, as of this call: none where the server has no chunk loaded there. |
| `heightmap` | `func(dimension: string, x: s32, z: s32) -> option<list<s32>>` | The loaded chunk's OCEAN_FLOOR heights, by chunk coordinates (block x >> 4, block z >> 4), as of this call: exactly 256 values, indexed z * 16 + x for local x, z in 0..16. Each is the first Y above the highest opaque block (fluids do not count, leaves do), or the column's minimum Y when empty. None for an unloaded chunk or unknown dimension. |
| `get-biome` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<string>` | The biome at a position (`minecraft:plains`, ...), as the server stores it (one per 4 x 4 x 4 cell): none where no chunk is loaded. |
| `set-blocks` | `func(dimension: string, blocks: list<block-change>) -> result<_, string>` | Sets blocks as `/setblock` does: the old block entity removed, neighbours and shapes updated, light, and every player told. Every state is checked now (an error queues nothing); the changes are made in order after this call returns, before that dimension's next tick; one in a chunk not loaded then is skipped. At most 4096 per call. |
| `set-loot-table` | `func(dimension: string, x: s32, y: s32, z: s32, table: string) -> result<_, string>` | Gives the container at a position (a chest, trapped chest, barrel, ...) a loot table, as a structure's chest has: it is rolled as generated chests are, after this tick's `set-blocks` (so a chest placed by them gets it). `table` is a loot table id (`minecraft:chests/simple_dungeon`, a mod's own). Not a container then: nothing. |
| `container` | `func(dimension: string, x: s32, y: s32, z: s32) -> option<list<option<item-stack>>>` | Slots of the block's container; none when it has no container or is unloaded. This reads one block's slots, including one half of a double chest. |
| `set-container` | `func(dimension: string, x: s32, y: s32, z: s32, slots: list<option<item-stack>>) -> result<_, string>` | Replace plain stacks after this tick's block writes, updating open viewers. The slot count must match; item ids and stack counts are validated. If the container is absent or its slot count changed when applied, skip it. |
| `place-structure` | `func(dimension: string, id: string, x: s32, y: s32, z: s32, rotation: u32, mirror: bool) -> result<_, string>` | Load a vanilla or mod structure template, as a structure block's LOAD does, after this tick's block writes, preserving block entities and loot tables. Rotation is 0..3 clockwise quarter turns; mirror reflects before rotating. |
| `set-spawner` | `func(dimension: string, x: s32, y: s32, z: s32, entity: string) -> result<_, string>` | Sets the mob a monster spawner (`minecraft:spawner`) spawns, as a spawn egg used on it does, after this tick's block writes (so a spawner placed by them gets it). `entity` is an entity type id, vanilla's or a content mod's. The spawner keeps its delays, counts and ranges. Not a spawner then, or an unknown type: nothing, logged. |
| `game-rule` | `func(name: string) -> option<string>` | Current server rule value as text, or none for an unknown rule. Uses 26.3 names (mob_griefing, minecraft:mob_griefing); mobGriefing is an alias. |
| `difficulty` | `func() -> string` | peaceful, easy, normal or hard, as of the start of this call. |

## `entities`

Access: capability `entities`.

Capability `entities`, server mods only (manifest `api = "0.3"`, experimental): the mobs on the server, and orders to the content mods' own (docs/modpacks.md, entities): where to walk, where to look, how to fly, which of their animations to play, and whom their breath hurts.

### record `hurt-event`

Accepted damage to content mobs during the last tick.

| Field | Type | |
| --- | --- | --- |
| `victim` | `u32` |  |
| `kind` | `string` |  |
| `attacker-mob` | `option<u32>` |  |
| `attacker-player` | `option<string>` |  |
| `amount` | `f32` |  |

### record `death`

Content mob's position when it died, before corpse movement/removal.

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` |  |
| `kind` | `string` |  |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |

### record `rider`

A player riding a mob, and the keys and look they steer with.

| Field | Type | |
| --- | --- | --- |
| `player` | `string` | The player's UUID, as `players.online` has it. |
| `forward` | `f32` | 1 forward, -1 back, 0 neither (the movement keys or stick). |
| `strafe` | `f32` | 1 left, -1 right, 0 neither. |
| `jumping` | `bool` | The jump key is held. |
| `sprinting` | `bool` | The sprint key is held, or the player sprints. |
| `yaw` | `f32` | Where the player looks, vanilla's yaw and pitch in degrees. |
| `pitch` | `f32` |  |

### record `entity`

| Field | Type | |
| --- | --- | --- |
| `id` | `u32` | The mob's id on this server while it is loaded. |
| `kind` | `string` | `minecraft:zombie`, `ruby:golem`, ... |
| `dimension` | `string` | `minecraft:overworld`, ... |
| `x` | `f64` | Feet position, in blocks. |
| `y` | `f64` |  |
| `z` | `f64` |  |
| `yaw` | `f32` | Where its body faces, vanilla's yaw in degrees (0 south, 90 west). |
| `health` | `f32` |  |
| `max-health` | `f32` |  |
| `on-ground` | `bool` |  |
| `target` | `option<u32>` | The mob it attacks, by id; none for no mob (a player, or none). |
| `target-player` | `bool` | It attacks a player: the nearest it sees or one selected by UUID. |
| `scale` | `f32` | Its `minecraft:scale`: 1 is its type's size. |
| `data` | `list<tuple<string, string>>` | What mods stored on it (`set-data`, `spawn`), kept in its save. |
| `age` | `u32` | Ticks it has lived since it was loaded. |
| `animation` | `option<string>` | The animation an `animate` order plays on it now. |
| `texture` | `option<string>` | The per-mob texture override; none uses its type's texture. |
| `rider` | `option<rider>` | The player riding it, if one does (seated by `mount`), and that player's input this tick. |

### record `mount`

Seat a player on a mob.

| Field | Type | |
| --- | --- | --- |
| `player` | `string` | That online player's UUID: in the mob's dimension, within 16 blocks, alive and not riding anything. |
| `height` | `f32` | Seat height above the mob's feet, -16 to 64 blocks. |
| `forward` | `f32` | Seat distance ahead of the mob's middle along its yaw, -32 to 32. |

### record `point`

| Field | Type | |
| --- | --- | --- |
| `x` | `f64` |  |
| `y` | `f64` |  |
| `z` | `f64` |  |

### record `strike-hit`

A filtered hurt-area, in the source mob's dimension. WIT requires a distinct record name from the strike function.

| Field | Type | |
| --- | --- | --- |
| `center` | `point` |  |
| `radius` | `f64` |  |
| `amount` | `f32` |  |
| `fire` | `u32` |  |
| `skip-kinds` | `list<string>` |  |
| `knockback` | `f32` | Extra vanilla knockback, away from the source mob, strength 0..4. |
| `credit-player` | `option<string>` | Online player's UUID: player attack and kill credit; that player is spared. |

### record `walk`

| Field | Type | |
| --- | --- | --- |
| `to` | `point` |  |
| `speed` | `f64` | A multiple of its movement speed, 0 to 4. |

### record `area`

| Field | Type | |
| --- | --- | --- |
| `center` | `point` | The centre of a ball, in the mob's dimension. |
| `radius` | `f64` | Its radius in blocks, at most 16. |
| `amount` | `f32` | Hit points every living thing in it but the mob loses (vanilla's `mob_attack` damage from the mob), at most 1024. |
| `fire` | `u32` | Seconds those not immune to fire burn after, at most 30. |

### record `attribute-change`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | max_health, movement_speed, flying_speed, attack_damage, attack_knockback, armor, armor_toughness, knockback_resistance, follow_range or step_height (as in entity JSON). |
| `value` | `f64` | Base value, 0 to 1024; vanilla's attribute bounds limit its effective value. |

### record `effect-change`

| Field | Type | |
| --- | --- | --- |
| `id` | `string` | A registered effect id, such as minecraft:slowness. |
| `seconds` | `u32` | Duration in seconds; zero removes this effect. |
| `amplifier` | `u8` |  |

### record `sound`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | A vanilla sound event id or a loaded mod's sounds.json event. |
| `volume` | `f32` |  |
| `pitch` | `f32` |  |

### record `particles`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | A vanilla particle type with no options. |
| `at` | `point` | Position in the mob's dimension. |
| `delta` | `point` |  |
| `speed` | `f32` |  |
| `count` | `u32` | At most 256; zero sends one with delta * speed as its velocity. |

### record `interaction`

A player's right click on a content mod's mob.

| Field | Type | |
| --- | --- | --- |
| `player` | `string` | The player's UUID, as `players.online` has it. |
| `mob` | `u32` |  |
| `kind` | `string` |  |
| `item` | `option<string>` | What the player held in the hand they used, `minecraft:beef`, ... |
| `sneaking` | `bool` | The player's shift key at the time of the click. |
| `hand` | `string` | main_hand or off_hand: the hand that clicked. |

### variant `target`

Whom a mob attacks.

| Case | Payload | |
| --- | --- | --- |
| `none` |  |  |
| `mob` | `u32` | A mob, by id. |
| `nearest-player` |  | The nearest player it sees. |
| `player` | `string` | That online player's UUID, even when another player is nearer. Dropped if the player leaves the level or becomes untargetable. |

### variant `order`

| Case | Payload | |
| --- | --- | --- |
| `walk-to` | `walk` | Walk to a point by its path navigation, ahead of its own goals, until it arrives or gets another order to walk or `halt`. |
| `halt` |  | Stop walking and looking where orders said: its goals take over. |
| `look-at` | `point` | Turn its head toward a point until another order. |
| `push` | `point` | Set its motion in blocks per tick, each axis -4 to 4: flight, leaps and dives. |
| `gravity` | `bool` | Whether gravity pulls it (false: it stays where it is pushed). |
| `animate` | `option<string>` | Play one of the animations of its own model's animation file, from its start (a looping one loops); none: back to its states' (idle, walk, attack). Players' games see it too. |
| `texture` | `option<string>` | A loaded mod's <namespace>:entity/<path> texture; none restores the type's texture. Saved on the mob and sent to every player. |
| `sound` | `sound` | Play a sound at the mob, within 16 * max(1, volume) blocks. |
| `particles` | `particles` | Send particles to players within 32 blocks, in the mob's dimension. |
| `hurt-area` | `area` | Hurt and burn what is in an area: a breath, a stomp. |
| `discard` |  | Remove it, with no drops (`Entity.discard`). |
| `target` | `target` | Have it attack, as its own goals do: its template's attacks (melee, arrows, ...) on that target. |
| `scale` | `f32` | Its `minecraft:scale`, 0.0625 to 16: growth. Its size and model follow, on every player's game. |
| `attribute` | `attribute-change` | Set a base attribute, kept in vanilla's saved attributes list. Raising max_health does not heal; lowering it clamps health. |
| `heal` | `f32` | LivingEntity.heal: add a finite amount, clamped to 0..max health, only while health is positive (never revives a dead mob). |
| `effect` | `effect-change` | Apply a registered status effect; zero seconds removes it. |
| `mount` | `mount` | Seat that player on it, at that seat (moved with later `mount` orders for the same player). The player gets off as from any mount (sneaking) or on `dismount`; the mob does not move by itself for the rider: the mod moves it from `rider`'s input. |
| `dismount` |  | Let its rider off, beside it. |
| `set-data` | `list<tuple<string, option<string>>>` | Store strings on it, kept in its save: a key (1 to 64 characters) to a value (up to 1024), none removes the key. At most 32 keys. |

| Function | Signature | What it does |
| --- | --- | --- |
| `hurt-events` | `func() -> list<hurt-event>` | Polled last-tick events, at most 256; reading does not consume them. |
| `deaths` | `func() -> list<death>` | Content mobs that died last tick, at most 256; reading does not consume them. |
| `effect` | `func(mob: u32, change: effect-change) -> result<_, string>` | Status effects on any loaded mob, including vanilla ones. Zero seconds removes. |
| `push` | `func(mob: u32, velocity: point) -> result<_, string>` | Set any loaded mob's velocity, as the push order does; each axis -4..4. |
| `tame` | `func(mob: u32, owner: option<string>) -> result<_, string>` | Mark any loaded tameable mob (including vanilla horses) tame. Optional owner is a UUID, including an offline player's; none preserves its owner. |
| `strike` | `func(mob: u32, hit: strike-hit) -> result<_, string>` | Source must be a loaded content mob. Radius <=16, damage <=1024, fire <=30 seconds; skips source and the listed entity types. |
| `mobs` | `func() -> list<entity>` | The loaded mobs of content mods' kinds (any namespace but `minecraft`), as at the start of the tick, at most 1024. |
| `near` | `func(dimension: string, at: point, radius: f64) -> list<entity>` | Every loaded mob within `radius` (at most 64) blocks of a point in a dimension, nearest first, at most 256. Players are `players.online`'s. |
| `interactions` | `func() -> list<interaction>` | The right clicks players made on content mods' mobs during the last tick, in order, at most 256: feeding, taming, mounting are the mod's to decide. The game's own use of the click (a lead, a name tag) still happens. |
| `give-orders` | `func(orders: list<tuple<u32, order>>) -> result<_, string>` | Orders, by mob id, to content mods' mobs: checked now (an error queues none) and carried out in order before their dimension's next tick; an order to a mob gone by then is dropped. At most 1024 per call. |
| `spawn` | `func(kind: string, dimension: string, at: point, yaw: f32, data: list<tuple<string, string>>) -> result<_, string>` | Spawns a content mod's mob (any namespace but `minecraft`) at a point of a dimension, facing `yaw`, persistent, with `data` stored on it (as `set-data`), before that dimension's next tick: `mobs` lists it from the tick after, where `data` tells it apart. At most 64 spawns per hook call; an unknown kind is an error. |
| `ride` | `func(rider: u32, vehicle: option<u32>) -> result<_, string>` | Seats a content mod's mob on another loaded mob (`Entity.startRiding`, as `/ride` does), or with no vehicle lets it off (`stopRiding`). Checked now: both loaded in one dimension, within 16 blocks, not the same mob, and no loop of riders. Carried out with the tick's orders, unless by then the vehicle carries another mob or a player, or either is gone. The rider then sits on the vehicle, which walks and fights as before; it is saved riding (`Passengers`) and every player sees it. |
| `vehicle` | `func(id: u32) -> option<u32>` | The mob or boat a loaded mob rides, as at the start of the tick. |

## `menus`

Access: capability `world`.

What the server calls in a server mod (manifest `api = "0.3"`, experimental). Only the hooks `init` asks for are ever called. The player in each event is as the server saw them when it happened. API 30, under capability world: display copies, never real item transfers.

### record `menu-item`

| Field | Type | |
| --- | --- | --- |
| `item` | `string` |  |
| `count` | `u32` |  |
| `name` | `option<string>` |  |
| `lore` | `list<string>` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `open-menu` | `func(player: string, rows: u32, title: string, items: list<option<menu-item>>) -> result<_, string>` | 1..6 rows, at most rows*9 copies; replaces the player's previous menu. |
| `set-menu-items` | `func(player: string, items: list<option<menu-item>>) -> result<_, string>` | Replace the calling mod's open display menu copies; 64 writes/mod/tick. |
| `close-menu` | `func(player: string) -> result<_, string>` | Close only a menu owned by the calling mod. |
| `inventory-slot` | `func(rows: u32, slot: s32) -> option<u32>` | Map the player's portion of a chest menu to inventory 0..35. |

## `server-events`

Access: exported by the mod: the game calls it.

### type `player`

`type player = player`

### record `block-pos`

| Field | Type | |
| --- | --- | --- |
| `x` | `s32` |  |
| `y` | `s32` |  |
| `z` | `s32` |  |

### variant `argument-kind`

What an argument of a command takes. The server sends each command and its arguments to players' games, whose chat box completes and colours them as vanilla's; the server parses them before the mod's `command` hook gets their values.

| Case | Payload | |
| --- | --- | --- |
| `word` |  | One word: letters, digits and `_ - . +`. |
| `text` |  | The rest of the line, spaces included: only last. |
| `integer` | `tuple<option<s32>, option<s32>>` | A whole number, within the bounds given. |
| `number` |  | A number with or without decimals. |
| `boolean` |  | `true` or `false`. |
| `player` |  | One online player: a name, or `@s` or `@p` (whoever typed it, as vanilla picks for a player). |
| `block-pos` |  | A block position: three whole numbers, each one also `~` or `~n`, relative to whoever typed it. |

### record `argument`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | Shown in the chat box's hint (`<height>`) and the key of its value. |
| `kind` | `argument-kind` |  |
| `optional` | `bool` | Optional arguments come after every required one. |

### record `server-command`

| Field | Type | |
| --- | --- | --- |
| `name` | `string` | Without the slash. A name vanilla already uses is refused (vanilla's command runs). |
| `arguments` | `list<argument>` | In order; none: the command takes nothing after its name. Free text is one optional `text` argument. |

### variant `argument-value`

An argument's value, parsed as its kind says.

| Case | Payload | |
| --- | --- | --- |
| `word` | `string` |  |
| `text` | `string` |  |
| `integer` | `s32` |  |
| `number` | `f64` |  |
| `boolean` | `bool` |  |
| `player` | `player` |  |
| `block-pos` | `block-pos` |  |

### record `registration`

| Field | Type | |
| --- | --- | --- |
| `commands` | `list<server-command>` | Commands players may type. |
| `tick` | `bool` | Call `tick` every server tick (20 per second). |
| `joins` | `bool` | Call `player-joined` and `player-left`. |
| `chat` | `bool` | Call `chat` for each chat message a player sends. |
| `blocks` | `bool` | Call `block-broken` and `block-placed`. |

### enum `decision`

API 29: before a player changes a watched block. Cancel keeps block and item.

| Case | |
| --- | --- |
| `allow` |  |
| `cancel` |  |

### enum `hand`

| Case | |
| --- | --- |
| `main` |  |
| `off` |  |

### variant `chat-decision`

| Case | Payload | |
| --- | --- | --- |
| `allow` |  |  |
| `cancel` |  |  |
| `replace` | `string` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `menu-clicked` | `func(player: player, slot: s32, button: s32, mode: s32)` | API 30: optional exports, routed only to the menu's owner; 64 clicks/tick. |
| `menu-closed` | `func(player: player)` | The owned display menu closed, including replacement, death or leave. |
| `init` | `func() -> result<registration, string>` | Once, after loading. An error keeps the mod off and is logged. |
| `tick` | `func(tick: u64)` | Each server tick, with the server's tick counter. |
| `player-joined` | `func(player: player)` | A player joined the server; they are in `players.online` already. |
| `player-left` | `func(player: player)` | A player left the server; they are no longer in `players.online`. |
| `chat` | `func(player: player, text: string)` | A chat message a player sent, as plain text. |
| `command` | `func(player: player, name: string, args: string, values: list<tuple<string, argument-value>>) -> result<option<string>, string>` | `/name args` typed by `player`: ok(some(text)) is their feedback, err(text) their error in red. `values` holds each argument given, by name, in order (an optional one left out isn't there); a line that doesn't parse never reaches the mod: the player gets vanilla's error. |
| `block-broken` | `func(player: player, pos: block-pos, state: string)` | A player broke the block at `pos` in their dimension: `state` is what was there, as `minecraft:oak_log[axis=y]`. |
| `block-used` | `func(player: player, pos: block-pos, state: string, sneaking: bool) -> bool` | API level 28: main-hand block use before vanilla/placement. True consumes it. Opt in through server-hooks.set-block-uses; at most 32 calls/mod/tick. |
| `channel-message` | `func(player: player, name: string, data: list<u8>)` | API level 28: channel capability, only this mod id's messages (32/tick). |
| `item-picked-up` | `func(player: player, item: string) -> decision` | API 29: general hooks use flags without watched ids. |
| `chat-sent-filter` | `func(player: player, text: string) -> chat-decision` | Replacements contain at most 256 chat characters; invalid replacements allow. |
| `item-used-on-block` | `func(player: player, item: string, pos: block-pos, state: string, face: s32) -> decision` | Before watched ItemStack.useOn, after the block lets the use pass. Cancel leaves the hand unchanged. |
| `item-used` | `func(player: player, item: string) -> decision` | Before watched Item.use (air or fallback). Cancel suppresses use and consumption. |
| `item-hit-entity` | `func(player: player, item: string, entity-kind: string, entity-id: s32) -> decision` | Before an attack with a watched held item; kind and network id identify the target. |
| `item-use-finished` | `func(player: player, item: string)` | A watched player item completed its timed use; observation only. |
| `block-neighbour-changed` | `func(dimension: string, pos: block-pos, state: string, from: string)` | API 29: observations of the engine's neighbour/random hooks. `from` is the changed block's resource id (the native hook's BlockId). |
| `block-random-tick` | `func(dimension: string, pos: block-pos, state: string)` | Observe an engine random tick, including content blocks with random_ticks. |
| `block-scheduled-tick` | `func(dimension: string, pos: block-pos, state: string)` | A mod-requested tick became due for an unchanged loaded watched block. |
| `block-interacted` | `func(player: player, pos: block-pos, state: string, face: s32, hit-x: f64, hit-y: f64, hit-z: f64, hand: hand, item: string, sneaking: bool) -> decision` | API 29: relative hit coordinates; face is vanilla's 0..5, hand main/off. |
| `block-breaking` | `func(player: player, pos: block-pos, state: string) -> decision` | Before player destruction of a watched block. Cancel keeps it and its drops. |
| `block-placing` | `func(player: player, pos: block-pos, state: string, item: string) -> decision` | Before engine placement of a watched candidate state from item. Cancel preserves block and hand. |
| `block-placed` | `func(player: player, pos: block-pos, state: string)` | A player placed `state` at `pos` in their dimension. |

## `server-hooks`

Access: capability `world`.

API level 28: additive server hook opt-in, capability `world`. Leaves the original server-events.registration record unchanged.

### record `behaviour-config`

API 29: all flags default false. Each family has 32 calls/mod/server tick.

| Field | Type | |
| --- | --- | --- |
| `block-breaking` | `bool` |  |
| `block-placing` | `bool` |  |
| `block-interacted` | `bool` |  |
| `block-neighbour-changed` | `bool` |  |
| `block-random-tick` | `bool` |  |
| `block-scheduled-tick` | `bool` |  |
| `item-used-on-block` | `bool` |  |
| `item-used` | `bool` |  |
| `item-hit-entity` | `bool` |  |
| `item-use-finished` | `bool` |  |
| `item-picked-up` | `bool` |  |
| `chat-sent-filter` | `bool` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `set-block-uses` | `func(enabled: bool, including-sneaking-item: bool)` | Disabled initially. Sneaking with a nonempty main hand bypasses the hook unless including-sneaking-item is true. May be set during init. |
| `set-behaviour-hooks` | `func(config: behaviour-config)` | API 29: replace the opt-in flags; disabled flags cost no guest calls. |
| `watch-blocks` | `func(ids: list<string>) -> result<_, string>` | Exact resource ids or namespace:*, at most 64. No global wildcard. Replaces the list atomically; invalid input leaves the previous list intact. |
| `watch-items` | `func(ids: list<string>) -> result<_, string>` | Replace item id filters atomically, with the same bounds as watch-blocks. |

## `atmosphere`

Access: capability `atmosphere`.

API level 33: experimental capability `atmosphere` (client mods). The mod defines fog and sky profiles keyed by time of day once, then picks a weighted mix of them a few times a second; the game samples, blends and cross-fades them every frame, after vanilla fog and weather and before native mods' fog hooks. Nothing changes in lava, powder snow, blindness, darkness or on the title.

### record `fog-key`

Atmospheric fog at one time of day; keys wrap from the last back to the first.

| Field | Type | |
| --- | --- | --- |
| `day-time` | `u32` | 0..24000 ticks (0 is 6:00); two keys of a list may not share a time. |
| `color` | `u32` | 0xRRGGBB. |
| `start` | `f32` | Where fog starts and where it is full: blocks 0..4096, or fractions of the render distance 0..2 when `relative`. start <= end. |
| `end` | `f32` |  |
| `relative` | `bool` |  |

### record `sky-key`

The sky's colours at one time of day (0xRRGGBB each).

| Field | Type | |
| --- | --- | --- |
| `day-time` | `u32` |  |
| `zenith` | `u32` | Overhead. |
| `horizon` | `u32` | Near the horizon (the colour vanilla fog fades to). |
| `cloud` | `u32` | Clouds. |
| `cloud-far` | `u32` | Distant clouds: kept for later levels, the game tints all clouds with `cloud` for now. |

### record `water-fog`

Fog with the camera under water: blocks 0..4096, start <= end.

| Field | Type | |
| --- | --- | --- |
| `color` | `u32` |  |
| `start` | `f32` |  |
| `end` | `f32` |  |

### record `profile`

Up to 32 keys per list; a part a profile leaves out (no fog keys, no sky keys, no water fog) stays vanilla where it is mixed in. At least one part.

| Field | Type | |
| --- | --- | --- |
| `fog-keys` | `list<fog-key>` |  |
| `sky-keys` | `list<sky-key>` |  |
| `water-fog` | `option<water-fog>` |  |

### enum `eye-medium`

What the camera is in.

| Case | |
| --- | --- |
| `air` |  |
| `water` |  |
| `lava` |  |
| `powder-snow` |  |

### record `scene`

What a mod picks its mix from, read once per tick.

| Field | Type | |
| --- | --- | --- |
| `rain` | `f32` | 0..1, as the frame draws them. |
| `thunder` | `f32` |  |
| `day-time` | `u32` | 0..24000. |
| `moon-phase` | `u8` | 0 (full) to 7. |
| `camera-x` | `f64` |  |
| `camera-y` | `f64` |  |
| `camera-z` | `f64` |  |
| `yaw` | `f32` |  |
| `pitch` | `f32` |  |
| `eye` | `eye-medium` |  |
| `sky-light` | `u8` | 0..15 at the camera's block. |
| `block-light` | `u8` |  |
| `sky-visible` | `bool` | No block above the camera's column blocks the sky. |

### record `span`

A value range: each particle (or spawn) draws one uniformly in lo..=hi; lo <= hi.

| Field | Type | |
| --- | --- | --- |
| `lo` | `f32` |  |
| `hi` | `f32` |  |

### record `span3`

One range per world axis.

| Field | Type | |
| --- | --- | --- |
| `x` | `span` |  |
| `y` | `span` |  |
| `z` | `span` |  |

### record `wave`

`amplitude * sin(2 pi (age / period + phase))`, age in seconds; each particle draws its own amplitude, period (0.01..600 s) and phase (turns).

| Field | Type | |
| --- | --- | --- |
| `amplitude` | `span` |  |
| `period` | `span` |  |
| `phase` | `span` |  |

### enum `ease`

How a curve goes from one key to the next.

| Case | |
| --- | --- |
| `linear` |  |
| `smooth` |  |
| `step` |  |

### record `curve-key`

A curve's value at a fraction `t` (0..1) of the particle's life.

| Field | Type | |
| --- | --- | --- |
| `t` | `f32` |  |
| `value` | `f32` |  |

### record `curve`

`base * keys(life) + wave(age)`: the base is drawn once per particle; no keys means 1, one key is constant, and up to 8 keys are sorted by `t` and eased between.

| Field | Type | |
| --- | --- | --- |
| `base` | `span` |  |
| `keys` | `list<curve-key>` |  |
| `ease` | `ease` |  |
| `wave` | `option<wave>` |  |

### record `color-key`

A colour (0xRRGGBB) at a fraction `t` of the life, blended linearly.

| Field | Type | |
| --- | --- | --- |
| `t` | `f32` |  |
| `color` | `u32` |  |

### record `curve3`

| Field | Type | |
| --- | --- | --- |
| `x` | `curve` |  |
| `y` | `curve` |  |
| `z` | `curve` |  |

### enum `emitter-shape`

Where an emitter spawns around its anchor plus `offset`.

| Case | |
| --- | --- |
| `point` | At the offset. |
| `box` | In the box of `half-size` (on a face with `surface`). |
| `disc` | In a flat horizontal disc of `radius` (on its rim with `surface`). |
| `sphere` | In a ball of `radius` (on its shell with `surface`). |

### record `emitter`

| Field | Type | |
| --- | --- | --- |
| `looping` | `bool` | Loops forever, or spawns for `duration` seconds and ends. |
| `duration` | `f32` | Seconds, 0.05..600. |
| `rate` | `f32` | Particles per second, 0..2048. |
| `density-scaled` | `bool` | The rate scales with `start-emitter`'s density. |
| `max` | `u32` | This emitter's live particles at most, 1..512. |
| `burst` | `u32` | Spawned at once when it starts, 0..512 (counted in `max`). |
| `shape` | `emitter-shape` |  |
| `offset` | `span3` | From the anchor, drawn per particle: blocks, -64..64. |
| `half-size` | `tuple<f32, f32, f32>` | The box's half extents: blocks 0..64. |
| `radius` | `span` | The disc's or ball's radius, drawn per particle: blocks 0..64. |
| `surface` | `bool` |  |
| `direction` | `option<span3>` | The direction `speed` pushes along, normalised; none: outwards from the shape's centre (a random direction for a point). |

### enum `medium`

Where a particle may live: it dies on leaving it.

| Case | |
| --- | --- |
| `anywhere` |  |
| `air` |  |
| `water` |  |

### enum `frame-mode`

Which sprite of the set a particle shows.

| Case | |
| --- | --- |
| `first` |  |
| `random` | One picked at spawn. |
| `over-life` | First to last over the life. |
| `loop` | Cycling at `frame-rate` per second. |

### record `particle`

| Field | Type | |
| --- | --- | --- |
| `sprite` | `string` | A sprite set: `<namespace>:<name>` for `assets/<namespace>/particles/<name>.json` in the mod's own content (a namespace it owns, capability `registries`), or `minecraft:<type>` for a vanilla particle type's sprites. |
| `frame` | `frame-mode` |  |
| `frame-rate` | `f32` | Sprites per second for `loop`, 0..60. |
| `lifetime` | `span` | Seconds, 0.05..60. |
| `speed` | `span` | Blocks per second along the direction, -64..64. |
| `velocity` | `span3` | Added to the start velocity: blocks per second, -64..64. |
| `drag` | `f32` | Velocity lost per second (`v *= exp(-drag * dt)`), 0..20. |
| `gravity` | `f32` | Downwards, blocks per second squared, -40..40. |
| `acceleration` | `option<curve3>` | Blocks per second squared per axis; curve values -64..64. |
| `size` | `curve` | Quad size in blocks, 0..16. |
| `alpha` | `curve` | 0..1 (clamped after sampling). |
| `color` | `u32` | 0xRRGGBB, used when `color-keys` is empty. |
| `color-keys` | `list<color-key>` | Up to 8. |
| `shade` | `f32` | Each particle darkens by up to this fraction, 0..1. |
| `spin` | `span` | Start roll in degrees and roll per second. |
| `spin-rate` | `span` |  |
| `fade-near` | `option<span>` | Invisible nearer than `lo` blocks from the camera, full from `hi` (0..128). |
| `fade-far` | `option<span>` | Full until `lo` blocks from the camera, gone from `hi` (0..128). |
| `emissive` | `bool` | Full bright instead of the world's light. |
| `translucent` | `bool` | Drawn on the translucent particle layer (otherwise cut out). |
| `only-in` | `medium` |  |
| `die-nearer-than` | `f32` | Dies when the camera comes nearer than this many blocks, 0 (off)..16. |

### record `effect`

A particle effect: how an emitter spawns and how each particle moves and looks.

| Field | Type | |
| --- | --- | --- |
| `emitter` | `emitter` |  |
| `particle` | `particle` |  |

### variant `anchor`

What an emitter is placed at: the camera (followed, world axes) or a fixed point.

| Case | Payload | |
| --- | --- | --- |
| `camera` |  |  |
| `point` | `tuple<f64, f64, f64>` |  |

| Function | Signature | What it does |
| --- | --- | --- |
| `define-profile` | `func(profile: profile) -> result<u32, string>` | Stores a profile (at most 64 per mod) and returns its id. Profiles are never removed. |
| `set-mix` | `func(weights: list<tuple<u32, f32>>, transition-seconds: f32) -> result<_, string>` | Blends up to 4 of this mod's profiles: weights >= 0, normalised by the game; an empty list returns to vanilla. The game cross-fades from what it shows now over `transition-seconds` (0..30, clamped). Errors keep the previous mix. More than 8 calls per second are ignored. With several mods, the first in load order with a mix wins. |
| `conditions` | `func() -> option<scene>` | None on the title screen. |
| `define-effect` | `func(effect: effect) -> result<u32, string>` | Stores an effect (at most 256 per mod) and returns its id. Effects are never removed. |
| `start-emitter` | `func(effect: u32, anchor: anchor, density: f32, tint: u32) -> result<u32, string>` | Starts an emitter of one of this mod's effects (at most 48 live per mod); `density` 0..4 scales a density-scaled rate, `tint` (0xRRGGBB) multiplies the colour. The game simulates its particles; a mod's live particles are capped at 3072 and its spawns at 512 per tick. Nothing spawns on the title screen, while paused, with Particles: Minimal, or more than 64 blocks from the camera. A one-shot emitter's id frees when it ends. |
| `move-emitter` | `func(emitter: u32, anchor: anchor) -> result<_, string>` | Moves a live emitter. |
| `stop-emitter` | `func(emitter: u32, fade: bool) -> result<_, string>` | Stops a live emitter; with `fade` its particles live out their lives, without they go at once. Its id may be reused. |

## `channel`

Access: capability `channel`.

API level 28: experimental capability `channel`, on either side.

| Function | Signature | What it does |
| --- | --- | --- |
| `send` | `func(name: string, data: list<u8>) -> result<_, string>` | Client to the same mod id on the server. Name [a-z0-9_./-]{1,64}. Data at most 65536 bytes, 32 messages/mod/tick across all hooks. Err when disconnected, unnegotiated, missing peer mod, or over a limit. |
| `send-to-player` | `func(player: string, name: string, data: list<u8>) -> result<_, string>` | Server to an online player by UUID or exact name, with the same limits. |

## `channel-events`

Access: exported by the mod: the game calls it.

API level 28: optional client export, gated by capability `channel`.

| Function | Signature | What it does |
| --- | --- | --- |
| `message` | `func(name: string, data: list<u8>)` | A complete message from the same mod id on the server, at most 32/tick. |
