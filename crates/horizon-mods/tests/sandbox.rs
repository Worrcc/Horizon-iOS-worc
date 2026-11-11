//! Loads real mods built with the SDK: the coords-hud and predator-missile
//! examples and a guest
//! that misbehaves on command. Needs `rustup target add wasm32-unknown-unknown`.
use horizon_mods::{
    BlockPos, Clock, Corner, GameMode, HeldItem, Hit, Mods, Output, Package, Player, Runs,
    ServerPlayer, Shape, Snapshot, Status,
    manifest::componentize,
    testing::{Script, TestGame},
};
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
    time::Instant,
};

const GUEST_MANIFEST: &str = r#"
id = "test_guest"
name = "Test Guest"
version = "0.1.0"
api = "0.1"
capabilities = ["chat", "storage"]
"#;
const UI_MANIFEST: &str = r#"
id = "test_ui_guest"
name = "Test UI Guest"
version = "0.1.0"
api = "0.1"
capabilities = ["game-read", "chat", "command", "ui", "render", "input", "recipes", "container"]
experimental = ["ui", "render", "input", "recipes", "container"]
"#;
const CANVAS_MANIFEST: &str = r#"
id = "test_canvas_guest"
name = "Test Canvas Guest"
version = "0.1.0"
api = "0.1"
capabilities = ["game-read", "ui", "render", "world-read", "input"]
experimental = ["ui", "render", "world-read", "input"]
"#;

/// Builds the mods once and writes their `.hmod` files; returns the folder.
fn mods_dir() -> &'static Path {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("target"))
            .join("mods-test");
        let status = Command::new(env!("CARGO"))
            .current_dir(&root)
            .args([
                "build",
                "--offline",
                "--release",
                "--target",
                "wasm32-unknown-unknown",
            ])
            .args([
                "-p",
                "coords-hud",
                "-p",
                "predator-missile",
                "-p",
                "welcome",
                "-p",
                "beacons",
                "-p",
                "mob-orders",
                "-p",
                "horizon-mods-test-guest",
                "-p",
                "horizon-mods-test-ui-guest",
                "-p",
                "horizon-mods-test-canvas-guest",
                "-p",
                "horizon-mods-test-api20-server-guest",
                "-p",
                "canvas-test",
                "--target-dir",
            ])
            .arg(&target)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "building the test mods needs `rustup target add wasm32-unknown-unknown`"
        );
        let built = target.join("wasm32-unknown-unknown/release");
        let dir = target.join("mods");
        std::fs::create_dir_all(&dir).unwrap();
        let example = |name: &str| {
            let path = format!("crates/horizon-mod-sdk/examples/{name}/horizon-mod.toml");
            std::fs::read_to_string(root.join(path)).unwrap()
        };
        let mods = [
            ("coords_hud", example("coords-hud")),
            ("predator_missile", example("predator-missile")),
            ("welcome", example("welcome")),
            ("beacons", example("beacons")),
            ("mob_orders", example("mob-orders")),
            ("horizon_mods_test_guest", GUEST_MANIFEST.to_string()),
            ("horizon_mods_test_ui_guest", UI_MANIFEST.to_string()),
            (
                "horizon_mods_test_canvas_guest",
                CANVAS_MANIFEST.to_string(),
            ),
            ("canvas_test", example("canvas-test")),
            (
                "horizon_mods_test_api20_server_guest",
                r#"
id = "test_api20_server"
name = "API20 Server"
version = "0.1.0"
api = "0.3"
side = "server"
capabilities = ["players", "world"]
experimental = ["players", "world"]
"#
                .into(),
            ),
        ];
        for (name, manifest) in mods {
            let core = std::fs::read(built.join(format!("{name}.wasm"))).unwrap();
            let hmod = Package::write(&manifest, &componentize(&core).unwrap()).unwrap();
            std::fs::write(dir.join(format!("{name}.hmod")), hmod).unwrap();
            if name == "horizon_mods_test_guest" {
                // This guest never calls chat.open: reproduce the pre-21 chat import contract.
                let (stripped, mut metadata) = wit_component::metadata::decode(&core).unwrap();
                for (_, interface) in metadata.resolve.interfaces.iter_mut() {
                    if interface.name.as_deref() == Some("chat") {
                        interface.functions.shift_remove("open");
                    }
                }
                let mut legacy = stripped.unwrap();
                wit_component::embed_component_metadata(
                    &mut legacy,
                    &metadata.resolve,
                    metadata.world,
                    wit_component::StringEncoding::UTF8,
                )
                .unwrap();
                let hmod = Package::write(&manifest, &componentize(&legacy).unwrap()).unwrap();
                std::fs::write(dir.join("legacy_chat.hmod"), hmod).unwrap();
            }
            if name == "horizon_mods_test_ui_guest" {
                // Actual older components: absent interface, and an interface
                // with only pointer. Resolve each optional callback independently.
                for (file, partial) in [
                    ("legacy_no_container", false),
                    ("legacy_container_pointer", true),
                ] {
                    let (stripped, mut metadata) = wit_component::metadata::decode(&core).unwrap();
                    if partial {
                        for (_, interface) in metadata.resolve.interfaces.iter_mut() {
                            if interface.name.as_deref() == Some("container-events") {
                                interface.functions.retain(|name, _| name == "pointer");
                            }
                        }
                    } else {
                        let interfaces = &metadata.resolve.interfaces;
                        metadata.resolve.worlds[metadata.world]
                            .exports
                            .retain(|_, item| {
                                !matches!(item, wit_parser::WorldItem::Interface {id, ..}
                                if interfaces[*id].name.as_deref() == Some("container-events"))
                            });
                    }
                    let mut legacy = stripped.unwrap();
                    wit_component::embed_component_metadata(
                        &mut legacy,
                        &metadata.resolve,
                        metadata.world,
                        wit_component::StringEncoding::UTF8,
                    )
                    .unwrap();
                    std::fs::write(
                        dir.join(format!("{file}.hmod")),
                        Package::write(&manifest, &componentize(&legacy).unwrap()).unwrap(),
                    )
                    .unwrap();
                }
            }
            if name == "horizon_mods_test_api20_server_guest" {
                let other = manifest
                    .replace("test_api20_server", "test_api20_other")
                    .replace("API20 Server", "API20 Other");
                let package = Package::write(&other, &componentize(&core).unwrap()).unwrap();
                std::fs::write(dir.join("api20_other.hmod"), package).unwrap();
            }
            if name == "horizon_mods_test_ui_guest" {
                let (stripped, mut metadata) = wit_component::metadata::decode(&core).unwrap();
                for (_, interface) in metadata.resolve.interfaces.iter_mut() {
                    if interface.name.as_deref() == Some("ui-events") {
                        interface.functions.retain(|name, _| {
                            matches!(name.as_str(), "select" | "closed" | "settings-opened")
                        });
                        interface.types.retain(|name, _| name != "pointer-kind");
                    }
                }
                let mut legacy = stripped.clone().unwrap();
                wit_component::embed_component_metadata(
                    &mut legacy,
                    &metadata.resolve,
                    metadata.world,
                    wit_component::StringEncoding::UTF8,
                )
                .unwrap();
                let hmod = Package::write(&manifest, &componentize(&legacy).unwrap()).unwrap();
                std::fs::write(dir.join("legacy_ui.hmod"), hmod).unwrap();
                let interfaces = &metadata.resolve.interfaces;
                metadata.resolve.worlds[metadata.world]
                    .exports
                    .retain(|_, item| {
                        !matches!(item, wit_parser::WorldItem::Interface { id, .. }
                        if interfaces[*id].name.as_deref() == Some("ui-events"))
                    });
                let mut legacy = stripped.unwrap();
                wit_component::embed_component_metadata(
                    &mut legacy,
                    &metadata.resolve,
                    metadata.world,
                    wit_component::StringEncoding::UTF8,
                )
                .unwrap();
                let hmod = Package::write(&manifest, &componentize(&legacy).unwrap()).unwrap();
                std::fs::write(dir.join("legacy_no_ui.hmod"), hmod).unwrap();
            }
        }
        dir
    })
}

fn load(name: &str) -> Mods {
    Mods::load(&[mods_dir().join(name)], |_| true, |c| c == "give")
}

fn player() -> Snapshot {
    Snapshot {
        player: Some(Player {
            x: 12.5,
            y: 64.0,
            z: -3.25,
            yaw: 180.0,
            pitch: 0.0,
            health: 20.0,
            food: 20,
            dimension: "minecraft:overworld".into(),
        }),
        clock: Some(Clock {
            game_time: 100,
            day_time: 24000 + 6000,
        }),
        gui_size: (427.0, 240.0),
        ..Snapshot::default()
    }
}

/// Capability `command`: `/missile` queues vanilla commands, and the chat
/// line its impact check prints sets off the warhead.
#[test]
fn predator_missile_runs_commands() {
    let mut mods = load("predator_missile.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    mods.set_snapshot(player());
    let commands = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Command { line } => Some(line),
                _ => None,
            })
            .collect()
    };
    assert!(mods.command("missile"));
    let launch = commands(&mut mods);
    assert_eq!(launch.len(), 6);
    assert!(launch.contains(&"gamemode spectator".to_string()));
    mods.tick(1);
    mods.tick(2);
    let flight = commands(&mut mods);
    assert!(flight.iter().any(|l| l.ends_with("run tp @s ^ ^ ^1.5")));
    mods.chat("Predator: impact");
    let blast = commands(&mut mods);
    assert_eq!(blast.len(), 4);
    assert!(blast.iter().all(|l| l.contains("summon tnt")));
}

#[test]
fn coords_hud_draws_and_answers() {
    let mut mods = load("coords_hud.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert_eq!(mods.commands().collect::<Vec<_>>(), ["where", "coords"]);
    mods.set_snapshot(player());
    mods.tick(1);
    let panel = mods.panels().next().expect("a panel after a tick");
    assert_eq!(panel.corner, Corner::TopLeft);
    let lines: Vec<&str> = panel.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        lines,
        [
            "XYZ 12.5 / 64.0 / -3.2",
            "Facing North (-Z)",
            "Day 2, 12:00"
        ]
    );

    assert!(mods.command("where"));
    let chat = Output::Chat {
        name: "Coordinates HUD".into(),
        text: "12, 64, -4 in minecraft:overworld".into(),
    };
    assert_eq!(mods.drain(), [chat]);
    assert!(mods.command("coords"));
    mods.tick(2);
    assert_eq!(mods.panels().count(), 0);
    let hidden = Output::Feedback {
        text: "Coordinates panel hidden".into(),
        error: false,
    };
    assert_eq!(mods.drain(), [hidden]);
    assert!(!mods.command("gamemode creative"));
}

#[test]
fn off_mods_cost_nothing() {
    let mods = Mods::load(&[mods_dir().join("coords_hud.hmod")], |_| false, |_| false);
    assert_eq!(mods.entries()[0].status, Status::Off);
    assert!(!mods.is_running());
    assert_eq!(mods.commands().count(), 0);
}

#[test]
fn undeclared_capabilities_are_refused() {
    let dir = mods_dir().join("undeclared");
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = Package::read(&mods_dir().join("horizon_mods_test_guest.hmod"))
        .unwrap()
        .wasm
        .expect("the guest has code");
    let manifest = GUEST_MANIFEST.replace("\"chat\", ", "");
    std::fs::write(dir.join("horizon-mod.toml"), manifest).unwrap();
    std::fs::write(dir.join("mod.wasm"), wasm).unwrap();
    let mods = Mods::load(&[dir], |_| true, |_| false);
    let refused = Status::Failed("uses capability chat, missing from horizon-mod.toml".into());
    assert_eq!(mods.entries()[0].status, refused);
}

/// A mod calling a function this Horizon doesn't have is refused with
/// "update Horizon", not a linking error; the test mods' levels are read
/// from the functions they import.
#[test]
fn a_mod_for_a_newer_horizon_is_refused() {
    use horizon_mods::api_level::{imports, needed};
    use wit_parser::{ManglingAndAbi, Resolve};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let wit = std::fs::read_to_string(root.join("crates/horizon-mod-sdk/wit/horizon.wit"))
        .unwrap()
        .replace(
            "    set-minimap: func(minimap: option<minimap>);",
            "    set-minimap: func(minimap: option<minimap>);\n    set-sky: func(on: bool);",
        );
    let mut resolve = Resolve::default();
    let package = resolve.push_str("horizon.wit", &wit).unwrap();
    let world = resolve
        .select_world(&[package], Some("horizon-ui-mod"))
        .unwrap();
    let mut module = wit_component::dummy_module(&resolve, world, ManglingAndAbi::Standard32);
    wit_component::embed_component_metadata(
        &mut module,
        &resolve,
        world,
        wit_component::StringEncoding::UTF8,
    )
    .unwrap();
    let wasm = wit_component::ComponentEncoder::default()
        .module(&module)
        .unwrap()
        .encode()
        .unwrap();
    let functions = imports(&wasm).unwrap();
    assert!(functions.iter().any(|f| f == "render.set-sky"));
    let dir = mods_dir().join("too_new");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = r#"
id = "too_new"
name = "Too New"
version = "0.1.0"
api = "0.1"
capabilities = ["game-read", "chat", "hud", "storage", "command", "ui", "render", "input", "recipes", "world-read", "container"]
experimental = ["ui", "render", "input", "recipes", "world-read", "container"]
"#;
    std::fs::write(dir.join("horizon-mod.toml"), manifest).unwrap();
    std::fs::write(dir.join("mod.wasm"), wasm).unwrap();
    let mods = Mods::load(&[dir], |_| true, |_| false);
    let refused = "made for a newer Horizon (it uses render.set-sky): update Horizon to run it";
    assert_eq!(mods.entries()[0].status, Status::Failed(refused.into()));

    let level = |file: &str| {
        let wasm = Package::read(&mods_dir().join(file)).unwrap().wasm.unwrap();
        needed(imports(&wasm).unwrap().iter().map(String::as_str)).0
    };
    assert_eq!(level("coords_hud.hmod"), 0);
    // The UI guest exercises the API 22 container calls.
    assert_eq!(level("horizon_mods_test_ui_guest.hmod"), 22);
    // canvas-test uses a placed box and a canvas screen (level 18).
    assert_eq!(level("canvas_test.hmod"), 18);
}

#[test]
fn vanilla_commands_win() {
    let mods = Mods::load(
        &[mods_dir().join("coords_hud.hmod")],
        |_| true,
        |c| c == "where",
    );
    assert_eq!(mods.commands().collect::<Vec<_>>(), ["coords"]);
}

#[test]
fn a_misbehaving_mod_is_stopped_alone() {
    for (command, reason) in [
        ("spin", "it took too long (an endless loop?)"),
        ("panic", "it crashed (a panic)"),
        ("grow", "it crashed (a panic)"),
    ] {
        let paths = [
            mods_dir().join("coords_hud.hmod"),
            mods_dir().join("horizon_mods_test_guest.hmod"),
        ];
        let mut mods = Mods::load(&paths, |_| true, |_| false);
        let started = Instant::now();
        assert!(mods.command(command));
        assert!(started.elapsed().as_secs() < 5, "/{command} was cut short");
        let stopped = Output::Stopped {
            name: "Test Guest".into(),
            reason: reason.into(),
        };
        assert_eq!(mods.drain(), [stopped], "/{command}");
        assert_eq!(mods.entries()[1].status, Status::Failed(reason.into()));
        // The other mod runs on, and the stopped one's command is gone.
        assert!(mods.is_running());
        assert!(!mods.command("hello"));
        mods.set_snapshot(player());
        mods.tick(1);
        assert_eq!(mods.panels().count(), 1);
    }
}

#[test]
fn hot_reload_restarts_a_changed_mod() {
    let dir = mods_dir().join("reload");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.hmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.hmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    mods.tick(1);
    assert!(!mods.reload_changed(|_| false));
    // A rebuild: same bytes, newer file.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(mods.reload_changed(|_| false));
    assert!(mods.command("hello world"));
    let chat = Output::Chat {
        name: "Test Guest".into(),
        text: "hello world after 0 ticks".into(),
    };
    assert_eq!(mods.drain(), [chat]);
}

#[test]
fn storage_persists_across_reload() {
    let dir = mods_dir().join("storage_reload");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.hmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.hmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    assert!(mods.command("save remembered"));
    let saved = Output::Feedback {
        text: "saved".into(),
        error: false,
    };
    assert_eq!(mods.drain(), [saved]);

    // A rebuild (hot_reload_restarts_a_changed_mod's pattern): a fresh Store,
    // but the same on-disk data directory.
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(later)
        .unwrap();
    assert!(mods.reload_changed(|_| false));

    assert!(mods.command("load"));
    let chat = Output::Chat {
        name: "Test Guest".into(),
        text: "remembered".into(),
    };
    assert_eq!(mods.drain(), [chat]);
}

#[test]
fn storage_write_budget_limits_one_call() {
    let dir = mods_dir().join("storage_budget");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.hmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.hmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    // Under the 4 MiB on-disk quota, but over the 1 MiB a single call may
    // write: a loop replacing one key can never bypass the quota by
    // writing a lot in one call, only across many.
    let over_budget = "a".repeat((1 << 20) + 1024);
    assert!(mods.command(&format!("save {over_budget}")));
    let refused = Output::Feedback {
        text: "too much storage traffic in one call".into(),
        error: true,
    };
    assert_eq!(mods.drain(), [refused]);
}

#[test]
fn storage_quota_is_enforced() {
    let dir = mods_dir().join("storage_quota");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.hmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.hmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    // Each call is under the per-call write budget; distinct keys so the
    // quota (not the fixed "note" key being overwritten) is what's tested.
    let chunk = "a".repeat(900 << 10);
    for i in 0..4 {
        assert!(mods.command(&format!("save_key k{i} {chunk}")));
        assert_eq!(
            mods.drain(),
            [Output::Feedback {
                text: "saved".into(),
                error: false,
            }]
        );
    }
    assert!(mods.command(&format!("save_key k4 {chunk}")));
    let refused = Output::Feedback {
        text: "would use over 4 MiB of storage".into(),
        error: true,
    };
    assert_eq!(mods.drain(), [refused]);
}

#[test]
fn storage_key_count_is_bounded() {
    let dir = mods_dir().join("storage_keys");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("guest.hmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.hmod"), &path).unwrap();
    let mut mods = Mods::load(std::slice::from_ref(&path), |_| true, |_| false);
    for i in 0..64 {
        assert!(mods.command(&format!("save_key k{i} x")));
        assert_eq!(
            mods.drain(),
            [Output::Feedback {
                text: "saved".into(),
                error: false,
            }],
            "key {i}"
        );
    }
    assert!(mods.command("save_key k64 x"));
    let refused = Output::Feedback {
        text: "would hold over 64 keys".into(),
        error: true,
    };
    assert_eq!(mods.drain(), [refused]);
}

#[test]
fn duplicate_mod_ids_are_refused() {
    let dir = mods_dir().join("duplicate_ids");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.hmod");
    let second = dir.join("second.hmod");
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.hmod"), &first).unwrap();
    std::fs::copy(mods_dir().join("horizon_mods_test_guest.hmod"), &second).unwrap();
    // Both declare id "test_guest" (the guest's own manifest): sharing a
    // `data_dir` would let the second read, overwrite or delete the
    // first's storage, so only the first-loaded one may start.
    let mods = Mods::load(&[first, second], |_| true, |_| false);
    assert_eq!(mods.entries()[0].status, Status::Running);
    let refused = Status::Failed("another running mod already uses id test_guest".into());
    assert_eq!(mods.entries()[1].status, refused);
}

/// `cargo test -p horizon-mods --release -- --ignored --nocapture`: the
/// cost of the per-tick and per-frame hooks (docs/mods.md, Performance).
#[test]
#[ignore]
fn hook_overhead() {
    let time = |mods: &mut Mods, frame: bool| {
        let n = 200_000;
        for i in 0..1000 {
            mods.tick(i);
        }
        let started = Instant::now();
        for i in 0..n {
            if frame {
                mods.frame(0.016);
            } else {
                mods.tick(i);
            }
        }
        started.elapsed().as_nanos() as f64 / n as f64
    };
    let mut guest = load("horizon_mods_test_guest.hmod");
    let mut hud = load("coords_hud.hmod");
    hud.set_snapshot(player());
    let mut none = Mods::default();
    println!("no mods, tick:        {:8.1} ns", time(&mut none, false));
    println!("empty tick hook:      {:8.1} ns", time(&mut guest, false));
    println!("empty frame hook:     {:8.1} ns", time(&mut guest, true));
    println!("coords-hud tick hook: {:8.1} ns", time(&mut hud, false));
    let started = Instant::now();
    let _ = load("coords_hud.hmod");
    println!(
        "load coords-hud:      {:8.1} ms",
        started.elapsed().as_secs_f64() * 1000.0
    );
}

#[test]
fn ui_screens_settings_and_events() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(!mods.entries()[0].has_ui_settings());
    // `/ui_screen` pushes the screen; the host clips nothing of this one.
    assert!(mods.command("ui_screen"));
    assert_eq!(mods.drain(), [Output::OpenScreen { owner: 0 }]);
    let screen = mods.screen(0).expect("a pushed screen");
    assert_eq!(screen.title, "Guest's screen");
    assert_eq!(screen.widgets.len(), 3);
    // What the player did comes back through `ui-events`.
    mods.ui_select(0, 7, 2);
    mods.ui_closed(0);
    assert_eq!(
        mods.drain(),
        [
            Output::Chat {
                name: "Test UI Guest".into(),
                text: "selected 7:2".into(),
            },
            Output::Chat {
                name: "Test UI Guest".into(),
                text: "closed".into(),
            },
        ]
    );
    // The settings page: empty until the mod fills it, refreshed on open.
    assert!(mods.command("ui_settings"));
    assert_eq!(mods.settings_widgets(0).len(), 3);
    assert!(mods.entries()[0].has_ui_settings());
    mods.ui_settings_opened(0);
    assert_eq!(mods.settings_widgets(0).len(), 2);
    // `/ui_close` closes the mod's own screen.
    assert!(mods.command("ui_close"));
    assert!(mods.drain().contains(&Output::CloseScreen { owner: 0 }));
    assert!(mods.screen(0).is_none());
}

#[test]
fn render_shapes_and_shakes() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    mods.set_snapshot(Snapshot {
        gui_size: (427.0, 240.0),
        ..Snapshot::default()
    });
    assert_eq!(mods.hud_shapes().count(), 0);
    assert!(mods.command("render_hud"));
    let shapes: Vec<_> = mods.hud_shapes().cloned().collect();
    assert_eq!(shapes.len(), 5);
    // The guest read `gui-size`; text is clipped to 256 characters.
    let Shape::Text(t) = &shapes[4] else {
        panic!("{:?}", shapes[4])
    };
    assert!(t.text.starts_with("gui 427x240 "), "{}", t.text);
    assert_eq!(t.text.chars().count(), 256);
    assert!(mods.command("render_clear"));
    assert_eq!(mods.hud_shapes().count(), 0);
    // One jolt, clamped; zero and NaN shake nothing.
    mods.drain();
    assert!(mods.command("render_shake"));
    assert_eq!(
        mods.drain(),
        [Output::Shake {
            strength: 1.0,
            seconds: 10.0
        }]
    );
}

#[test]
fn input_claims_keys_and_events() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let chat = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert!(!mods.claims_any() && !mods.has_keys());
    assert!(mods.command("input_claim"));
    let said = chat(&mut mods);
    assert!(
        said[0].starts_with("claimed; \"R\" is not a key"),
        "{said:?}"
    );
    assert!(mods.claims("minecraft:stick") && !mods.claims("minecraft:stone"));
    let keys: Vec<_> = mods
        .mod_keys()
        .map(|(m, k)| (m.to_owned(), k.key.clone()))
        .collect();
    assert_eq!(
        keys,
        [("Test UI Guest".to_owned(), "key.keyboard.r".to_owned())]
    );
    // The game feeds the events; the mod reads them in order with `poll`.
    let stick = HeldItem {
        id: "minecraft:stick".into(),
        count: 3,
        slot: 2,
    };
    mods.use_start(&stick);
    mods.key("key.keyboard.r", true);
    mods.key("key.keyboard.t", true);
    mods.key("key.keyboard.r", false);
    mods.use_stop(&stick, 12);
    assert!(mods.command("input_poll"));
    assert_eq!(
        chat(&mut mods),
        [
            "use-start minecraft:stick 0",
            "key-down wheel",
            "key-up wheel",
            "use-stop minecraft:stick 12"
        ]
    );
    // `held` reads the snapshot.
    mods.set_snapshot(Snapshot {
        held: Some(stick),
        ..Snapshot::default()
    });
    assert!(mods.command("input_held"));
    assert_eq!(chat(&mut mods), ["holding minecraft:stick x3 in 2"]);
    // A screen opening lets held keys up, once; the release that follows
    // is not sent again.
    mods.key("key.keyboard.r", true);
    mods.release_keys();
    mods.release_keys();
    mods.key("key.keyboard.r", false);
    assert!(mods.command("input_poll"));
    assert_eq!(chat(&mut mods), ["key-down wheel", "key-up wheel"]);
}

#[test]
fn the_players_key_choice_wins() {
    let chat = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    assert!(mods.command("input_claim"));
    // Its keys alone give it a page in Options > Mods, where the game lists them.
    assert!(mods.settings_widgets(0).is_empty() && mods.entries()[0].has_ui_settings());
    // Rebinding while held lets the old key go.
    mods.key("key.keyboard.r", true);
    assert!(mods.rebind(0, "wheel", "key.mouse.4"));
    assert_eq!(mods.keys_of(0)[0].key, "key.mouse.4");
    mods.key("key.keyboard.r", false);
    mods.key("key.mouse.4", true);
    assert!(!mods.rebind(0, "wheel", "R") && !mods.rebind(0, "nope", "key.keyboard.g"));
    chat(&mut mods);
    assert!(mods.command("input_poll"));
    assert_eq!(
        chat(&mut mods),
        ["key-down wheel", "key-up wheel", "key-down wheel"]
    );
    // A choice saved earlier replaces the default, registered before or after.
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    mods.set_bindings(|_| vec![("wheel", "key.keyboard.g"), ("bad", "G")]);
    assert!(mods.command("input_claim"));
    assert_eq!(mods.keys_of(0)[0].key, "key.keyboard.g");
    mods.set_bindings(|_| vec![("wheel", "key.keyboard.h")]);
    assert_eq!(mods.keys_of(0)[0].key, "key.keyboard.h");
}

#[test]
fn input_wheel_is_the_mods_while_claimed() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let chat = |mods: &mut Mods| -> Vec<String> {
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    // Unclaimed, the hotbar keeps the wheel and the mod hears nothing.
    assert!(!mods.wheel_claimed() && !mods.wheel(1.0));
    assert!(mods.command("input_wheel"));
    assert!(mods.wheel_claimed());
    assert!(mods.wheel(1.0) && mods.wheel(-2.0));
    assert!(mods.command("input_poll"));
    assert_eq!(chat(&mut mods), ["wheel 1", "wheel -2"]);
    assert!(mods.command("input_wheel off"));
    assert!(!mods.wheel(1.0));
}

#[test]
fn render_zoom_is_clamped_and_the_strongest_counts() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let zoom = |mods: &mut Mods, args: &str| {
        assert!(mods.command(&format!("zoom {args}")));
        mods.zoom().map(|z| (z.factor, z.smooth, z.turn))
    };
    assert!(mods.zoom().is_none());
    assert_eq!(zoom(&mut mods, "4 1 0.25"), Some((4.0, true, 0.25)));
    // Over 10 and a turn under 0.05 are clamped.
    assert_eq!(zoom(&mut mods, "50 0 0"), Some((10.0, false, 0.05)));
    // A NaN turn keeps the player's sensitivity.
    assert_eq!(zoom(&mut mods, "2 0 x"), Some((2.0, false, 1.0)));
    // 1, under 1 and NaN end the zoom.
    assert_eq!(zoom(&mut mods, "1 0 1"), None);
    assert_eq!(zoom(&mut mods, "0.5 0 1"), None);
    assert_eq!(zoom(&mut mods, "x 0 1"), None);
}

#[test]
fn render_minimap_is_clamped_and_hidden() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let map = |mods: &mut Mods, args: &str| {
        assert!(mods.command(&format!("minimap {args}")));
        mods.minimap()
            .map(|m| (m.corner, m.size, m.blocks_per_pixel, m.rotate, m.round))
    };
    assert!(mods.minimap().is_none());
    let corner = horizon_mods::RenderCorner::BottomLeft;
    assert_eq!(
        map(&mut mods, "96 2 1 0"),
        Some((corner, 96.0, 2.0, true, false))
    );
    // The side and the scale are clamped; NaN takes the smallest.
    assert_eq!(
        map(&mut mods, "999 0.1 0 1"),
        Some((corner, 256.0, 0.5, false, true))
    );
    assert_eq!(
        map(&mut mods, "x 9 0 0"),
        Some((corner, 16.0, 4.0, false, false))
    );
    assert_eq!(map(&mut mods, "off"), None);
}

#[test]
fn render_far_terrain_is_clamped_and_ended() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let far = |mods: &mut Mods, args: &str| {
        assert!(mods.command(&format!("far_terrain {args}")));
        mods.far_terrain().map(|f| (f.distance, f.reduced))
    };
    assert!(mods.far_terrain().is_none());
    assert_eq!(far(&mut mods, "256 1"), Some((256, true)));
    // The distance is clamped to LOD Distance's 64 to 1024 chunks.
    assert_eq!(far(&mut mods, "8 0"), Some((64, false)));
    assert_eq!(far(&mut mods, "99999 0"), Some((1024, false)));
    assert_eq!(far(&mut mods, "off"), None);
}

/// `game.raycast`: the view's hit the game worked out, up to the distance
/// asked for; the game works it out only while a mod imports the call.
#[test]
fn game_raycast_sees_the_view_hit_within_its_distance() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    assert!(mods.wants_raycast());
    assert!(!load("coords_hud.hmod").wants_raycast());
    let said = |mods: &mut Mods, args: &str| -> Vec<String> {
        assert!(mods.command(&format!("raycast {args}")));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    };
    assert_eq!(said(&mut mods, "100"), ["no hit"]);
    mods.set_snapshot(Snapshot {
        view_hit: Some((
            40.5,
            Hit {
                block_x: 12,
                block_y: 63,
                block_z: -44,
                x: 12.5,
                y: 64.0,
                z: -43.75,
                block: "minecraft:grass_block".into(),
            },
        )),
        ..player()
    });
    let hit = "hit minecraft:grass_block at 12 63 -44 (12.5 64.0 -43.8)";
    assert_eq!(said(&mut mods, "100"), [hit]);
    assert_eq!(said(&mut mods, "40"), ["no hit"]);
    // NaN asks for nothing; past 1024 is 1024.
    assert_eq!(said(&mut mods, "x"), ["no hit"]);
    assert_eq!(said(&mut mods, "1e9"), [hit]);
}

#[test]
fn quiet_commands_come_out_as_one_batch_capped() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let quiet = |mods: &mut Mods, command: &str| -> Vec<(String, Vec<String>)> {
        assert!(mods.command(command));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::QuietCommands { mod_id, lines } => Some((mod_id, lines)),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        quiet(&mut mods, "quiet setblock 0 0 0 stone;say hi"),
        [(
            "test_ui_guest".to_string(),
            vec!["setblock 0 0 0 stone".to_string(), "say hi".to_string()]
        )]
    );
    let batches = quiet(&mut mods, "quiet_many 300");
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].1.len(), 256);
    assert_eq!(batches[0].1[255], "setblock 255 300 0 stone");
}

#[test]
fn corner_panels_text_width_and_readings() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    // A font of 5-pixel glyphs, 3-pixel spaces.
    let mut widths = vec![5.0f32; 0x250];
    widths[' ' as usize] = 3.0;
    mods.set_snapshot(Snapshot {
        frame: (60, 41, 52),
        mappings: vec![("key.forward", "W".into(), true)],
        cps: (7, 2),
        font_widths: widths.into(),
        ..Snapshot::default()
    });
    assert!(mods.command("corner"));
    let panels: Vec<_> = mods.corner_panels().cloned().collect();
    assert_eq!(panels.len(), 1);
    // "60 fps": five glyphs and a space, plus the box's 4.
    assert_eq!(
        (panels[0].width, panels[0].height),
        (5.0 * 5.0 + 3.0 + 4.0, 12.0)
    );
    assert_eq!(panels[0].shapes.len(), 2);
    assert!(matches!(
        panels[0].corner,
        horizon_mods::RenderCorner::TopRight
    ));
    assert!(matches!(
        &mods.settings_widgets(0)[0],
        horizon_mods::UiWidget::Button(b) if b.id == 3 && b.label == "Corner: Top Right"
    ));
    mods.drain();
    assert!(mods.command("readings"));
    assert_eq!(
        mods.drain(),
        [Output::Chat {
            name: "Test UI Guest".into(),
            text: "60 41 52; forward W true; cps 7 2; other true".into(),
        }]
    );
}

#[test]
fn content_only_mods_load_without_an_engine() {
    let manifest = r#"
id = "ruby"
name = "Ruby"
version = "0.1.0"
api = "0.2"
side = "both"
capabilities = ["registries"]
"#;
    let block: &[u8] = br#"{"template": "minecraft:iron_block"}"#;
    let dir = mods_dir().join("content");
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("ruby.hmod");
    let files: &[(&str, &[u8])] = &[("data/ruby/horizon/block/ruby_block.json", block)];
    std::fs::write(&ok, Package::write_files(manifest, None, files).unwrap()).unwrap();
    let bad = dir.join("bad.hmod");
    let files: &[(&str, &[u8])] = &[("data/minecraft/recipe/stone.json", b"{}")];
    let bad_manifest = manifest.replace("\"ruby\"", "\"bad\"");
    std::fs::write(
        &bad,
        Package::write_files(&bad_manifest, None, files).unwrap(),
    )
    .unwrap();
    let mods = Mods::load(&[ok, bad], |_| true, |_| false);
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(matches!(&mods.entries()[1].status, Status::Failed(e) if e.contains("own namespace")));
    // Content has no hooks: the game's per-tick mod work stays skipped.
    assert!(!mods.is_running());
}

/// `cargo horizon-mod test` on the examples: their own `tests/*.toml` pass.
#[test]
fn the_examples_test_scripts_pass() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("../horizon-mod-sdk/examples");
    let mut ran = 0;
    let examples_and_files = [
        ("coords-hud", "coords_hud"),
        ("welcome", "welcome"),
        ("beacons", "beacons"),
        ("mob-orders", "mob_orders"),
        ("canvas-test", "canvas_test"),
    ];
    for (example, hmod) in examples_and_files {
        for entry in std::fs::read_dir(examples.join(example).join("tests")).unwrap() {
            let path = entry.unwrap().path();
            let script = Script::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let mut game = TestGame::load(&mods_dir().join(format!("{hmod}.hmod"))).unwrap();
            script
                .run(&mut game)
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            ran += 1;
        }
    }
    assert_eq!(ran, 7);
}

/// Capability `world` without a world lent (the registry's trial run):
/// `get-block` finds nothing, `set-blocks` checks its count and queues.
#[test]
fn a_server_mod_without_a_world_reads_nothing() {
    let mut game = TestGame::load(&mods_dir().join("beacons.hmod")).unwrap();
    game.join("Alex");
    let alex = game.players[0].clone();
    assert!(game.player_command(&alex, "ground"));
    let out = game.drain();
    assert!(
        out.iter()
            .any(|o| matches!(o, Output::Reply { text, error: true, .. }
            if text == "That block isn't loaded")),
        "{out:?}"
    );
}

#[test]
fn a_test_script_says_which_step_failed_and_why() {
    let script = Script::parse(
        r#"
        [[step]]
        ticks = 3
        command = "hello there"
        expect.chat = ["hello there after 2 ticks"]
        "#,
    )
    .unwrap();
    let mut game = TestGame::load(&mods_dir().join("horizon_mods_test_guest.hmod")).unwrap();
    assert_eq!(
        script.run(&mut game).unwrap_err(),
        "step 1 (3 ticks, /hello there): chat\n    expected: [\"hello there after 2 ticks\"]\n    \
         got:      [\"hello there after 3 ticks\"]"
    );
    assert!(
        Script::parse("[[step]]\ntick = 1").is_err(),
        "a typo is an error"
    );

    let mut game = TestGame::load(&mods_dir().join("welcome.hmod")).unwrap();
    let script = "[[step]]\njoin = \"Alex\"\n\n[[step]]\nas = \"Steve\"\ncommand = \"mined\"";
    assert_eq!(
        Script::parse(script).unwrap().run(&mut game).unwrap_err(),
        "step 2 (/mined): Steve is not on the server"
    );
}

#[test]
fn a_test_script_reloads_and_expects_a_stop() {
    let script = Script::parse(
        r#"
        [[step]]
        command = "save kept"
        expect.feedback = "saved"

        [[step]]
        reload = true
        command = "load"
        expect.chat = ["kept"]

        [[step]]
        command = "panic"
        expect.stopped = "panic"

        [[step]]
        reload = true
        command = "nope"
        "#,
    )
    .unwrap();
    let mut game = TestGame::load(&mods_dir().join("horizon_mods_test_guest.hmod")).unwrap();
    let dir = game.hmod().parent().unwrap().to_path_buf();
    assert_eq!(
        script.run(&mut game).unwrap_err(),
        "step 4 (reload, /nope): the mod owns no command /nope"
    );
    drop(game);
    assert!(
        !dir.exists(),
        "the scratch copy and its storage are removed"
    );
}

fn alex() -> ServerPlayer {
    ServerPlayer {
        id: "00000000-0000-4000-8000-000000000001".into(),
        name: "Alex".into(),
        x: 0.5,
        y: 64.0,
        z: 0.5,
        dimension: "minecraft:the_nether".into(),
        game_mode: GameMode::Survival,
        health: 20.0,
    }
}

#[test]
fn a_server_mod_runs_only_where_the_world_does() {
    // A fresh folder: what the mod stored in an earlier run is beside it.
    let dir = mods_dir().join("server");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("welcome.hmod");
    std::fs::copy(mods_dir().join("welcome.hmod"), &path).unwrap();
    let paths = std::slice::from_ref(&path);
    let window = Mods::load_for(Runs::Client, paths, |_| true, |_| false);
    assert_eq!(window.entries()[0].status, Status::Elsewhere);
    assert!(!window.is_running());

    let mut mods = Mods::load_for(Runs::Server, paths, |_| true, |c| c == "give");
    assert!(mods.runs_server());
    let commands: Vec<_> = mods.server_commands().map(|c| c.name.as_str()).collect();
    assert_eq!(commands, ["online", "mined"]);
    // `/mined [player]`: its argument travels with it, for the command tree.
    let mined = mods.server_commands().nth(1).unwrap();
    assert_eq!(mined.arguments[0].kind, horizon_mods::ArgumentKind::Player);
    assert!(mined.arguments[0].optional);
    assert_eq!(mods.commands().count(), 0, "not the window's chat commands");
    let alex = alex();
    mods.set_snapshot(Snapshot {
        players: vec![alex.clone()],
        ..Snapshot::default()
    });
    mods.player_joined(&alex);
    let greeted = [
        Output::Tell {
            name: "Welcome".into(),
            player: alex.id.clone(),
            text: "Welcome, Alex! 0 other player(s) online.".into(),
        },
        Output::Broadcast {
            name: "Welcome".into(),
            text: "Alex is here for the first time".into(),
        },
    ];
    assert_eq!(mods.drain(), greeted);
    let stone = BlockPos { x: 1, y: 63, z: 0 };
    mods.block_broken(&alex, stone, "minecraft:stone");
    assert!(mods.server_command(&alex, "mined"));
    assert!(mods.server_command(&alex, "online"));
    let reply = |text: &str| Output::Reply {
        player: alex.id.clone(),
        text: text.into(),
        error: false,
    };
    assert_eq!(
        mods.drain(),
        [reply("You broke 1 block(s)"), reply("Alex (the_nether)")]
    );
    assert!(!mods.server_command(&alex, "where"));
    assert!(
        !mods.command("online"),
        "a server command isn't the window's"
    );
}

#[test]
fn browser_reads_component_facts_and_other_recipe_files() {
    fn said(mods: &mut Mods, command: &str) -> Vec<String> {
        assert!(mods.command(command));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect()
    }

    use horizon_mods::{ItemFacts, OtherRecipe, Recipe, RecipeKind, RecipeStack, Recipes};
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    assert_eq!(
        said(&mut mods, "recipe_facts"),
        [
            "recipe count 0, all 0, count matches true, page matches true, empty edges true",
            "0 facts, 0 other recipes"
        ]
    );
    mods.set_snapshot(Snapshot {
        recipes: Some(std::sync::Arc::new(Recipes {
            recipes: vec![Recipe {
                id: "minecraft:stone".into(),
                kind: RecipeKind::CraftingShapeless,
                category: "building".into(),
                group: String::new(),
                width: 0,
                height: 0,
                slots: vec![vec![RecipeStack {
                    item: 1,
                    count: 1,
                    components: String::new(),
                }]],
                results: vec![RecipeStack {
                    item: 2,
                    count: 1,
                    components: String::new(),
                }],
                cooking_time: 0,
                experience: 0.0,
                book_id: 4,
            }],
            facts: vec![ItemFacts {
                item: 3,
                burn_ticks: 1600,
                compost_chance: 0.3,
                repaired_by: vec![1, 4],
            }],
            others: vec![OtherRecipe {
                id: "iceandfire:silver".into(),
                kind: "iceandfire:dragonforge".into(),
                json: "{\"type\":\"iceandfire:dragonforge\"}".into(),
            }],
            ..Recipes::default()
        })),
        ..Snapshot::default()
    });
    assert_eq!(
        said(&mut mods, "recipe_facts"),
        [
            "recipe count 1, all 1, count matches true, page matches true, empty edges true",
            "1 facts, 1 other recipes",
            "3: burn 1600, compost 0.3, repaired by [1, 4]",
            "iceandfire:silver: iceandfire:dragonforge {\"type\":\"iceandfire:dragonforge\"}",
        ]
    );
}

/// Capability `recipes`: the guest reads tables as big as vanilla's (1,500
/// items, 1,600 recipes of nine five-item slots) within one command's fuel.
#[test]
fn recipes_reads_the_tables() {
    use horizon_mods::{Recipe, RecipeItem, RecipeKind, RecipeStack, Recipes};
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let stack = |item| RecipeStack {
        item,
        count: 1,
        components: String::new(),
    };
    let items = (0..1500)
        .map(|i| RecipeItem {
            id: format!("minecraft:item_{i}"),
            name: format!("Item {i}"),
            tags: vec!["planks".into(), "logs".into()],
        })
        .collect();
    let mut recipes: Vec<Recipe> = (0..1600)
        .map(|i| Recipe {
            id: format!("minecraft:recipe_{i}"),
            kind: RecipeKind::CraftingShaped,
            category: "crafting_misc".into(),
            group: String::new(),
            width: 3,
            height: 3,
            slots: (0..9)
                .map(|k| (0..5).map(|j| stack((i + k + j) % 1500)).collect())
                .collect(),
            results: vec![RecipeStack {
                item: 7,
                count: 4,
                components: String::new(),
            }],
            cooking_time: 0,
            experience: 0.0,
            book_id: i as i32,
        })
        .collect();
    recipes[1].kind = RecipeKind::Brewing;
    recipes[1].results[0].components =
        "[minecraft:potion_contents={potion:\"minecraft:swiftness\"}]".into();
    let mods_recipes = Some(std::sync::Arc::new(Recipes {
        items,
        recipes,
        ..Recipes::default()
    }));
    mods.set_snapshot(Snapshot {
        recipes: mods_recipes.clone(),
        known_recipes: vec![1, 5, 9].into(),
        ..Snapshot::default()
    });
    let said = |mods: &mut Mods, command: &str| {
        assert!(mods.command(command));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        said(&mut mods, "recipes"),
        ["1500 items, 1600 recipes, 3 known, 1 brewing"]
    );
    assert_eq!(
        said(&mut mods, "recipes minecraft:recipe_1"),
        [
            "minecraft:recipe_1 Kind::Brewing 3x3 0t slots 9, the first 1 minecraft:item_1 -> \
          4 minecraft:item_7[minecraft:potion_contents={potion:\"minecraft:swiftness\"}] known true"
        ]
    );
    assert!(
        mods.entries()
            .iter()
            .all(|e| e.status == horizon_mods::Status::Running),
        "it ran out of fuel"
    );
    // Actions: refused without a crafting screen, for an unknown recipe,
    // and outside creative mode.
    assert_eq!(
        said(&mut mods, "recipe_place minecraft:recipe_5"),
        ["place minecraft:recipe_5: no screen that takes recipes is open"]
    );
    let mut snap = Snapshot {
        recipes: mods_recipes.clone(),
        known_recipes: vec![1, 5, 9].into(),
        placing: true,
        ..Snapshot::default()
    };
    mods.set_snapshot(snap.clone());
    assert_eq!(
        said(&mut mods, "recipe_place minecraft:recipe_6"),
        ["place minecraft:recipe_6: recipe 6 is not in the player's recipe book"]
    );
    assert!(mods.command("recipe_place minecraft:recipe_5 all"));
    assert_eq!(
        mods.drain()[0],
        Output::PlaceRecipe {
            book_id: 5,
            all: true
        }
    );
    assert_eq!(
        said(&mut mods, "recipe_give minecraft:item_7 3"),
        ["give minecraft:item_7: only in creative mode"]
    );
    snap.creative = true;
    mods.set_snapshot(snap);
    assert!(mods.command("recipe_give minecraft:item_7 3"));
    assert_eq!(
        mods.drain()[0],
        Output::Give {
            item: "minecraft:item_7".into(),
            count: 3,
            components: String::new()
        }
    );
    // Without the game's tables (no mod read them yet): empty, not a trap.
    mods.set_snapshot(Snapshot::default());
    assert_eq!(
        said(&mut mods, "recipes"),
        ["0 items, 0 recipes, 0 known, 0 brewing"]
    );
}

#[test]
fn browser_extension_keeps_empty_panels_and_delivers_bounded_bookmarks() {
    use horizon_mods::{BrowserClick, BrowserExtEvent};
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let command = |mods: &mut Mods, text: &str| {
        assert!(mods.command(text));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(mods.browser_items().is_none());
    command(&mut mods, "browser_keep_empty true");
    let (owner, _, items) = mods.browser_items().unwrap();
    assert!(items.is_empty());
    command(&mut mods, "bookmarks 1025");
    let (version, bookmarks) = mods.browser_bookmarks(owner).unwrap();
    assert_eq!(bookmarks.len(), 1024);
    assert_eq!(bookmarks[0].item, "é".repeat(256));
    assert_eq!(bookmarks[0].components, "界".repeat(4096));
    assert_eq!(
        (
            bookmarks[1].item.as_str(),
            bookmarks[1].count,
            bookmarks[1].components.as_str()
        ),
        ("minecraft:stone", 7, "[minecraft:damage=1]")
    );
    for index in 0..300 {
        mods.browser_ext_event(
            owner,
            BrowserExtEvent::BookmarkClicked(BrowserClick { index, right: true }),
        );
    }
    assert_eq!(
        command(&mut mods, "bookmarks_poll"),
        ["256 bookmark events", "bookmark 44 true"]
    );
    assert_eq!(command(&mut mods, "bookmarks_poll"), ["0 bookmark events"]);
    // Extension events do not change the old browser event contract.
    assert_eq!(command(&mut mods, "browser_poll"), ["0 events"]);
    command(&mut mods, "bookmarks");
    let (next, bookmarks) = mods.browser_bookmarks(owner).unwrap();
    assert_ne!(next, version);
    assert!(bookmarks.is_empty());
    command(&mut mods, "browser_keep_empty false");
    assert!(mods.browser_items().is_none());
    assert!(mods.browser_bookmarks(usize::MAX).is_none());
}

#[test]
fn browser_item_lines_are_bounded_replaced_and_cleared_per_panel() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let command = |mods: &mut Mods, text: &str| {
        assert!(mods.command(text));
        mods.drain();
    };
    command(&mut mods, "browser minecraft:stone minecraft:dirt");
    command(&mut mods, "bookmarks 2");
    let (owner, main_version, _) = mods.browser_items().unwrap();
    let bookmark_version = mods.browser_bookmarks(owner).unwrap().0;
    assert!(mods.browser_item_lines(owner).is_none());
    assert!(mods.browser_bookmark_lines(owner).is_none());
    assert!(mods.browser_item_lines(usize::MAX).is_none());
    assert!(mods.browser_bookmark_lines(usize::MAX).is_none());

    let long = "界".repeat(200);
    for bookmarks in [false, true] {
        command(
            &mut mods,
            &format!("item_lines {bookmarks} 2 {long}|1|2|3|4|5|6|7|8"),
        );
        let lines = if bookmarks {
            mods.browser_bookmark_lines(owner)
        } else {
            mods.browser_item_lines(owner)
        }
        .unwrap();
        assert_eq!(lines.len(), 2);
        for index in [0, 1] {
            assert_eq!(
                lines[&index],
                [
                    "界".repeat(128),
                    "1".into(),
                    "2".into(),
                    "3".into(),
                    "4".into(),
                    "5".into(),
                    "6".into(),
                    "7".into()
                ]
            );
        }
        if !bookmarks {
            assert_ne!(mods.browser_items().unwrap().1, main_version);
            assert_eq!(mods.browser_bookmarks(owner).unwrap().0, bookmark_version);
            assert!(mods.browser_bookmark_lines(owner).is_none());
        }
    }
    assert_ne!(mods.browser_bookmarks(owner).unwrap().0, bookmark_version);
    for bookmarks in [false, true] {
        let main_version = mods.browser_items().unwrap().1;
        let bookmark_version = mods.browser_bookmarks(owner).unwrap().0;
        command(
            &mut mods,
            &format!("item_lines {bookmarks} 1 replacement|tail"),
        );
        let lines = if bookmarks {
            assert_eq!(mods.browser_items().unwrap().1, main_version);
            assert_ne!(mods.browser_bookmarks(owner).unwrap().0, bookmark_version);
            mods.browser_bookmark_lines(owner)
        } else {
            assert_ne!(mods.browser_items().unwrap().1, main_version);
            assert_eq!(mods.browser_bookmarks(owner).unwrap().0, bookmark_version);
            mods.browser_item_lines(owner)
        }
        .unwrap();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[&0], ["replacement", "tail"]);
    }
    command(&mut mods, "browser minecraft:stone");
    assert!(mods.browser_item_lines(owner).is_none());
    assert_eq!(
        mods.browser_bookmark_lines(owner).unwrap()[&0],
        ["replacement", "tail"]
    );
    command(&mut mods, "item_lines false 8193 capped");
    let lines = mods.browser_item_lines(owner).unwrap();
    assert_eq!(lines.len(), 8192);
    assert_eq!(lines[&8191], ["capped"]);
    assert!(!lines.contains_key(&8192));
    command(&mut mods, "bookmarks 1");
    assert!(mods.browser_bookmark_lines(owner).is_none());
    assert_eq!(mods.browser_item_lines(owner).unwrap().len(), 8192);
    command(&mut mods, "item_lines false 0");
    assert!(mods.browser_item_lines(owner).is_none());

    let untouched = load("horizon_mods_test_guest.hmod");
    assert!(untouched.browser_item_lines(0).is_none());
    assert!(untouched.browser_bookmark_lines(0).is_none());
}

#[test]
fn browser_custom_recipes_clip_geometry_and_mix_with_standard_pages() {
    use horizon_mods::BrowserLayout;
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let command = |mods: &mut Mods, arg: &str| {
        assert!(mods.command(&format!("custom_recipes {arg}")));
        mods.drain();
    };
    command(&mut mods, "mixed");
    let (owner, version, _) = mods.browser_items().unwrap();
    let p = mods.browser_recipes(owner).unwrap();
    assert_eq!((p.title.as_str(), p.tabs.len(), p.tab), ("custom", 16, 3));
    assert_eq!((p.recipes.len(), p.custom.len()), (2, 2));
    let c = p.custom[0].as_ref().unwrap();
    assert_eq!((c.width, c.height), (160, 120));
    assert_eq!(c.slots, vec![(0, 119); 9]);
    assert_eq!(c.result, Some((159, 0)));
    assert_eq!(c.texts, vec![(159, 0, "界".repeat(256), 0x123456); 16]);
    assert_eq!(
        c.sprites,
        vec![(0, 119, "container/furnace/lit_progress".into()); 16]
    );
    let r = &p.recipes[0];
    assert_eq!(
        (r.layout, r.width, r.height, r.action),
        (BrowserLayout::Single, 1, 1, Some(true))
    );
    assert!(r.note.is_empty());
    assert_eq!(r.slots.len(), 9);
    assert!(
        r.slots
            .iter()
            .all(|s| s.len() == 64 && s[0].item == "minecraft:stone")
    );
    assert_eq!(r.results.len(), 1);
    assert!(p.custom[1].is_none());
    assert_eq!(
        (
            p.recipes[1].width,
            p.recipes[1].height,
            p.recipes[1].slots.len()
        ),
        (3, 1, 9)
    );
    command(&mut mods, "no_result");
    let p = mods.browser_recipes(owner).unwrap();
    assert!(p.custom[0].as_ref().unwrap().result.is_none());
    assert!(p.recipes[0].results.is_empty());
    assert_ne!(mods.browser_items().unwrap().1, version);
    command(&mut mods, "standard");
    let p = mods.browser_recipes(owner).unwrap();
    assert_eq!(p.title, "standard");
    assert_eq!(p.recipes.len(), 1);
    assert_eq!(p.custom, [None]);
    command(&mut mods, "empty");
    assert!(mods.browser_recipes(owner).is_none());
}

#[test]
fn browser_lists_items_and_hears_the_player() {
    use horizon_mods::{BrowserClick, BrowserEvent, BrowserStack};
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let said = |mods: &mut Mods, command: &str| {
        assert!(mods.command(command));
        mods.drain()
            .into_iter()
            .filter_map(|o| match o {
                Output::Chat { text, .. } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert!(mods.browser_items().is_none());
    let potion = "[minecraft:potion_contents={potion:\"minecraft:swiftness\"}]";
    assert_eq!(
        said(
            &mut mods,
            &format!("browser minecraft:stone minecraft:potion{potion}")
        ),
        ["panel of 2"]
    );
    let (owner, version, items) = mods.browser_items().unwrap();
    assert_eq!(
        (items[0].item.as_str(), items[1].components.as_str()),
        ("minecraft:stone", potion)
    );
    mods.browser_event(
        owner,
        BrowserEvent::Clicked(BrowserClick {
            index: 1,
            right: true,
        }),
    );
    mods.browser_event(owner, BrowserEvent::Search("oak".into()));
    let log = BrowserStack {
        item: "minecraft:oak_log".into(),
        count: 3,
        components: String::new(),
    };
    // R is the guest's `recipe`; X is none of its keys.
    mods.browser_key("key.keyboard.r", &log);
    mods.browser_key("key.keyboard.x", &log);
    assert_eq!(
        said(&mut mods, "browser_poll"),
        [
            "3 events",
            "clicked 1 right",
            "search \"oak\"",
            "key recipe on minecraft:oak_log x3"
        ]
    );
    // Each `set-items` is a new version; an empty list hides the panel.
    said(&mut mods, "browser minecraft:dirt");
    assert_ne!(mods.browser_items().unwrap().1, version);
    assert_eq!(said(&mut mods, "browser"), ["panel of 0"]);
    assert!(mods.browser_items().is_none());
    // Recipes in the panel's place: the guest shows those making planks.
    use horizon_mods::{BrowserLayout, Recipe, RecipeItem, RecipeKind, RecipeStack, Recipes};
    let item = |id: &str| RecipeItem {
        id: id.into(),
        name: id.into(),
        tags: Vec::new(),
    };
    let stack = |item| RecipeStack {
        item,
        count: 1,
        components: String::new(),
    };
    mods.set_snapshot(Snapshot {
        recipes: Some(std::sync::Arc::new(Recipes {
            items: vec![item("minecraft:oak_log"), item("minecraft:oak_planks")],
            recipes: vec![Recipe {
                id: "minecraft:oak_planks".into(),
                kind: RecipeKind::CraftingShapeless,
                category: "building".into(),
                group: "planks".into(),
                width: 0,
                height: 0,
                slots: vec![vec![stack(0)]],
                results: vec![RecipeStack {
                    count: 4,
                    ..stack(1)
                }],
                cooking_time: 0,
                experience: 0.0,
                book_id: 1,
            }],
            ..Recipes::default()
        })),
        ..Snapshot::default()
    });
    said(&mut mods, "browser minecraft:dirt");
    assert_eq!(
        said(&mut mods, "browser_recipes minecraft:oak_planks"),
        ["recipes shown: 1"]
    );
    let page = mods.browser_recipes(owner).unwrap();
    assert_eq!(page.title, "Recipes for minecraft:oak_planks");
    assert_eq!(
        (page.tabs[0].item.as_str(), page.tab),
        ("minecraft:crafting_table", 0)
    );
    let r = &page.recipes[0];
    assert_eq!(
        (r.layout, r.width, r.height, r.action),
        (BrowserLayout::Grid, 1, 1, Some(false))
    );
    assert_eq!(
        (r.slots[0][0].item.as_str(), r.results[0].count),
        ("minecraft:oak_log", 4)
    );
    assert_eq!(said(&mut mods, "browser_recipes"), ["recipes shown: 0"]);
    assert!(mods.browser_recipes(owner).is_none());
    // At most 256 events wait for a poll: the oldest go.
    for _ in 0..300 {
        mods.browser_event(owner, BrowserEvent::Page(1));
    }
    assert_eq!(said(&mut mods, "browser_poll")[0], "256 events");
}

#[test]
fn mob_orders_consumes_food_once_labels_sneak_clicks_and_targets_a_uuid() {
    use horizon_mods::{
        EntityInteraction, EntityOrder, EntityTarget, ModEntity, testing::InventoryFixture,
    };
    let mut mods = load("mob_orders.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    let p = ServerPlayer {
        id: "00000000-0000-4000-8000-000000000001".into(),
        name: "Alex".into(),
        x: 0.0,
        y: 64.0,
        z: 0.0,
        dimension: "minecraft:overworld".into(),
        game_mode: GameMode::Survival,
        health: 20.0,
    };
    let mut fixture = InventoryFixture {
        players: [(p.id.clone(), false)].into(),
        ..Default::default()
    };
    fixture
        .counts
        .lock()
        .unwrap()
        .insert((p.id.clone(), "minecraft:beef".into()), 2);
    mods.lend_inventories(Box::new(fixture.clone()));
    let click = |item: &str, sneaking, hand: &str| EntityInteraction {
        player: p.id.clone(),
        mob: 7,
        kind: "ruby:dragon".into(),
        item: Some(item.into()),
        sneaking,
        hand: hand.into(),
    };
    let mut snap = Snapshot {
        players: vec![p.clone()],
        entities: vec![ModEntity {
            id: 7,
            kind: "ruby:dragon".into(),
            dimension: p.dimension.clone(),
            x: 3.0,
            y: 64.0,
            z: 0.0,
            yaw: 0.0,
            health: 10.0,
            max_health: 20.0,
            on_ground: true,
            target: None,
            target_player: false,
            scale: 1.0,
            data: vec![],
            age: 0,
            animation: None,
            texture: None,
            rider: None,
        }]
        .into(),
        ..Default::default()
    };
    snap.interactions = vec![click("minecraft:bone", false, "main_hand")].into();
    mods.set_snapshot(snap.clone());
    mods.server_tick(0);
    assert!(
        !mods
            .drain()
            .iter()
            .any(|o| matches!(o, Output::Orders { .. }))
    );
    snap.interactions = vec![click("minecraft:bone", true, "off_hand")].into();
    mods.set_snapshot(snap.clone());
    mods.server_tick(1);
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o, Output::Orders { orders, .. }
        if orders.iter().any(|(id, o)| *id == 7 && matches!(o, EntityOrder::SetData(entries)
            if entries.contains(&("owner".into(), Some(p.id.clone())))
            && entries.contains(&("tamed".into(), Some("true".into())))))))
    );
    let heals = |out: Vec<Output>| {
        out.into_iter()
            .filter_map(|o| match o {
                Output::Orders { orders, .. } => Some(
                    orders
                        .into_iter()
                        .filter(|(id, o)| *id == 7 && matches!(o, EntityOrder::Heal(4.0)))
                        .count(),
                ),
                _ => None,
            })
            .sum::<usize>()
    };
    snap.interactions = vec![
        click("minecraft:beef", false, "main_hand"),
        click("minecraft:beef", false, "off_hand"),
    ]
    .into();
    mods.set_snapshot(snap.clone());
    mods.server_tick(2);
    assert_eq!(heals(mods.drain()), 2);
    assert_eq!(
        fixture.counts.lock().unwrap()[&(p.id.clone(), "minecraft:beef".into())],
        0
    );
    mods.server_tick(3);
    assert_eq!(heals(mods.drain()), 0);
    fixture.players.insert(p.id.clone(), true);
    mods.lend_inventories(Box::new(fixture.clone()));
    snap.interactions = vec![click("minecraft:beef", false, "off_hand")].into();
    mods.set_snapshot(snap.clone());
    mods.server_tick(4);
    assert_eq!(heals(mods.drain()), 1);
    assert_eq!(
        fixture.counts.lock().unwrap()[&(p.id.clone(), "minecraft:beef".into())],
        0
    );
    assert!(mods.server_command(&p, "gift ruby:scale 3"));
    assert_eq!(
        fixture.counts.lock().unwrap()[&(p.id.clone(), "ruby:scale".into())],
        3
    );
    mods.drain();
    assert!(mods.server_command(&p, "attackme"));
    assert!(mods.drain().iter().any(|o| matches!(o, Output::Orders { orders, .. }
        if orders.iter().any(|(id, o)| *id == 7 && matches!(o, EntityOrder::Target(EntityTarget::Player(id)) if id == &p.id)))));
    snap.item_uses = vec![horizon_mods::PlayerItemUse {
        player: p.id.clone(),
        item: "minecraft:stick".into(),
        hand: "off_hand".into(),
        sneaking: true,
        target: Some(horizon_mods::PlayerBlockHit {
            x: 1,
            y: 64,
            z: 2,
            face: "up".into(),
        }),
    }]
    .into();
    snap.effect_ids = std::sync::Arc::new(["minecraft:wither".into()].into());
    mods.set_snapshot(snap);
    assert!(mods.server_command(&p, "uses"));
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o, Output::Reply { text, error: false, .. }
        if text == &format!("{}: minecraft:stick (off_hand, sneaking true) at 1 64 2 up", p.id)))
    );
    assert!(mods.server_command(&p, "burn"));
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o, Output::Orders { orders, .. }
        if orders.iter().any(|(id, o)| *id == 7 && matches!(o, EntityOrder::Effect(effect)
            if effect.id == "minecraft:wither" && effect.seconds == 10 && effect.amplifier == 0))))
    );
    struct Chest;
    impl horizon_mods::WorldView for Chest {
        fn block(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            None
        }
        fn has_dimension(&self, _: &str) -> bool {
            true
        }
        fn check_state(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn container(
            &self,
            _: &str,
            x: i32,
            y: i32,
            z: i32,
        ) -> Option<Vec<Option<horizon_mods::ContainerItemStack>>> {
            ((x, y, z) == (1, 64, 2)).then(|| {
                vec![
                    None,
                    Some(horizon_mods::ContainerItemStack {
                        item: "minecraft:stick".into(),
                        count: 3,
                    }),
                ]
            })
        }
    }
    mods.lend_world(Box::new(Chest));
    assert!(mods.server_command(&p, "peek 1 64 2"));
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o, Output::Reply { text, error: false, .. }
        if text == "2 slots: 1: minecraft:stick x3"))
    );
    assert_eq!(mods.entries()[0].status, Status::Running);
}

/// The command's feedback, or its error.
fn answer(game: &mut TestGame, line: &str) -> Result<String, String> {
    assert!(game.command(line), "/{line}");
    let out = game.drain();
    out.into_iter()
        .rev()
        .find_map(|o| match o {
            Output::Feedback { text, error } => Some(if error { Err(text) } else { Ok(text) }),
            Output::Stopped { reason, .. } => panic!("stopped: {reason}"),
            _ => None,
        })
        .unwrap_or_else(|| panic!("/{line} said nothing"))
}

fn canvas_guest() -> TestGame {
    TestGame::load(&mods_dir().join("horizon_mods_test_canvas_guest.hmod")).unwrap()
}

/// Canvas images: sides and the mod's share are checked, updates are
/// checked and capped per frame, and the texels go up once a frame.
#[test]
fn canvas_images_are_checked_capped_and_uploaded() {
    let mut game = canvas_guest();
    for bad in ["image 0 4", "image 1025 1"] {
        let e = answer(&mut game, bad).unwrap_err();
        assert!(e.contains("1 to 1024 texels a side"), "{e}");
    }
    assert_eq!(
        answer(&mut game, "image 1024 1024").as_deref(),
        Ok("image 1")
    );
    let e = answer(&mut game, "image 1 1").unwrap_err();
    assert!(e.contains("at most 1048576 texels"), "{e}");
    answer(&mut game, "drop 1").unwrap();
    assert_eq!(answer(&mut game, "image 64 32").as_deref(), Ok("image 2"));
    // A new image goes up whole and clear, before anything is drawn on it.
    let first = game.canvas_uploads();
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].width, first[0].height), (64, 32));
    assert!(first[0].rgba.iter().all(|&b| b == 0));
    assert!(game.canvas_uploads().is_empty());
    answer(&mut game, "update 2 4 2 3 2").unwrap();
    let up = game.canvas_uploads();
    assert_eq!(up.len(), 1);
    assert_eq!((up[0].width, up[0].height), (3, 2));
    assert_eq!(&up[0].rgba[..4], &[255, 0, 0, 255]);
    for bad in ["update 2 62 0 3 1", "update 2 0 31 1 2", "update 9 0 0 1 1"] {
        assert!(answer(&mut game, bad).is_err(), "{bad}");
    }
    let e = answer(&mut game, "update_short 2").unwrap_err();
    assert!(e.contains("16 bytes, not 15"), "{e}");
    // 8 KiB a call: 128 fit in a frame's 1 MiB, the 129th waits a frame.
    for _ in 0..128 {
        answer(&mut game, "update 2 0 0 64 32").unwrap();
    }
    let e = answer(&mut game, "update 2 0 0 64 32").unwrap_err();
    assert!(e.contains("over 1024 KiB"), "{e}");
    assert_eq!(game.canvas_uploads().len(), 1);
    answer(&mut game, "update 2 0 0 64 32").unwrap();
    // The box draws image 2; an image dropped draws nothing.
    answer(&mut game, "box 2").unwrap();
    let panel = game.canvas().unwrap();
    assert_eq!((panel.width, panel.height), (32.0, 32.0));
    assert_eq!(game.status(), &Status::Running);
}

/// Two mods' images share the page without overlapping, and a mod's go
/// when it stops.
#[test]
fn canvas_images_of_two_mods_share_the_page() {
    let mut mods = Mods::load(
        &[
            mods_dir().join("canvas_test.hmod"),
            mods_dir().join("horizon_mods_test_canvas_guest.hmod"),
        ],
        |_| true,
        |_| false,
    );
    assert!(mods.uses_canvas());
    mods.set_snapshot(player());
    // canvas_test made its 64 × 64 gradient in `init`.
    let (owner, panel) = mods.canvas_panels().next().unwrap();
    let Some(horizon_mods::CanvasShape::Image(shape)) = panel.shapes.get(1).cloned() else {
        panic!("{:?}", panel.shapes)
    };
    let gradient = mods.canvas_image(owner, shape.image).unwrap();
    assert!(mods.command("image 100 20"));
    let guest = mods.canvas_image(1, 1).unwrap();
    let overlap = gradient.x < guest.x + guest.width
        && guest.x < gradient.x + gradient.width
        && gradient.y < guest.y + guest.height
        && guest.y < gradient.y + gradient.height;
    assert!(!overlap, "{gradient:?} {guest:?}");
    // The gradient's texels: red across, green down.
    let up = mods.take_canvas_uploads(false);
    let g = up
        .iter()
        .find(|u| u.x == gradient.x && u.y == gradient.y)
        .unwrap();
    let px = |x: usize, y: usize| &g.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
    assert_eq!(px(0, 0), &[0, 0, 255, 255]);
    assert_eq!(px(10, 5), &[40, 20, 235, 255]);
    // After an atlas rebuild, every image goes up again whole.
    assert!(mods.take_canvas_uploads(false).is_empty());
    assert_eq!(mods.take_canvas_uploads(true).len(), 2);
    assert_eq!(mods.frame_budget(0), Some(horizon_mods::FRAME_BUDGET));
}

/// `game.world-id` and `client-world`'s reads: counts and depths checked,
/// the view's chunks and changes as lent.
#[test]
fn client_world_reads_are_checked() {
    let mut game = canvas_guest();
    assert_eq!(
        answer(&mut game, "world_id").as_deref(),
        Ok("Some(\"sp:Test World\")")
    );
    let e = answer(&mut game, "columns 17 1").unwrap_err();
    assert!(e.contains("at most 16 chunks"), "{e}");
    let e = answer(&mut game, "columns 2 0").unwrap_err();
    assert!(e.contains("depth is 1 to 16"), "{e}");
    assert_eq!(
        answer(&mut game, "columns 3 16").as_deref(),
        Ok("3 chunks, 3 loaded")
    );
    game.set_block(20, 64, 5, "minecraft:stone");
    game.set_block(-200, 64, 5, "minecraft:stone");
    assert_eq!(
        answer(&mut game, "table").as_deref(),
        Ok("2 states, 2 blocks, 2 sprites")
    );
    assert_eq!(
        answer(&mut game, "texture block/stone").as_deref(),
        Ok("none")
    );
    // Only the chunk within 4 of the player's; the far one is past it.
    assert_eq!(
        answer(&mut game, "changes 0 4").as_deref(),
        Ok("now 3: [(1, 0)]")
    );
    assert_eq!(answer(&mut game, "changes 3 4").as_deref(), Ok("now 3: []"));
}

/// The frame budget: asked for and clamped, and `work-left` keeps a busy
/// `frame` hook to it once the rate is measured. Timing, so run alone and
/// in release (`--ignored`; with `--features force-pulley` as on iOS).
#[test]
#[ignore]
fn the_frame_budget_converges() {
    let mut game = canvas_guest();
    assert_eq!(
        answer(&mut game, "budget 9999").as_deref(),
        Ok("granted 500")
    );
    assert_eq!(
        answer(&mut game, "budget 300").as_deref(),
        Ok("granted 300")
    );
    answer(&mut game, "work on").unwrap();
    let mut times = Vec::new();
    for _ in 0..400 {
        let started = Instant::now();
        game.frames(1);
        times.push(started.elapsed().as_secs_f64() * 1e6);
    }
    let mut last: Vec<f64> = times[200..].to_vec();
    last.sort_by(f64::total_cmp);
    let median = last[last.len() / 2];
    println!(
        "frame call, µs: first {:.0}, tenth {:.0}, median of the last 200 {median:.0}",
        times[0], times[9]
    );
    assert!((240.0..=360.0).contains(&median), "{median} µs");
    assert_eq!(game.status(), &Status::Running);
}

#[test]
fn canvas_screen_budget_tracks_close_reopen_and_thermal_pressure() {
    let mut mods = Mods::default();
    mods.limit_canvas_screen_budget(true);
    mods.load_into(
        Runs::Both,
        &[mods_dir().join("horizon_mods_test_canvas_guest.hmod")],
        |_| true,
        |_| false,
    );
    assert!(mods.command("budget 9999"));
    assert!(mods.drain().iter().any(|o| matches!(o,
        Output::Feedback { text, error: false } if text == "granted 500"
    )));
    assert!(mods.command("screen 1"));
    assert_eq!(mods.frame_budget(0), Some(500));
    mods.limit_canvas_screen_budget(false);
    assert_eq!(mods.frame_budget(0), Some(4000));
    assert!(mods.command("budget 9999"));
    assert!(mods.drain().iter().any(|o| matches!(o,
        Output::Feedback { text, error: false } if text == "granted 4000"
    )));
    assert!(mods.command("work on"));
    // Exercise cooperative work at the larger grant, including rate updates.
    for _ in 0..64 {
        mods.frame(1.0 / 60.0);
    }
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.command("screen_close"));
    assert_eq!(mods.frame_budget(0), Some(500));
    assert!(mods.command("screen 1"));
    assert_eq!(mods.frame_budget(0), Some(4000));
    mods.ui_closed(0); // The game uses this for both Esc and Done.
    assert_eq!(mods.frame_budget(0), Some(500));
    assert!(mods.command("screen 1"));
    assert_eq!(mods.frame_budget(0), Some(4000));
    mods.limit_canvas_screen_budget(true);
    assert_eq!(mods.frame_budget(0), Some(500));
    mods.limit_canvas_screen_budget(false);
    assert_eq!(mods.frame_budget(0), Some(4000));
    assert!(mods.command("budget 300"));
    assert!(mods.command("screen_close"));
    assert!(mods.command("screen 1"));
    assert_eq!(mods.frame_budget(0), Some(300));
}

/// Placed boxes replace corner boxes, retain offsets through a resize and
/// clamp independently of the overlay stack, including non-finite inputs.
#[test]
fn canvas_placed_boxes_replace_and_clamp() {
    let mut game = canvas_guest();
    answer(&mut game, "placed 0 12 20 32 24 1100").unwrap();
    let p = game.canvas().unwrap();
    assert_eq!(p.placed, Some((12.0, 20.0)));
    assert_eq!(p.shapes.len(), 1024);
    assert_eq!(p.placed_origin((427.0, 240.0)), Some((12.0, 20.0)));
    answer(&mut game, "placed 3 12 20 32 24 1").unwrap();
    let p = game.canvas().unwrap();
    assert_eq!(p.placed_origin((427.0, 240.0)), Some((383.0, 196.0)));
    assert_eq!(p.placed_origin((100.0, 80.0)), Some((56.0, 36.0)));
    answer(&mut game, "placed 1 -90 1000 32 24 1").unwrap();
    assert_eq!(
        game.canvas().unwrap().placed_origin((100.0, 80.0)),
        Some((68.0, 56.0))
    );
    answer(&mut game, "placed 2 1000 -10 32 24 1").unwrap();
    assert_eq!(
        game.canvas().unwrap().placed_origin((100.0, 80.0)),
        Some((68.0, 56.0))
    );
    answer(&mut game, "placed 0 NaN inf 2000 -1 0").unwrap();
    let p = game.canvas().unwrap();
    assert_eq!(
        (p.width, p.height, p.placed),
        (1024.0, 0.0, Some((0.0, 0.0)))
    );
    assert_eq!(p.placed_origin((100.0, 80.0)), Some((0.0, 0.0)));
    answer(&mut game, "box 99").unwrap();
    assert!(game.canvas().unwrap().placed.is_none());
    answer(&mut game, "clear").unwrap();
    assert!(game.canvas().is_none());
}

/// Real guest callbacks, screen limits, replacement, close notification and
/// frame updates, driven through the same scripts as cargo horizon-mod test.
#[test]
fn canvas_screen_events_and_scenarios() {
    let mut game = canvas_guest();
    let script = Script::parse(r#"
[[step]]
command = "placed 3 8 12 32 24 1"
expect.canvas = { placed = true, dx = 8.0, dy = 12.0, x = 387.0, y = 204.0 }
[[step]]
command = "screen_set 2"
expect.canvas-screen = { open = false, shapes = 0 }
[[step]]
command = "screen 4100"
expect.canvas-screen = { open = true, title = "Canvas guest", shapes = 4096 }
[[step]]
pointer = { kind = "down", x = 10.5, y = 20.0, button = 0 }
[[step]]
pointer = { kind = "drag", x = 12.0, y = 24.0, button = 0 }
[[step]]
pointer = { kind = "up", x = 12.0, y = 24.0, button = 0 }
[[step]]
pointer = { kind = "move", x = 13.0, y = 25.0, button = 0 }
[[step]]
pointer = { kind = "down", x = 13.0, y = 25.0, button = 1 }
scroll = { x = 13.0, y = 25.0, steps = 2.0 }
screen-key = { key = "key.keyboard.left.shift", down = true }
[[step]]
screen-key = { key = "key.keyboard.left.shift", down = false }
[[step]]
screen-key = { key = "key.keyboard.left", down = true }
[[step]]
screen-key = { key = "key.keyboard.left", down = false }
[[step]]
command = "events"
expect.feedback = "down 10.5 20 0;drag 12 24 0;up 12 24 0;move 13 25 0;down 13 25 1;scroll 13 25 2;key key.keyboard.left.shift true;key key.keyboard.left.shift false;key key.keyboard.left true;key key.keyboard.left false"
[[step]]
command = "screen_set 4200"
expect.canvas-screen = { shapes = 4096 }
[[step]]
command = "screen_frames"
[[step]]
frames = 2
expect.canvas-screen = { shapes = 1 }
[[step]]
screen-key = { key = "key.keyboard.escape", down = true }
expect.canvas-screen = { open = false, shapes = 0 }
[[step]]
pointer = { kind = "down", x = 0.0, y = 0.0 }
scroll = { x = 0.0, y = 0.0, steps = -1.0 }
screen-key = { key = "key.keyboard.left", down = true }
command = "events"
expect.feedback = "closed"
[[step]]
command = "screen 2"
[[step]]
command = "screen_close"
expect.canvas-screen = { open = false }
[[step]]
command = "events"
expect.feedback = ""
"#).unwrap();
    script.run(&mut game).unwrap();
    assert!(
        game.canvas().is_some(),
        "closing a screen keeps the HUD box"
    );
}

/// Screen imports require both permissions even though canvas's existing
/// image and HUD calls continue to need render alone.
#[test]
fn canvas_screens_require_ui_and_render() {
    let package = Package::read(&mods_dir().join("horizon_mods_test_canvas_guest.hmod")).unwrap();
    let manifest = CANVAS_MANIFEST.replace("\"ui\", ", "");
    let bytes = Package::write(&manifest, package.wasm.as_ref().unwrap()).unwrap();
    let dir = mods_dir().join("canvas_no_ui");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("mod.hmod");
    std::fs::write(&path, bytes).unwrap();
    let mods = Mods::load(&[path], |_| true, |_| false);
    assert!(matches!(&mods.entries()[0].status, Status::Failed(s) if s.contains("capability ui")));
}

/// A pre-18 UI export keeps its original callbacks even with no new hooks.
#[test]
fn legacy_ui_events_still_work() {
    let mut mods = load("legacy_ui.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.command("ui_screen"));
    mods.drain();
    mods.ui_select(0, 7, 2);
    mods.ui_closed(0);
    let lines: Vec<_> = mods
        .drain()
        .into_iter()
        .filter_map(|o| match o {
            Output::Chat { text, .. } => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(lines, ["selected 7:2", "closed"]);
    mods.ui_settings_opened(0);
    assert_eq!(mods.settings_widgets(0).len(), 2);
}

/// Older components with no ui-events export still load and run normally.
#[test]
fn missing_ui_events_are_optional() {
    let mut mods = load("legacy_no_ui.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.command("ui_screen"));
    mods.drain();
    mods.ui_select(0, 7, 2);
    mods.ui_closed(0);
    mods.ui_settings_opened(0);
    assert!(mods.drain().is_empty());
    assert_eq!(mods.entries()[0].status, Status::Running);
}

/// The example opens its full-window gradient from its registered key.
#[test]
fn canvas_example_opens_on_its_key() {
    let mut mods = load("canvas_test.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    mods.set_snapshot(player());
    assert!(mods.canvas_screen(0).is_none());
    mods.key("key.keyboard.m", true);
    mods.frame(1.0 / 60.0);
    let screen = mods.canvas_screen(0).unwrap();
    assert_eq!(screen.title, "Canvas test");
    assert_eq!(screen.shapes.len(), 1);
    assert!(mods.drain().contains(&Output::OpenScreen { owner: 0 }));
}

#[test]
fn api20_client_block_at_reads_loaded_states_and_none_off_world() {
    let mut game = canvas_guest();
    game.set_block(1, 64, 0, "minecraft:oak_log[axis=y]");
    for (query, want) in [
        ("block_at 1 64 0", "Some(\"minecraft:oak_log[axis=y]\")"),
        ("block_at 2 64 0", "Some(\"minecraft:air\")"),
        ("block_at 32 64 0", "None"),
    ] {
        assert!(game.command(query));
        assert!(
            game.drain()
                .iter()
                .any(|o| matches!(o,Output::Feedback {text,..} if text==want))
        );
    }
    game.player = None;
    assert!(game.command("block_at 1 64 0"));
    assert!(
        game.drain()
            .iter()
            .any(|o| matches!(o,Output::Feedback {text,..} if text=="None"))
    );
}

#[test]
fn api20_lines_touch_keys_and_stop_cleanup_cross_the_sandbox() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    mods.set_snapshot(Snapshot {
        touch_active: true,
        ..player()
    });
    assert!(mods.command("world_lines 4097"));
    assert_eq!(mods.world_lines().next().unwrap().lines.len(), 4096);
    assert!(mods.command("world_lines 0"));
    assert!(mods.world_lines().next().unwrap().lines.is_empty());
    assert!(mods.command("touch_button on"));
    let (owner, name, b, _) = mods.touch_buttons().next().unwrap();
    assert_eq!((&*b.label, b.size, b.lit), ("Mine", 24.0, true));
    assert_eq!(b.icon.as_deref(), Some("minecraft:diamond_pickaxe"));
    let (owner, name) = (owner, name.to_owned());
    mods.touch_key(owner, &name, true);
    mods.touch_key(owner, &name, false);
    assert!(mods.command("input_poll"));
    let text = mods
        .drain()
        .into_iter()
        .filter_map(|o| match o {
            Output::Chat { text, .. } => Some(text),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("touch true"), "{text}");
    assert!(text.contains("key-down mine"), "{text}");
    assert!(text.contains("key-up mine"), "{text}");
    // Either input source keeps the key held until both have released it.
    mods.key("key.keyboard.r", true);
    mods.touch_key(owner, &name, true);
    mods.key("key.keyboard.r", false);
    assert!(mods.command("input_poll"));
    let events = mods.drain();
    assert!(
        events
            .iter()
            .any(|o| matches!(o,Output::Chat {text,..} if text=="key-down mine"))
    );
    assert!(
        !events
            .iter()
            .any(|o| matches!(o,Output::Chat {text,..} if text=="key-up mine"))
    );
    mods.touch_key(owner, &name, false);
    assert!(mods.command("input_poll"));
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o,Output::Chat {text,..} if text=="key-up mine"))
    );
    assert!(mods.command("touch_button off"));
    assert_eq!(mods.touch_buttons().count(), 0);
    assert!(mods.command("world_lines 1"));
    mods.clear_world_lines();
    assert!(mods.world_lines().next().unwrap().lines.is_empty());
    // Replacing runtimes on reload drops all persistent API 20 state.
    mods = Mods::default();
    assert_eq!(mods.world_lines().count(), 0);
    assert_eq!(mods.touch_buttons().count(), 0);
}

#[test]
fn api20_queued_breaks_exclude_the_caller_but_notify_other_mods() {
    let mut mods = Mods::load(
        &[
            mods_dir().join("horizon_mods_test_api20_server_guest.hmod"),
            mods_dir().join("api20_other.hmod"),
        ],
        |_| true,
        |_| false,
    );
    assert!(mods.entries().iter().all(|e| e.status == Status::Running));
    let p = ServerPlayer {
        id: "alice".into(),
        name: "Alex".into(),
        x: 0.0,
        y: 64.0,
        z: 0.0,
        dimension: "minecraft:overworld".into(),
        game_mode: GameMode::Survival,
        health: 20.0,
    };
    mods.set_snapshot(Snapshot {
        players: vec![p.clone()],
        ..player()
    });
    mods.server_tick(1);
    let out = mods.drain();
    assert!(out.iter().any(|o| matches!(o,Output::BreakBlocksAs {mod_id,player,..} if mod_id == "test_api20_server" && player == "alice")));
    let pos = BlockPos { x: 1, y: 64, z: 0 };
    mods.block_broken_except(&p, pos, "minecraft:stone", Some("test_api20_server"));
    let delivered = mods.drain();
    assert_eq!(delivered.len(), 1);
    assert!(
        matches!(&delivered[0],Output::Tell { name,text,.. } if name=="API20 Other" && text=="heard break")
    );
    mods.block_broken_except(&p, pos, "minecraft:stone", Some("test_api20_other"));
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o,Output::Tell {text,..} if text == "heard break"))
    );
}

#[test]
fn mod_quiet_commands_cross_the_sandbox_and_reset_each_tick() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    let mut snapshot = player();
    mods.set_snapshot(snapshot.clone());
    assert!(mods.command("quiet_mod beacon 2;missing;setblock 0 0 0 stone"));
    let out = mods.drain();
    assert!(
        matches!(&out[..],[Output::ModQuietCommands {mod_id,lines}] if mod_id=="test_ui_guest" && lines==&["beacon 2","missing","setblock 0 0 0 stone"])
    );
    let mut queued = 3;
    for _ in 0..5 {
        assert!(mods.command("quiet_mod_many 30"));
        for out in mods.drain() {
            if let Output::ModQuietCommands { lines, .. } = out {
                assert!(lines.len() <= 16);
                queued += lines.len();
            }
        }
    }
    assert_eq!(queued, 64);
    snapshot.tick += 1;
    mods.set_snapshot(snapshot);
    assert!(mods.command("quiet_mod beacon 3"));
    assert!(
        matches!(&mods.drain()[..],[Output::ModQuietCommands {lines,..}] if lines==&["beacon 3"])
    );
}

#[test]
fn quiet_server_mod_hooks_keep_operations_but_drop_success_and_failure_feedback() {
    let mut mods = load("beacons.hmod");
    let p = ServerPlayer {
        id: "alice".into(),
        name: "Alex".into(),
        x: 0.0,
        y: 64.0,
        z: 0.0,
        dimension: "minecraft:overworld".into(),
        game_mode: GameMode::Survival,
        health: 20.0,
    };
    mods.set_snapshot(Snapshot {
        players: vec![p.clone()],
        ..player()
    });
    assert!(mods.server_command_quiet(&p, "beacon 2"));
    let out = mods.drain();
    assert!(
        out.iter()
            .any(|o| matches!(o,Output::SetBlocks {blocks,..} if blocks.len()==2))
    );
    assert!(
        !out.iter()
            .any(|o| matches!(o, Output::Reply { .. } | Output::Feedback { .. }))
    );
    assert!(mods.server_command_quiet(&p, "beacon invalid"));
    assert!(mods.drain().is_empty());
    assert!(!mods.server_command_quiet(&p, "not_registered"));
    assert!(mods.drain().is_empty());
    struct Ground;
    impl horizon_mods::WorldView for Ground {
        fn block(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            Some("minecraft:stone".into())
        }
        fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
            Some("minecraft:plains".into())
        }
        fn has_dimension(&self, _: &str) -> bool {
            true
        }
        fn check_state(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    mods.lend_world(Box::new(Ground));
    assert!(mods.server_command_quiet(&p, "ground"));
    assert!(mods.drain().is_empty());
    // The original typed-command API still gives its normal success/error feedback.
    assert!(mods.server_command(&p, "ground"));
    assert!(mods.drain().iter().any(|o| matches!(o,Output::Reply {text,error:false,..} if text == "You are standing on minecraft:stone in minecraft:plains")));
    assert!(mods.server_command(&p, "beacon invalid"));
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o, Output::Reply { error: true, .. }))
    );
}

#[test]
fn api21_canvas_box_key_validates_clears_and_delivers_registered_key() {
    fn reply(mods: &mut Mods, line: &str) -> Result<String, String> {
        assert!(mods.command(line));
        mods.drain()
            .into_iter()
            .find_map(|out| match out {
                Output::Feedback { text, error } => Some(if error { Err(text) } else { Ok(text) }),
                _ => None,
            })
            .expect("command feedback")
    }
    let mut mods = load("horizon_mods_test_canvas_guest.hmod");
    assert_eq!(mods.canvas_box_key(0), None);
    assert!(
        reply(&mut mods, "box_key missing")
            .unwrap_err()
            .contains("no registered key missing")
    );
    assert_eq!(
        reply(&mut mods, "box_key box").as_deref(),
        Ok("box key set")
    );
    assert_eq!(mods.canvas_box_key(0), Some("box"));
    assert!(reply(&mut mods, "box_key missing").is_err());
    assert_eq!(mods.canvas_box_key(0), Some("box"));
    mods.touch_key(0, "box", true);
    mods.touch_key(0, "box", false);
    assert_eq!(
        reply(&mut mods, "box_poll").as_deref(),
        Ok("down box;up box")
    );
    assert_eq!(
        reply(&mut mods, "box_key none").as_deref(),
        Ok("box key set")
    );
    assert_eq!(mods.canvas_box_key(0), None);
    mods.touch_key(0, "box", true);
    mods.touch_key(0, "box", false);
    assert_eq!(reply(&mut mods, "box_poll").as_deref(), Ok(""));
    // The game's automatic button can use this registered key with neither an
    // explicit touch button nor a canvas binding. Physical input still shares the hold.
    mods.registered_touch_key(0, "box", true);
    assert!(mods.touch_key_down(0, "box"));
    mods.key("key.keyboard.m", true);
    mods.registered_touch_key(0, "box", false);
    assert_eq!(
        reply(&mut mods, "box_poll").as_deref(),
        Ok("down box;down box")
    );
    mods.key("key.keyboard.m", false);
    assert_eq!(reply(&mut mods, "box_poll").as_deref(), Ok("up box"));
    mods.registered_touch_key(0, "missing", true);
    assert!(!mods.touch_key_down(0, "missing"));
    assert_eq!(reply(&mut mods, "box_poll").as_deref(), Ok(""));
}

#[test]
fn controller_registered_key_events_merge_with_touch_keyboard_and_menu_release() {
    fn poll(mods: &mut Mods) -> String {
        assert!(mods.command("box_poll"));
        mods.drain()
            .into_iter()
            .find_map(|o| match o {
                Output::Feedback { text, error: false } => Some(text),
                _ => None,
            })
            .unwrap()
    }
    let mut mods = load("horizon_mods_test_canvas_guest.hmod");
    mods.controller_key(0, "missing", true);
    assert_eq!(poll(&mut mods), "");
    mods.controller_key(0, "box", true);
    mods.controller_key(0, "box", true);
    assert_eq!(poll(&mut mods), "down box");
    mods.registered_touch_key(0, "box", true);
    mods.controller_key(0, "box", false);
    assert_eq!(poll(&mut mods), "");
    mods.registered_touch_key(0, "box", false);
    assert_eq!(poll(&mut mods), "up box");
    mods.key("key.keyboard.m", true);
    mods.controller_key(0, "box", true);
    mods.key("key.keyboard.m", false);
    assert_eq!(poll(&mut mods), "down box");
    mods.controller_key(0, "box", false);
    assert_eq!(poll(&mut mods), "up box");
    mods.controller_key(0, "box", true);
    mods.release_keys();
    mods.controller_key(0, "box", false);
    assert_eq!(poll(&mut mods), "down box;up box");
    mods.controller_key(0, "box", true);
    mods.controller_key(0, "box", false);
    assert_eq!(poll(&mut mods), "down box;up box");
}

#[test]
fn api21_chat_open_queues_only_a_capped_prefill_and_legacy_chat_still_links() {
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    assert!(mods.command("ui_screen"));
    assert_eq!(mods.drain(), [Output::OpenScreen { owner: 0 }]);
    assert!(mods.command("chat_open /waypoint rename Home"));
    assert_eq!(
        mods.drain(),
        [Output::OpenChat {
            owner: 0,
            text: "/waypoint rename Home".into()
        }]
    );
    assert!(
        mods.drain().is_empty(),
        "draining consumes the request; nothing was sent"
    );
    let text = format!("{}tail", "é".repeat(256));
    assert!(mods.command(&format!("chat_open {text}")));
    assert_eq!(
        mods.drain(),
        [Output::OpenChat {
            owner: 0,
            text: "é".repeat(256)
        }]
    );
    assert!(mods.command("chat_open"));
    assert_eq!(
        mods.drain(),
        [Output::OpenChat {
            owner: 0,
            text: String::new()
        }]
    );
    let mut legacy = load("legacy_chat.hmod");
    assert_eq!(legacy.entries()[0].status, Status::Running);
    assert!(legacy.command("hello old chat"));
    assert_eq!(
        legacy.drain(),
        [Output::Chat {
            name: "Test Guest".into(),
            text: "hello old chat after 0 ticks".into()
        }]
    );
    let package = Package::read(&mods_dir().join("legacy_chat.hmod")).unwrap();
    let functions = horizon_mods::api_level::imports(package.wasm.as_ref().unwrap()).unwrap();
    assert!(functions.iter().any(|f| f == "chat.show"));
    assert!(!functions.iter().any(|f| f == "chat.open"));
}

#[test]
fn container_calls_read_live_updates_and_events_are_optional() {
    use horizon_mods::{
        ContainerPointerKind as K, ContainerRequest as Q, ContainerStack, ContainerState,
    };
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    assert!(mods.command("container_state"));
    assert!(matches!(&mods.drain()[..], [Output::Chat {text, ..}] if text == "no container"));
    let mut state = ContainerState {
        slots: vec![],
        carried: None,
        x: 20.0,
        y: 30.0,
        width: 176.0,
        height: 166.0,
    };
    let mut took = false;
    mods.with_container(
        |q| match q {
            Q::State => Some(state.clone()),
            Q::Click {
                slot,
                button,
                shift,
            } => {
                assert_eq!((slot, button, shift), (9, 1, true));
                state.carried = Some(ContainerStack {
                    item: "minecraft:stone".into(),
                    count: 7,
                    max_stack: 64,
                    same_key: 11,
                });
                None
            }
            Q::TakeDrag => {
                took = true;
                None
            }
        },
        |mods| {
            assert!(mods.command("container_click 9 1 shift"));
            mods.container_pointer(K::Down, 10.0, 20.0, 1, Some(9), false);
        },
    );
    assert!(took);
    assert!(
        mods.drain()
            .iter()
            .any(|o| matches!(o, Output::Chat {text, ..} if text == "after click carried 7"))
    );
    mods.command("container_latch on");
    let mut latched_click = false;
    mods.with_container(
        |q| {
            if let Q::Click { shift, .. } = q {
                latched_click = shift;
            }
            None
        },
        |mods| {
            mods.command("container_click 9 0");
        },
    );
    assert!(
        !latched_click,
        "a mod click's shift is used as given, the latch only shifts player clicks"
    );
    mods.command("container_latch off");
    mods.container_opened();
    mods.container_closed();
    assert_eq!(mods.drain().len(), 2);
    assert!(!mods.container_scroll(10.0, 20.0, 1.0, None));
    assert!(mods.container_scroll(10.0, 20.0, -1.0, Some(9)));
    // Components without the new interface keep running and don't consume input.
    let mut old = load("legacy_no_container.hmod");
    old.container_opened();
    old.container_pointer(K::Drag, 1.0, 2.0, 0, Some(0), false);
    old.container_closed();
    assert!(!old.container_scroll(1.0, 2.0, 1.0, Some(0)));
    assert_eq!(old.entries()[0].status, Status::Running);
    assert!(old.drain().is_empty());
    let mut partial = load("legacy_container_pointer.hmod");
    partial.container_opened();
    assert!(!partial.container_scroll(1.0, 2.0, 1.0, Some(0)));
    partial.container_pointer(K::Drag, 1.0, 2.0, 0, Some(0), false);
    assert_eq!(partial.drain().len(), 1, "present pointer still runs");
}

#[test]
fn container_buttons_are_owned_bounded_and_latches_clear_on_stop() {
    use horizon_mods::ContainerPointerKind as K;
    let mut mods = load("horizon_mods_test_ui_guest.hmod");
    assert!(mods.command("container_buttons"));
    let buttons: Vec<_> = mods.container_buttons().collect();
    assert_eq!(buttons.len(), 8);
    assert!(
        buttons
            .iter()
            .all(|(owner, b)| *owner == 0 && b.tooltip.chars().count() == 256)
    );
    mods.container_button(0, 7);
    assert!(mods.container_shift_latched());
    mods.container_opened();
    mods.container_closed();
    assert!(mods.container_shift_latched(), "latch survives screens");
    mods.drain();
    mods.container_pointer(K::Drag, 10.0, 20.0, 0, Some(9), false);
    assert!(matches!(&mods.drain()[..], [Output::Chat {text, ..}] if text.ends_with("true")));
    mods.command("container_latch off");
    assert!(!mods.container_shift_latched());
    mods.command("container_buttons off");
    assert_eq!(mods.container_buttons().count(), 7);
    mods.container_button(0, 0);
    assert!(mods.drain().is_empty(), "removed button cannot fire");
    mods.command("container_latch on");
    // Turning a mod off drops both the latch and buttons.
    mods.command("container_trap");
    assert!(matches!(mods.entries()[0].status, Status::Failed(_)));
    assert!(!mods.container_shift_latched());
    assert_eq!(mods.container_buttons().count(), 0);
}

#[test]
fn container_scroll_stops_at_first_consumer_and_requires_capability() {
    let package = Package::read(&mods_dir().join("horizon_mods_test_ui_guest.hmod")).unwrap();
    let dir = std::env::temp_dir().join(format!("horizon-container-pair-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let second = dir.join("second.hmod");
    let manifest = UI_MANIFEST.replace("test_ui_guest", "second_ui_guest");
    std::fs::write(
        &second,
        Package::write(&manifest, package.wasm.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    let mut mods = Mods::load(
        &[mods_dir().join("horizon_mods_test_ui_guest.hmod"), second],
        |_| true,
        |_| false,
    );
    assert!(mods.entries().iter().all(|e| e.status == Status::Running));
    assert!(!mods.container_scroll(1.0, 2.0, 1.0, None));
    assert_eq!(mods.drain().len(), 2, "both hear an unconsumed scroll");
    assert!(mods.container_scroll(1.0, 2.0, 1.0, Some(9)));
    assert_eq!(mods.drain().len(), 1, "only the first consumer hears it");
    mods.container_button(0, 0);
    mods.container_button(1, 0);
    assert!(mods.container_shift_latched());
    mods.container_button(0, 0);
    assert!(
        mods.container_shift_latched(),
        "second mod still latches shift"
    );
    mods.container_closed();
    assert!(mods.container_shift_latched());
    mods.container_button(1, 0);
    assert!(
        !mods.container_shift_latched(),
        "both mods released their latch"
    );
    let denied = dir.join("denied.hmod");
    let manifest = UI_MANIFEST.replace(", \"container\"", "");
    std::fs::write(
        &denied,
        Package::write(&manifest, package.wasm.as_ref().unwrap()).unwrap(),
    )
    .unwrap();
    let mods = Mods::load(&[denied], |_| true, |_| false);
    assert!(
        matches!(&mods.entries()[0].status, Status::Failed(why) if why.contains("capability container"))
    );
    std::fs::remove_dir_all(dir).unwrap();
}
