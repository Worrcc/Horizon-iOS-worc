# Horizon-iOS: handoff

Where the iOS port stands, what was verified on a real device, and what to do next. Written for
whoever picks this up next, person or agent. [README.md](README.md) is the player-facing guide.

## The state of things

- **Branch:** `claude/minecraft-ios-port-w46z3k` in `Worrcc/Horizon-iOS-worc` (public). Upstream
  is Horizon `launcher@0.2.20` (`b6c936f82`) with its full history, plus the commits below. No
  pull request has been opened.
- **Latest build:** commit `22e327920`, CI run
  [41156361406](https://github.com/Worrcc/Horizon-iOS-worc/actions/runs/41156361406), succeeded.
  Artifact `Horizon-ios-22e3279…` (unsigned `Horizon.ipa`, no game files, kept 30 days).
- **Test device:** an iPhone with an A16 GPU (430×932 pt screen, ProMotion), running Horizon
  **inside LiveContainer**. Most open questions are about LiveContainer, not iOS itself.

### Commits on top of upstream

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

## Verified on the device

- Installs and launches under LiveContainer; finds `Documents/resources`.
- Metal renderer works (A16): the title screen and a world render with the real textures.
- The missing-files alert and fatal-error alerts show (they used to leave a black screen).
- Resource loading takes about 12 s on the A16 (`latest.log`: "resources loaded in 12.7s").

## Waiting on the player's test of the latest build

Each item has a fix in the latest build, but nobody has seen it working yet.

1. **Picture fills the screen.** The last log before `b2a931271` showed the cause:
   `window bounds 720x1280 frame 720x1280 at 0,-850; screen 430x932; scene orientation 1`.
   The UIWindow got the desktop 1280×720 size. `b2a931271` removes the size on iOS. If it's
   still wrong, `latest.log` now logs `view bounds / frame / window / screen / scene orientation`
   at startup and at every resize. Read it before changing anything.
2. **Title-screen buttons respond to touch** (`f44ba77ff`). The first bug report was "can't
   press any buttons". At that point the window was also off-screen, so the two may have been
   the same bug.
3. **120 FPS** (`22e327920`). Under LiveContainer, iOS reads `CADisableMinimumFrameDurationOnPhone`
   from **LiveContainer's** Info.plist, not Horizon's, so it may stay at 60 there. A standalone
   install (SideStore, AltStore, Sideloadly) is the real test.
4. **Orientation.** The scene was portrait (`orientation 1`). The game now follows the host's
   orientation (`LandscapeAndPortrait`). Landscape needs Rotation Lock off. A standalone install
   is landscape-only through its Info.plist.

## Blocked on the owner (not code)

- **Microsoft sign-in needs an approved Azure client ID.** Register an app (personal accounts,
  public client flows on), apply for Minecraft API access at https://aka.ms/mce-reviewappid
  (days to weeks), then add the repo **variable** `HORIZON_MSA_CLIENT_ID`. The `ios` workflow
  passes it to the build. Never use Horizon's or another launcher's ID (docs/microsoft-account.md).
  Until then, the Sign in button explains what's missing. The flow itself is untested against
  the real services. Its unit tests cover parsing only.

## Paused here (2026-10-31): resume from this section

- **Branch:** `claude/launcher-mods-shaders-controller-xfv3kk`, all work committed and pushed.
  No pull request opened.
- **Done:** instances (below). Type-checked for iOS (`ios/scripts/check-ios.sh`), unit tests
  pass (`cargo test -p horizon-client --no-default-features --lib instance_list`), clippy and fmt
  clean. **Not yet seen on a device:** the Instances button, New Instance, and a switch on
  the title reloading packs and mods.
- **Player's answers so far:** launcher in-app (Rust, before the game); code mods via wasmtime
  with JIT (the player uses StikDebug); controller work wanted: full menu navigation, touch
  controls hidden while a pad is connected, vibration/lights/triggers, remapping (already
  exists: Controller Settings > Controller Binds).
- **Open questions for the player:** launcher screen or controller work next? Modpacks: Horizon
  registry packs, or Modrinth/CurseForge Java packs (porting runs Java, so on a computer)?

## The in-app launcher (in progress)

Goal: manage instances, mods, shader packs and modpacks inside the app, and make controllers a
full input path. Step 1 is done: **instances** (`src/instance_list.rs`, `game/instances.rs`,
Options › Horizon Settings › Instances; `main.rs` starts in the picked folder). Next steps:
a launcher screen ahead of the title that brings instances, mods, shader packs and modpack import
together; controller-only menu navigation everywhere; hiding touch controls while a pad is
connected; Core Haptics vibration and DualSense lights and triggers. Button remapping already
exists (Controller Settings › Controller Binds). Code mods: the player has JIT through StikDebug,
so the plan is wasmtime with JIT where it is allowed, not an interpreter.

## Not done yet

- Controller vibration, DualSense lights and triggers (`game/pad/hardware_ios.rs` has stubs).
- Mouse look with an iPad trackpad or mouse (they arrive as touches).
- Code mods: wasmtime needs a JIT, which iOS forbids. An interpreter (wasmi) would be the route.
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
- **Watch a run:** `gh run list -R Worrcc/Horizon-iOS-worc -w ios` works from the cloud session.
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
| `crates/horizon-client/src/game/pad/hardware_ios.rs` | Controllers |
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

### Merging a newer Horizon release

```sh
git fetch upstream tag launcher@<version> --no-tags
git merge launcher@<version>
ios/scripts/check-ios.sh
```

If upstream bumps winit, re-apply the patch in `third_party/winit` to the new version, or drop it
if winit gained iOS key presses itself.
