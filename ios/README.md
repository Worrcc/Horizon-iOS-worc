# Horizon on iPhone and iPad

[![ios](https://github.com/Worrcc/Horizon-iOS-worc/actions/workflows/ios.yml/badge.svg?branch=claude/minecraft-ios-port-w46z3k)](https://github.com/Worrcc/Horizon-iOS-worc/actions/workflows/ios.yml)

This fork of [Horizon](https://github.com/horizon-rust/horizon) (based on `launcher@0.2.20`)
runs the game on iOS and iPadOS. It is the same Rust game: same worlds, same rules, same
renderer (wgpu on Metal). The iOS port adds touch controls, keyboard and controller support, and
handles the iOS app lifecycle.

> Not an official Minecraft product, and not approved by or associated with Mojang, Microsoft or
> the Horizon project. Like Horizon, the app contains no Minecraft code, assets or data: you
> prepare the game files from **your own** copy of Minecraft: Java Edition 26.3, on a computer,
> and copy them to your device.

**Status: early.** The iOS build is type-checked (`ios/scripts/check-ios.sh`), the touch logic
is unit tested, and the desktop build and tests still pass. It has **not yet been run on a
device**. Expect rough edges the first time it starts on real hardware, and please report them.

## What you need

- A **Mac with Xcode** (15 or newer) and [Rust](https://rustup.rs) to build the app. Linking for
  iOS needs Apple's SDK, so the app can't be built on Linux or Windows.
- An **iPhone or iPad on iOS 15 or later**. An A12 chip or newer is recommended; iPads with M
  chips run it best.
- **Minecraft: Java Edition 26.3**, played once, and **Java 25** on a computer, to prepare the
  game files once (the same requirements as Horizon on desktop).
- A way to install apps you built yourself: a free or paid Apple ID with
  [SideStore](https://sidestore.io), [AltStore](https://altstore.io) or
  [Sideloadly](https://sideloadly.io), or Xcode with your Apple ID.

## 1. Prepare the game files (on a computer)

Use either of these:

- **The Horizon launcher** (easiest): install it from [playhorizon.dev](https://playhorizon.dev)
  and let it prepare a version. The prepared files are the `resources` folder of that version:
  - macOS: `~/Library/Application Support/horizon-launcher/versions/<version>/resources`
  - Linux: `~/.local/share/horizon-launcher/versions/<version>/resources`
  - Windows: `%APPDATA%\horizon-launcher\versions\<version>\resources`
- **From this repository** (see the main [README](../README.md#build-from-source)):

  ```sh
  cargo run --release -p horizon-prepare -- --prepare --server-jar ~/Downloads/server.jar
  cargo run --release -p horizon-prepare -- sounds   # optional: sounds
  ```

  The prepared files land in `.local/horizon-client/26.3`.

Either way you get a folder holding `horizon-resources.json`. Below it is called the
**resources folder**. It is derived from Mojang's files: keep it for your own devices and never
share it.

## 2. Build the app (on a Mac)

No Mac? The repository's **ios** GitHub Actions workflow builds the app on every push. Open a
run under **Actions › ios** and download **Horizon-ios-…**, which holds an unsigned
`Horizon.ipa` (no game files) for step 3. Then do step 4.

On a Mac:

```sh
git clone https://github.com/Worrcc/Horizon-iOS-worc.git
cd Horizon-iOS-worc
ios/build.sh
```

This builds `target/ios/Horizon.app` and `target/ios/Horizon.ipa` (a release build. The first
build takes a while). Options:

| Option | What it does |
|---|---|
| `--resources DIR` | Puts your resources folder inside the app, so step 4 isn't needed. The .ipa then holds Mojang-derived files: install it on your own devices only and never share it. |
| `--sign ID --profile FILE` | Signs the app with your certificate and provisioning profile, to install with Xcode's tools (see below). |
| `--bundle-id ID` | The app's bundle id (default `io.github.worrcc.horizon`). |
| `--sim` | A build for the iOS Simulator on Apple Silicon Macs. The simulator's GPU lacks some features the renderer may need. |
| `--debug` | A debug build: much slower in game. |

## 3. Install it

**With SideStore, AltStore or Sideloadly** (works with a free Apple ID): open
`target/ios/Horizon.ipa` in the tool and install it. With a free Apple ID, apps expire after 7
days. SideStore and AltStore refresh them for you.

**With Xcode's tools and your own signing**: you need a development certificate and a
provisioning profile for the bundle id:

1. In Xcode, create any iOS app project with the bundle id you'll use (for example
   `io.github.worrcc.horizon`), pick your team, and run it once on your device. Xcode then
   creates the profile in `~/Library/Developer/Xcode/UserData/Provisioning Profiles`.
2. `security find-identity -v -p codesigning` lists your signing identities.
3. Build, sign and install:

   ```sh
   ios/build.sh --sign "Apple Development: Your Name (TEAMID)" \
       --profile ~/Library/Developer/Xcode/UserData/Provisioning\ Profiles/<uuid>.mobileprovision \
       --bundle-id io.github.worrcc.horizon
   xcrun devicectl list devices
   xcrun devicectl device install app --device <id> target/ios/Horizon.app
   ```

The app's first start on the device may need **Settings › General › VPN & Device Management**
to trust your developer certificate. On iOS 16 or later, also turn on
**Settings › Privacy & Security › Developer Mode**.

## 4. Copy the game files to the device

Skip this if you built with `--resources`.

Horizon's game folder is the app's **Documents** folder, which you can open from your computer
and from the Files app:

- **Mac:** connect the device, select it in a Finder window, open **Files**, and drag your
  resources folder onto **Horizon**. Then rename it to `resources` if it has another name.
- **Windows:** the same in the Apple Devices app (or iTunes), under **File Sharing**.
- **On the device:** put the folder in iCloud Drive, then in the Files app move it to
  **On My iPhone › Horizon** and name it `resources`.

The folder should end up as `Horizon/resources/horizon-resources.json`. Your worlds
(`saves/`), settings (`options.txt`), screenshots, logs and crash reports are in the same
Horizon folder. That is how you back worlds up or copy them to and from a computer.

If the game files are missing, the app says so (and where its crash report is), then closes.

### The launcher and instances

Horizon opens on its launcher: your instances, and for the one you're playing, **Mods...**,
**Shader Packs...**, **Controller Settings...** and **Options...**. **Done** (or B on a
controller) goes to the title screen. The same instance list is in **Options › Horizon
Settings › Instances**.

Each instance is a separate game folder with its own worlds, settings, resource packs, shader
packs and mods. The Horizon folder itself is the first instance. **New Instance** makes another
one in `Horizon/instances/`; you can also copy a folder in there with the Files app. Tap an
instance to play it: with no world open the game switches at once, from a world at the next
start. Every instance uses the same `resources` folder, and logs stay in `Horizon/logs`.

### Mods

Horizon's mods (`.hmod` files) work on iOS: code mods run on an interpreter, since iOS doesn't
let apps compile code, so they are slower than on a computer but need no JIT. Copy a `.hmod`
into the instance's `horizon-mods` folder with the Files app (`Horizon/horizon-mods/`, or
`Horizon/instances/<instance>/horizon-mods/`), then turn it on in the launcher's **Mods...**.
Each mod stays off until you do.

The app's build comes with Horizon's **Block Info** mod (`block_info.hmod`, next to the
`.ipa`): it shows the block under the crosshair, its icon, name and the game or mod it comes
from, in a box at the top of the screen.

## Controls

Installed as its own app, Horizon runs in landscape. Under LiveContainer it follows the phone's
orientation, so turn the phone sideways to play in landscape.

On iPhones with a 120 Hz (ProMotion) screen the game asks iOS for 120 Hz; the frame limit in the
video settings still applies. iOS only allows an iPhone app past 60 Hz when its Info.plist opts
in (`CADisableMinimumFrameDurationOnPhone`, set in Horizon's). Under LiveContainer it is
LiveContainer's Info.plist that counts, so the game may stay at 60 there.

**Touch** (shown once you touch the screen):

| Where | What |
|---|---|
| Left half | A joystick appears under your thumb. Push it past its ring to sprint. |
| Right half | Drag to look. Tap to use or place (or to attack a mob under the crosshair). Hold still to break blocks. |
| **Jump**, **Sneak** | Jump while held. Sneak toggles on and off. |
| **Use** | Right click, held: eat, drink, draw a bow, block with a shield, trade. |
| **Drop**, **...** | Drop the held item, open the inventory. |
| **T**, **F5**, **\|\|** | Chat, change perspective, pause. |
| Hotbar | Tap a slot to select it. |
| Menus | Tap to click. Swipe to scroll. Press and hold, then drag, to move stacks, sliders and scroll bars. **Back** (top right) is Esc; **Aa** (top left) shows the keyboard for text fields. Chat, signs and command blocks open it by themselves. |

**Controllers**: Xbox, PlayStation (DualShock 4, DualSense), Switch Pro and MFi pads work
through Apple's GameController framework, with the same bindings as Horizon on desktop
([docs/controllers.md](../docs/controllers.md)). Pair them in iOS's Bluetooth settings.
Every menu works with a controller alone: on the first title screen the left stick or D-pad
moves a pointer, A clicks and B goes back. The touch controls hide while you play with a controller and come back at your next touch.
Vibration plays through Core Haptics. A DualSense's adaptive triggers resist as a bow draws,
click for a loaded crossbow and buzz while mining, and its lightbar (a DualShock 4's too)
follows your health or dimension. Each can be turned off in Controller Settings.

**Keyboards**: an iPad's Magic Keyboard or a Bluetooth keyboard works as on desktop (WASD,
E, Esc, F3 and so on).

## What doesn't work yet

- **Online-mode servers** need Microsoft sign-in, which needs a one-time setup (below). Until
  then you can play single player, Open to LAN, and offline-mode servers under an offline name.
- **Mouse look** with an iPad trackpad or mouse: they act as touches for now.
- **Discord presence** and **`horizon-ctl`** (there's no terminal on iOS).
- **Open to LAN discovery**: finding LAN worlds by multicast needs Apple's multicast entitlement.
  Joining by address works.

iOS can close a backgrounded app at any time, so Horizon pauses and **saves the world whenever
it goes to the background**. On devices with little memory, lower the render distance in the
video settings if the app closes while playing.

## Microsoft sign-in

Online-mode servers check that you own Minecraft: Java Edition, so the game signs in to your
Microsoft account. On iOS there is no launcher to do that, so the game does it itself. Tap the
**Sign in** button at the top right of the title screen. A popup shows a code, copies it and opens
microsoft.com/link; sign in there, then come back. The popup closes by itself and the button
shows your player name. Your sign-in is kept in the iOS Keychain (never in the Files app) and
renewed at each start.

**One-time setup, for whoever builds the app.** Signing in needs an *Azure app registration*
(a client ID) that Mojang has approved for Minecraft. Horizon's own isn't public, and using
another app's is against its rules, so register your own:

1. Register an app at https://portal.azure.com (Microsoft Entra ID › App registrations › New
   registration). Choose **Personal Microsoft accounts only**; no redirect URI is needed. Then,
   under Authentication › Advanced settings, set **Allow public client flows** to **Yes**.
   Copy the **Application (client) ID**.
2. Ask Mojang to approve it for Minecraft, with the form at https://aka.ms/mce-reviewappid
   (Minecraft Java "App ID review"). It takes days to weeks. Until then, sign-in stops with
   "not approved by Mojang yet".
3. Give the build the ID. On GitHub: the repository's **Settings › Secrets and variables ›
   Actions › Variables**, add `HORIZON_MSA_CLIENT_ID` with the ID (a variable, not a secret:
   client IDs are public). Then the next **ios** build has sign-in. With `ios/build.sh`,
   set the same environment variable when building.

[docs/microsoft-account.md](../docs/microsoft-account.md) describes each step of the sign-in.

## For developers

- `ios/scripts/check-ios.sh` type-checks the whole game for `aarch64-apple-ios` on any host,
  Linux included. Stub headers stand in for the SDK. Run it after changes that might affect
  iOS.
- What the port changed:
  - `crates/horizon-client/src/game/ios.rs`: the iOS lifecycle. `run_app` never returns and
    `exit` does nothing there, so this file ends the run, alerts on errors, saves when going to
    the background, and sets the window's orientation and edge gestures.
  - `crates/horizon-client/src/touch_controls.rs`: the touch controls' logic, unit tested.
    `game/touch.rs` applies and draws them.
  - `crates/horizon-client/src/msa.rs` and `game/account.rs`: Microsoft sign-in (the
    launcher's device-code flow, the title screen's button and its popups, the Keychain).
  - `crates/horizon-client/src/game/pad/hardware_ios.rs`: the controllers.
    `crates/horizon-ios` holds the UIKit and GameController calls (alert, clipboard is in
    `horizon-client-input`), so the game crate keeps `forbid(unsafe_code)`.
  - `third_party/winit`: winit 0.30.13 with an iOS keyboard patch (marked "Horizon" in
    `src/platform_impl/ios/`: `keyboard.rs`, `view.rs`, `view_controller.rs`, `window.rs`).
    It delivers hardware key presses, and gives the on-screen keyboard's characters key codes.
    Drop it when winit handles both.
  - Elsewhere, small `cfg(target_os = "ios")` changes: the Documents folder as the game folder
    (`logging.rs`, `main.rs`), the resources path (`game.rs`), Metal-only on all Apple targets
    (`gfx.rs`, `horizon-hdr`), `UIPasteboard` (`horizon-client-input/src/clipboard.rs`).
- Updating to a newer Horizon release: this branch keeps upstream's history, so merge the new tag:

  ```sh
  git remote add upstream https://github.com/horizon-rust/horizon.git
  git fetch upstream tag launcher@0.2.21 --no-tags
  git merge launcher@0.2.21
  ios/scripts/check-ios.sh
  ```
