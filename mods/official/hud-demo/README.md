# HUD Demo (API level 35)

Client mod; `/hud35 on` refreshes four times a second. A gold line below the vanilla food bar
shows current saturation plus the held food's saturation preview. Mounting removes that line
when mount hearts replace food. `/hud35 off` clears it.

`/hud35 stats`, `/hud35 food minecraft:apple`, `/hud35 entities 48 48`, `/hud35 kinds` and
`/hud35 bars` print the read-only observations. `/hud35 show` samples everything once.
`/hud35 budget` deliberately triggers the four-query frame limit for sandbox testing.
Exhaustion is optional: only the local integrated server supplies it. HUD bounds describe the
last rendered vanilla layout; before the first HUD frame or after resizing the call can fail.
Build with the existing `cargo horizon-mod build mods/official/hud-demo` workflow.
