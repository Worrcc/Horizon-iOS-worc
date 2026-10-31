# Horizon-iOS: handoff

Where the iOS port stands, what was verified on a real device, and what to do next. Written for
whoever picks this up next, person or agent. [README.md](README.md) is the player-facing guide.
**The next big piece of work, porting Ice and Fire's AI, has its own plan:
[ICE-AND-FIRE-AI-PLAN.md](ICE-AND-FIRE-AI-PLAN.md).**

## The state of things

- **Branches** in `Worrcc/Horizon-iOS-worc` (public). No pull requests have been opened.
  - `claude/minecraft-ios-port-w46z3k`: the port itself (the first table below).
  - `claude/launcher-mods-shaders-controller-xfv3kk`: **the current work**. It starts from the
    port and adds the launcher, instances, controller work, code mods on iOS, Block Info and the
    first part of mod entities (the second table). Continue from here.
- **Latest build:** commit `5f072f03d`, CI run
  [41207777202](https://github.com/Worrcc/Horizon-iOS-worc/actions/runs/41207777202), succeeded.
  Its artifact `Horizon-ios-5f072f03…` holds the unsigned `Horizon.ipa` and `block_info.hmod`
  (no game files, kept 30 days). The `ios` workflow's `push` trigger builds every branch.
- **Pushing from the player's Linux machine** works through a repository-local credential
  helper reading a fine-grained token from `~/.config/horizon-gh-token` (never print it);
  commits there need `git -c user.name=Claude -c user.email=noreply@anthropic.com`.
- **Test device:** an iPhone with an A16 GPU (430×932 pt screen, ProMotion), running Horizon
  **inside LiveContainer**. The player also has StikDebug (JIT).

### The port: commits on top of upstream

Upstream is Horizon `launcher@0.2.20` (`b6c936f82`) with its full history.

| Commit | What |
|---|---|
| `ab258ce89` | Builds for iOS; Documents as the game folder; lifecycle (`game/ios.rs`); touch controls; vendored winit keyboard patch; GameController backend; `horizon-ios` crate |
| `8044eb16b` | `ios/build.sh`, Info.plist, icons, `ios.yml` workflow, upstream workflows guarded to `horizon-rust/horizon` |
| `490e58e7c`, `46536e749` | Workflow tweak; README badge |
| `739b81aad` | Fatal errors show in an alert (own window, retried until shown); up-front check for `resources/` |
| `e4bf07b12` | Metal backend on iOS (`gfx.rs` `backend_order` only knew macOS) |
| `f44ba77ff` | Touch on the startup title screen (`MenuTouch`); winit reports the view's full bounds; iOS 16 orientation request |
| `a2b7491d2` | Microsoft sign-in (`msa.rs`, `game/account.rs`, Keychain, alerts with answers) |
| `bb9f7edb2` | Window accepts every orientation; every resize logs UIKit's geometry |
| `b2a931271` | iOS window no longer gets the desktop's 1280×720 size (it was larger than the screen, off-screen) |
| `22e327920` | ProMotion: `CADisableMinimumFrameDurationOnPhone` and a display link preferring 120 Hz |
| `bb59430b` | This handoff file |

### The launcher, controllers and mods: `claude/launcher-mods-shaders-controller-xfv3kk`

| Commit | What |
|---|---|
| `0deb09fc` | **Instances**: `src/instance_list.rs` (Documents is the default, others in `instances/<id>/`, the pick in `instances/instances.json`), `game/instances.rs`; `main.rs` starts in the picked folder; `resources/` is shared from Documents |
| `d859ac76` | Touch controls hide while a controller is used, and come back at the next touch (`game/touch.rs` `touch_hide`) |
| `6409bfd3` | Controller **vibration** through Core Haptics (`horizon-ios/src/controllers.rs` `vibrate`: a `CHHapticEngine` per pad through raw `msg_send`) |
| `ac0c8a68` | DualSense **lightbar and adaptive triggers** (`GCDeviceLight`, `GCDualSenseAdaptiveTrigger`). The desktop effects are read back with `horizon_dualsense::trigger::decode`, and the trigger feel logic moved to `pad.rs` so both backends share it |
| `89b6e862` | Controllers on the **startup title** while resources load (`pad.rs` `StartupPad`: the stick or D-pad moves a pointer, A clicks, B goes back). Its pads go on in the game |
| `7af8507e` | The **launcher page** the app opens on (`Page::Launcher` in `options_ui.rs`, opened by `Loading::open_launcher`): instances, Mods, Shader Packs, Controller Settings, Options, New Instance. Switching instances rebuilds the title's world list, and switches at once on `Game`'s title with no world open too |
| `fdd6591c` | **Block Info** (`mods/official/block-info`, id `block_info`): a Jade-style box showing the block under the crosshair, written for Horizon's API with no code or art from Jade. Listed in `docs/mods.md` |
| `5f03c66a` | **Code mods on iOS** on wasmtime's Pulley interpreter (no JIT): `horizon-mods/src/runtime.rs`, `ios/build.sh --features mods`. The ios workflow also builds `block_info.hmod` |
| `b9f6f05e` | **Mod entities, part 1**: mobs declared from a vanilla template (`data/<id>/horizon/entity/*.json`, see `docs/modpacks.md`). `horizon-port-pack` names ports "<name> (unofficial port)" and credits the original authors |
| `f04d0200` | Mod mobs' own texture, `scale` and loot; `mobs::vanilla_id` for vanilla's tables by template |
| `a67e4696` | **Mod entities, part 2**: models of their own (GeckoLib / Bedrock geometry, `mobs/geo.rs`) and animations (`mobs/bedrock_anim.rs`) |
| `871e7ef8` | **Tabula models** carried over by `horizon-port-pack` (`tabula.rs`), pose folders as animations; box UV fix in geo.rs |
| `1e2cddac`, `fd87a760` | **Capability `entities`** for server mods (API level 8): `mobs`, `near`, `give-orders`; animations synced on `horizon:entity_animation`; example `mob-orders`; review fixes |
| `bafffa15` | iOS launcher: the Mods page lists mods on disk; switching instances reloads packs only when they differ |
| `0c1978a9` | Entities: `spawn`, `target`, `scale` (`Attr::Scale` for every mob), `set-data`, data and animation in the save, brains wait for orders |
| `5f072f03` | Entities: `interactions` (players' right clicks on content mobs) |

## Verified on the device

- Installs and launches under LiveContainer; finds `Documents/resources`.
- Metal renderer works (A16): the title screen and a world render with the real textures.
- The missing-files alert and fatal-error alerts show (they used to leave a black screen).
- Resource loading takes about 12 s on the A16 (`latest.log`: "resources loaded in 12.7s").
- **Block Info works on the device** (build `5f03c66a`, reported by the player): a code mod
  running on the Pulley interpreter, turned on from the launcher's Mods page.
- **A converted Ice and Fire fire dragon runs on the device** (build `5f072f03d`, player's
  `latest.log` 12:37: content from 1 mod, "Summoned new Fire Dragon"); the launcher's Mods page
  lists it, and switching instances is "much better" (player). With that content mod on,
  resources load in **20.6 s** instead of 12.7 s: see the plan's "Performance to watch".

## Waiting on the player's test

Each item is in a build, but nobody has reported on it yet.

1. **Launcher and instances:** the app opens on the launcher. New Instance, then switching
   (before and after the ~12 s load) should give that instance's own worlds and settings.
   Mid-session switches reload settings and packs. Keybinds and language may need the next
   start: check.
2. **Controllers:** the arrow on the first title screen; touch controls hiding; vibration on
   damage and block breaking. On a DualSense: lightbar colour, bow-draw resistance, crossbow
   click. Apple's trigger vibration frequency is 0..1; the desktop's Hz are divided by 255, which
   may feel weak.
3. **From the port's first build:** whether the picture fills the screen, title-screen touch,
   120 Hz (under LiveContainer it's LiveContainer's Info.plist that counts) and orientation.
   `latest.log` logs `view bounds / frame / window / screen / scene orientation` at startup and at
   every resize. Read it before changing anything.

## Blocked on the owner (not code)

- **Microsoft sign-in needs an approved Azure client ID.** Register an app (personal accounts,
  public client flows on), apply for Minecraft API access at https://aka.ms/mce-reviewappid
  (days to weeks), then add the repo **variable** `HORIZON_MSA_CLIENT_ID`. The `ios` workflow
  passes it to the build. Never use Horizon's or another launcher's ID (docs/microsoft-account.md).
  Until then, the Sign in button explains what's missing. The flow itself is untested against
  the real services. Its unit tests cover parsing only.

## What the player asked for, and what's next

The player's goals: a launcher inside the app (instances, mods, shader packs; modpacks set aside
for now), controllers as a full input path, Jade (done as Block Info), and Ice and Fire
("build the API to allow it to run; give all credits; call it unofficial").

- **Ice and Fire** cannot be copied into the repository: its code and art belong to its authors,
  and credit doesn't change that. The player's jar is **Ice and Fire Community Edition 2.0,
  LGPL-3.0**: its AI may be ported closely, in its own LGPL project outside Horizon's MIT tree
  ([ICE-AND-FIRE-AI-PLAN.md](ICE-AND-FIRE-AI-PLAN.md)). Horizon's route is `horizon-port-pack`, which runs on the
  player's computer from their own jar, keeps the result local, and now labels it "(unofficial
  port)" with the authors credited. A port today carries blocks, items, recipes, tags,
  language files, worldgen and textures. Its creatures need the rest of API 0.5
  (`docs/modpacks.md`, "Entities 0.5"):
  1. **Models of their own: done** (part 2). GeckoLib geometry and animations in a mod's
     `assets/`, `model` and `animations` (idle, walk, attack) in the entity's JSON. Checked in
     the game with a test mob: the model, its texture, idle and walk loops, death tip-over.
     **Ice and Fire's dragons are Tabula models (Citadel)**: `horizon-port-pack` converts them
     (`tabula.rs`): all 192 of Ice and Fire 2.0's models, and pose folders as animations. A
     converted fire dragon (red_4 texture, `tail` idle, `walk` loop) was checked in the game,
     locally only.
  2. **The `entities` interface: started** (server mods, `api = "0.3"`, experimental, API
     level 8): `mobs`, `near`, `give-orders` (walk-to, halt, look-at, push, gravity, animate,
     hurt-area, discard). Animations reach players' games on `horizon:entity_animation`. The
     example `crates/horizon-mod-sdk/examples/mob-orders` (`/heel`, `/animate`, `/breathe`,
     `/hover`) was checked in the game with the converted dragon, in-frame and on the server
     thread. Since then: `spawn` (with data), orders `target`, `scale` (growth, through
     `minecraft:scale`, which now sizes every mob as vanilla's does) and `set-data`, kept in the
     save with the playing animation; brain-driven templates follow walk and look orders.
     `interactions()` reads players' right clicks and held items; the example stores an
     `owner` UUID when clicked with a bone, without consuming it. Still to come: taming,
     riding, piglin brains.
     `horizon-ctl command` runs a command in the window, so it never reaches server mods:
     drive them with `horizon-ctl chat "/heel"`.
  3. Spawn rules and structures with their mobs.
  4. Block entities and screens (API 0.4), for machines such as forges.
- **Mod entities, part 1 ran in the game** (desktop, server-thread single player, with a
  local test mod: a zombie template with its own texture, `scale` 1.25, 60 health, fire
  immune). `/summon` (with and without NBT), the name, health, `@e[type=...]`, fire immunity,
  the texture and scale, save and reload, and its own death loot all work. The first run found
  the texture and scale ignored, loot looked up as `minecraft:entities/<ns>:<name>`, and
  vanilla-id tables (tracking, entity data, NBT, effects) missing mod ids: fixed with
  `mobs::vanilla_id` (a mod type's template). Not yet checked: the in-frame path and a LAN
  guest. `/loot ... loot <mod table>` still refuses mod tables (the command's id list).
  Loot tables in mods use 26.3's format (`modifier`/`type`, not `functions`/`function`).
- **Block Info:** entities (mobs and their health) need the planned `target` interface
  (`docs/mods.md`, "Planned interfaces").
- **Faster mods with a JIT:** with StikDebug attached, native cranelift would beat Pulley. Not
  done: it needs a check for whether JIT is allowed now, and on iOS 26 (TXM) likely
  wasmtime's `CustomCodeMemory`.
- **Modpack import in the launcher:** set aside by the player.

## Not done yet

- Mouse look with an iPad trackpad or mouse (they arrive as touches).
- LAN discovery by multicast needs Apple's multicast entitlement. Joining by address works.
- Discord presence, `horizon-ctl` (no meaning on iOS).
- Sign-out takes effect at the next start: `auth::set_current` is a `OnceLock`.
- Performance tuning for phones: the defaults are desktop ones (render distance 12).

## How to work on it

- **Type-check for iOS from Linux:** `ios/scripts/check-ios.sh`. It stubs a few SDK headers so
  `ring` builds. Linking needs Apple's SDK, so only a Mac or CI can produce the app. A full
  `cargo build --target aarch64-apple-ios` on Linux compiles every crate and fails only at the
  link step, which is a useful stricter check.
- **Desktop must stay green:** `cargo clippy -p horizon-client --all-targets` (and
  `--no-default-features`), `cargo fmt --all -- --check`, `python3 tools/check_public_tree.py`.
  With Rust 1.97, clippy flags pre-existing upstream code (`dragon.rs`, `noise_based.rs`
  `manual_range_contains`). Those aren't from the port.
- **Build the app:** push to the branch. The `ios` workflow (`.github/workflows/ios.yml`,
  `macos-15`) runs `ios/build.sh`, checks the bundle, and uploads the `.ipa`. About 9 minutes,
  plus queue time for Apple Silicon runners. On a Mac: `ios/build.sh` (see `--help`).
- **Watch a run:** `gh run list -R Worrcc/Horizon-iOS-worc -w ios -b <branch>` and
  `gh run watch <id>` work from the cloud session. A newer push cancels the running build.
- **Mods:** `cargo build -p cargo-horizon-mod`, then `cargo-horizon-mod horizon-mod build` and
  `... test` inside a mod's folder (needs the `wasm32-unknown-unknown` target).
- **Cloud-session limits found so far:** the proxy refuses `workflow_dispatch`, re-runs and
  repository-settings changes (403). A push is the only way to start a build. Artifact and log
  downloads from GitHub's blob storage are refused too, so the player downloads the `.ipa`
  themselves and logs come back as uploads.

### Where the iOS code lives

| Path | What |
|---|---|
| `crates/horizon-client/src/game/ios.rs` | `Session` (ends the run; `run_app` never returns on iOS), window attributes, background save, size checks, geometry logs, missing-files check |
| `crates/horizon-client/src/touch_controls.rs` | Touch logic, unit tested |
| `crates/horizon-client/src/game/touch.rs` | Applying and drawing touch controls; `MenuTouch` for the startup menu |
| `crates/horizon-client/src/game/account.rs`, `src/msa.rs` | Microsoft sign-in |
| `crates/horizon-client/src/game/pad/hardware_ios.rs` | Controllers: input, vibration, lightbar, adaptive triggers |
| `crates/horizon-client/src/instance_list.rs`, `game/instances.rs` | Instances |
| `crates/horizon-client/src/options_ui.rs` (`Page::Launcher`, `Page::Instances`), `game/startup_menu.rs` (`open_launcher`, `switch_instance`) | The launcher page |
| `crates/horizon-client/src/game/pad.rs` (`StartupPad`, `trigger_feels`) | Controllers on the startup title; trigger feel shared by both backends |
| `crates/horizon-mods/src/runtime.rs` (`engine`) | Pulley on iOS |
| `mods/official/block-info/` | The Block Info mod (`cargo horizon-mod build` / `test` in its folder) |
| `crates/horizon-mods/src/content.rs`, `src/mod_content.rs`, `src/mobs.rs` (`mod_types`), `src/hud.rs` (`sprite_name`) | Mod entities, part 1 |
| `crates/horizon-ios/` | Every UIKit, GameController, Keychain and Core Animation call (`unsafe` lives here; the game keeps `forbid(unsafe_code)`) |
| `third_party/winit/` | winit 0.30.13 patched for iOS (search "Horizon" in `src/platform_impl/ios/`): keyboard presses and characters, full-view sizes, iOS 16 orientation request |
| `ios/` | `build.sh`, `App/Info.plist`, icons, `scripts/check-ios.sh`, docs |

### Things that bit, so they don't again

- On iOS, `exit()` is ignored and `run_app` never returns: everything after the event loop has
  to run inside it (`ios::Session`).
- winit can deliver `about_to_wait` before `resumed`, so an alert can fire before any window
  exists. Alerts get their own `UIWindow` and retry until `presentedViewController` is set.
- The startup title screen is `Loading` (`game/startup_menu.rs`), not `Game`. Input changes
  have to cover both.
- winit on iOS makes the UIWindow exactly `inner_size` if one is given. Never pass a size on
  iOS (`platform_window`).
- Upstream checks `target_os = "macos"` in places where it means "Apple". Look for the same
  pattern when merging new upstream code (`gfx.rs` `backend_order` was one).
- LiveContainer runs apps in its own scene and orientation, and its Info.plist is the one iOS
  reads.
- The game reads its folder from the working directory (`options.txt`, `saves/`, packs, mods),
  so an instance is a `set_current_dir`. Anything built once at start from the old folder, like
  the title's world list, has to be rebuilt on a switch.
- Pulley in a debug build is too slow for the sandbox test's 5 s endless-loop cutoff. Test the
  runtime on Pulley in release: force it on Linux by widening the two `target_os = "ios"`
  cfgs in `horizon-mods` (Cargo.toml and `runtime.rs`), then put them back.
- The iOS stub SDK in `check-ios.sh` needed `stdlib.h` for wasmtime's C helpers.
- Linux builds with audio and controllers need `libasound2-dev libudev-dev`.

### Merging a newer Horizon release

```sh
git fetch upstream tag launcher@<version> --no-tags
git merge launcher@<version>
ios/scripts/check-ios.sh
```

If upstream bumps winit, re-apply the patch in `third_party/winit` to the new version, or drop it
if winit gained iOS key presses itself.
