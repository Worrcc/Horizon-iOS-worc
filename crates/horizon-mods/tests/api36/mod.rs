//! API 36 (`world.top-block`) across the real SDK guest/component boundary.
use super::*;
use horizon_mods::WorldView;

/// A column at block x 3, z -2: stone at 10, water at 11, leaves at 200;
/// column 0, 0 has grass at 70; nothing else is loaded.
struct Column;
impl WorldView for Column {
    fn block(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
        None
    }
    fn biome(&self, _: &str, _: i32, _: i32, _: i32) -> Option<String> {
        None
    }
    fn has_dimension(&self, dim: &str) -> bool {
        dim == "minecraft:overworld"
    }
    fn check_state(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn top_block(&self, dim: &str, x: i32, z: i32, below: i32) -> Option<i32> {
        if !self.has_dimension(dim) {
            return None;
        }
        let column: &[i32] = match (x, z) {
            (3, -2) => &[200, 11, 10],
            (0, 0) => &[70],
            _ => return None,
        };
        column.iter().copied().find(|&y| y <= below.min(319))
    }
}

fn alice() -> ServerPlayer {
    ServerPlayer {
        id: "alice".into(),
        name: "Alice".into(),
        x: 0.0,
        y: 64.0,
        z: 0.0,
        dimension: "minecraft:overworld".into(),
        game_mode: GameMode::Creative,
        health: 20.0,
    }
}

fn reply(mods: &mut Mods, line: &str) -> String {
    assert!(mods.server_command(&alice(), line), "{line}");
    assert_eq!(mods.entries()[0].status, Status::Running);
    mods.drain()
        .into_iter()
        .find_map(|o| match o {
            Output::Reply {
                text, error: false, ..
            } => Some(text),
            _ => None,
        })
        .unwrap_or_else(|| panic!("no reply to {line}"))
}

fn setup() -> Mods {
    let mut mods = load("horizon_mods_test_api20_server_guest.hmod");
    assert_eq!(mods.entries()[0].status, Status::Running);
    mods.set_snapshot(Snapshot {
        players: vec![alice()],
        ..player()
    });
    mods.lend_world(Box::new(Column));
    mods.drain();
    mods
}

#[test]
fn api36_top_block_reads_at_or_below_a_height() {
    let mut mods = setup();
    assert_eq!(reply(&mut mods, "w36 top 3 -2 1000"), "top 200");
    assert_eq!(reply(&mut mods, "w36 top 3 -2 199"), "top 11");
    assert_eq!(reply(&mut mods, "w36 top 3 -2 10"), "top 10");
    assert_eq!(reply(&mut mods, "w36 top 3 -2 9"), "top none");
    assert_eq!(reply(&mut mods, "w36 top 5 5 100"), "top none");
    assert_eq!(reply(&mut mods, "w36 unknown"), "None");
    // The SDK's whole-column shorthand.
    assert_eq!(reply(&mut mods, "w36 surface"), "Some(70)");
}

#[test]
fn api36_without_a_lent_world_top_block_is_none_and_the_guest_needs_level_45() {
    let mut mods = load("horizon_mods_test_api20_server_guest.hmod");
    mods.set_snapshot(Snapshot {
        players: vec![alice()],
        ..player()
    });
    assert_eq!(reply(&mut mods, "w36 top 3 -2 1000"), "top none");
    let package =
        Package::read(&mods_dir().join("horizon_mods_test_api20_server_guest.hmod")).unwrap();
    let imports = horizon_mods::api_level::imports(package.wasm.as_deref().unwrap()).unwrap();
    assert!(imports.iter().any(|f| f == "world.top-block"));
    assert_eq!(
        horizon_mods::api_level::needed(imports.iter().map(String::as_str)).0,
        45
    );
}

#[test]
fn api36_server_mod_log_lines_reach_the_host_log_with_the_mod_id() {
    let mut mods = setup();
    let lines = capture_mod_log(|| {
        assert_eq!(reply(&mut mods, "w36 log"), "logged");
    });
    assert!(
        lines
            .iter()
            .any(|l| l == "INFO [mod test_api20_server] w36 info line"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l == "WARN [mod test_api20_server] w36 warn line"),
        "{lines:?}"
    );
    // The game's log keeps info and above: a debug line is kept, marked.
    assert!(
        lines
            .iter()
            .any(|l| l == "INFO [mod test_api20_server] (debug) w36 debug line"),
        "{lines:?}"
    );
}

/// Every `log` record while `run` runs, as `LEVEL message` (a process-wide
/// logger: the guest may run on another thread).
fn capture_mod_log(run: impl FnOnce()) -> Vec<String> {
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
    struct Capture;
    impl log::Log for Capture {
        fn enabled(&self, _: &log::Metadata) -> bool {
            ACTIVE.load(Ordering::Relaxed)
        }
        fn log(&self, record: &log::Record) {
            if !ACTIVE.load(Ordering::Relaxed) {
                return;
            }
            LINES
                .lock()
                .unwrap()
                .push(format!("{} {}", record.level(), record.args()));
        }
        fn flush(&self) {}
    }
    static CAPTURE: Capture = Capture;
    let _ = log::set_logger(&CAPTURE);
    let before = log::max_level();
    LINES.lock().unwrap().clear();
    log::set_max_level(log::LevelFilter::Trace);
    ACTIVE.store(true, Ordering::Relaxed);
    run();
    ACTIVE.store(false, Ordering::Relaxed);
    log::set_max_level(before);
    std::mem::take(&mut *LINES.lock().unwrap())
}
