# Touch and controller input for mods: plan (approved by the user 2026-11-10)

Branch claude/liteminer-api. Four parts, built and reviewed one at a time (A, then B, then C).

## Part A: touch on mod screens, and tapping a mod's HUD box

A1. Mod canvas screens (`canvas.open-canvas-screen`; game/mod_canvas_screen.rs) with a finger:
- today a quick finger drag on any menu is a scroll and only becomes a pointer drag after holding still
  (touch_controls.rs, Finger::Pointer: Pending -> Scrolling before POINTER_HOLD). On a mod canvas screen that
  makes dragging impossible (Diorama's "Move minimap" screen and its fullscreen map).
- New rule, only while a mod canvas screen is the open screen: one finger = pointer Down at once, Drag while it
  moves, Up when lifted (no tap slop delay beyond what a tap needs; a tap is still Down+Up at the same point).
  Two fingers = pinch: the change in distance between them is sent as `ui-events.scroll(x, y, steps)` at their
  midpoint (about one step per 24 GUI px of distance change; spreading = positive, as wheel-up zooms in), and no
  pointer events while two fingers are down (send Up for the first finger when the second lands).
- The game's own menu touch buttons on that screen ("Back", "Aa") keep working.

A2. API level 21 (new function only, older mods link unchanged), in the `canvas` interface:
```wit
/// API level 21: a tap on the mod's canvas box (set-corner / set-placed) acts as the mod's
/// registered key `name` (input.register-key): finger down inside the box sends key-down(name),
/// lifting sends key-up(name), and the touch does not turn the camera, break or place. Only while
/// playing with the touch controls shown. none: the box takes no touches again (the default).
/// An unknown key name is an error.
set-box-key: func(name: option<string>) -> result<_, string>;
```
The hit area is the box as drawn (wherever set-placed put it, after clamping). Game touch buttons and mod touch
buttons win where they overlap it.

## Part B: mod keys as on-screen buttons, and an editor for the touch layout

B1. Automatic buttons. While playing with the touch controls shown (they hide when a keyboard or controller
takes over: keep that rule, and make sure a connected-but-idle controller on iOS behaves as the code already
decides for the game's own buttons), every key a mod registered with `input.register-key` that the mod did not
give its own button (`input.set-touch-button`) gets a button automatically:
- look: the same slot frame as mod touch buttons, 20x20 GUI px, label = up to 4 characters made from the key's
  label (one word: its first 3-4 letters; several words: their initials, e.g. "Show recipes" -> "SR"); two keys
  of one mod that would get the same label get a digit appended.
- default place: they sort themselves onto the RIGHT side of the screen: a column down the right edge starting
  under the top-right row (F5 / T / pause, which is at y = 54, 20 high) with 2 px gaps, in a stable order (mods
  in load order, keys in registration order); when the column would reach the Drop/Use/Jump/Sneak cluster it
  continues in a new column to the left. Never overlapping a game button or a mod's own button.
- they behave exactly as mod touch buttons do (key-down on finger down, key-up on lift, no camera/break).
- keys that arrive or go as mods start/stop re-flow the automatic ones that the player has not moved.

B2. "Edit Touch Layout" screen (open it from Options > Controls and from the Mods page, a button
"Edit Touch Layout..."; only offered when touch controls can show, i.e. iOS or HORIZON_TOUCH=1):
- shows every touch button in place over the paused/blurred game: the game's own (stick, Jump, Sneak, Use, Drop,
  "...", F5, T, pause), mods' own buttons, and the automatic key buttons.
- drag a button to move it (clamped fully on screen, snapped to 2 GUI px); tap selects it (gold frame) and shows
  its name (game: the action's name; mod: "<Mod name>: <key label>").
- bottom bar: "Hide" / "Show" for the selected button (pause cannot be hidden; a hidden button is drawn faint in
  the editor and not at all in play), "Reset" (selected back to its default), "Reset All", "Done".
- saved per instance (a small text file beside options.txt, e.g. horizon-touch.txt): one line per button id
  (`game.jump`, `game.stick`, `mod.<modid>.<key name>`) with x, y in GUI px from the top left as fractions of the
  screen (so they survive rotation / scale changes) and hidden true/false. A player override wins over a mod's
  own anchor/offset for its button; Reset returns it to the mod's.
- lang keys for all text.

## Part C: controller combinations, and removing a binding

- A controller binding is one button or two buttons together (a chord), for EVERY controller binding (game
  actions and mod keys). Y+A is a different binding from A.
- Binding UI (controller options page): while it waits for a button, pressing one binds that button; holding one
  and pressing a second binds the chord (shown as "Y + A"). Each row gets a button that REMOVES the binding
  (the action then has no controller binding; shown as "Not bound").
- Mod keys: every key a mod registers is listed on the controller page (a "Mods" section, "<Mod name>: <label>"),
  unbound by default, bindable to a button or chord, and then sends the mod key-down / key-up like the keyboard
  key does. (If mod keys already have controller bindings, extend what exists.)
- Resolving: a chord fires when its two buttons go down within 50 ms of each other, or when one is held and the
  other is pressed; while a chord is active neither button's single action fires, and the chord's action stays
  held until either button is released. Only buttons that are part of some bound chord wait those 50 ms before
  their single action fires (a release inside the window fires the single action as a tap at once); all other
  buttons stay instant. Unit-test the resolver as a pure state machine (press order, timing edges, release
  order, three buttons, a button in two chords).
- Stored bindings stay readable: old single-button settings load unchanged.

## Rules for every part
New functions only in the mod API; no behaviour change for mods built for level <= 20. Tests for the pure
logic; clippy with the mods feature (warnings denied); rustfmt on touched files; both fast builds
(CARGO_TARGET_DIR=/home/worc/Projects/iaf-swarm/liteminer-api/target, --offline). horizon-ctl: where a part
cannot be driven with the existing commands (touch down|move|up|tap with --id for two fingers, pad, click-at,
ui), add what is missing and document it in docs/horizon-ctl-commands.md. Docs: docs/mods.md (+ the API
reference) and ios/README.md "Controls". No commit, no push. If something does not fit, stop and report
rather than redesign.
