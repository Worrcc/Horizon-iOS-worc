//! A shared engine for live worlds, finalized before any mod can run.
use super::{LimitMode, State, linker};
use std::{
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use wasmtime::{
    Config, Engine,
    component::{Component, Linker},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Native,
    Pulley,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Pulley => "pulley",
        }
    }
}

struct Choice {
    kind: Kind,
    reason: &'static str,
    unknown: bool,
}

/// Pure policy: platform and build defaults are explicit so every combination
/// can be tested on a desktop without changing the process environment.
fn choose(ios: bool, jit: bool, force_pulley: bool, preference: &str) -> Choice {
    let unknown = !matches!(preference, "auto" | "native" | "pulley");
    let (kind, reason) = match preference {
        "pulley" => (Kind::Pulley, "HORIZON_MODS_ENGINE=pulley"),
        "native" => (Kind::Native, "HORIZON_MODS_ENGINE=native"),
        _ if force_pulley => (Kind::Pulley, "force-pulley build default"),
        _ if ios && !jit => (Kind::Pulley, "iOS without CS_DEBUGGED"),
        _ if ios => (Kind::Native, "iOS CS_DEBUGGED set"),
        _ => (Kind::Native, "desktop auto"),
    };
    Choice {
        kind,
        reason,
        unknown,
    }
}

#[cfg(target_os = "ios")]
fn platform_jit() -> bool {
    const CS_OPS_STATUS: libc::c_uint = 0;
    const CS_DEBUGGED: u32 = 0x1000_0000;
    unsafe extern "C" {
        fn csops(
            pid: libc::pid_t,
            ops: libc::c_uint,
            useraddr: *mut libc::c_void,
            usersize: libc::size_t,
        ) -> libc::c_int;
    }
    let mut flags = 0u32;
    // SAFETY: csops receives the current pid and a writable, initialized u32
    // with its exact size. Failure (including a sandbox denial) means no JIT.
    let status = unsafe {
        csops(
            libc::getpid(),
            CS_OPS_STATUS,
            (&mut flags as *mut u32).cast(),
            4,
        )
    };
    status == 0 && flags & CS_DEBUGGED != 0
}

#[cfg(not(target_os = "ios"))]
fn platform_jit() -> bool {
    false
}

/// Probe only the first native component, before publishing the engine. A
/// failed probe drops the entire native runtime (including its linker) before
/// building Pulley. Pulley compilation errors remain per-mod errors, so one
/// malformed first mod cannot poison the engine for every other mod.
fn initialize<T>(
    choice: Choice,
    mut build: impl FnMut(Kind) -> Result<T, String>,
    mut probe: impl FnMut(&T) -> Result<(), String>,
) -> Result<(T, Kind, String), String> {
    if choice.kind == Kind::Pulley {
        return build(Kind::Pulley).map(|runtime| (runtime, Kind::Pulley, choice.reason.into()));
    }
    let native = build(Kind::Native).and_then(|runtime| {
        probe(&runtime)?;
        Ok(runtime)
    });
    match native {
        Ok(runtime) => Ok((runtime, Kind::Native, choice.reason.into())),
        Err(error) => {
            log::warn!("Horizon: native mods engine failed: {error}; falling back to pulley");
            build(Kind::Pulley)
                .map(|runtime| (runtime, Kind::Pulley, format!("native failed: {error}")))
                .map_err(|fallback| format!("native failed: {error}; pulley failed: {fallback}"))
        }
    }
}

struct Ticker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    #[cfg(test)]
    finished: Arc<AtomicBool>,
}
impl Ticker {
    fn new(engine: &Engine) -> Result<Self, String> {
        let engine = engine.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        #[cfg(test)]
        let finished = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let ended = finished.clone();
        let thread = thread::Builder::new()
            .name("horizon-mod-epoch".into())
            .spawn(move || {
                while !stopped.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(1));
                    engine.increment_epoch();
                }
                #[cfg(test)]
                ended.store(true, Ordering::Release);
            })
            .map_err(|e| format!("epoch ticker: {e}"))?;
        Ok(Self {
            stop,
            thread: Some(thread),
            #[cfg(test)]
            finished,
        })
    }
}
impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Runtime {
    _ticker: Option<Ticker>,
    engine: Engine,
    linker: Linker<State>,
}
impl Runtime {
    fn new(kind: Kind, limit: LimitMode) -> Result<Self, String> {
        let mut config = Config::new();
        config.wasm_component_model(true);
        limit.configure(&mut config);
        if kind == Kind::Pulley {
            config
                .target("pulley64")
                .map_err(|e| format!("engine target: {e}"))?;
        }
        if cfg!(target_os = "ios") || kind == Kind::Pulley {
            // iOS native code also avoids signal handlers, guard pages and
            // large virtual reservations; growing memory copies instead.
            config
                .signals_based_traps(false)
                .memory_reservation(0)
                .memory_guard_size(0);
        }
        let engine = Engine::new(&config).map_err(|e| format!("Engine::new: {e:#}"))?;
        let linker = linker(&engine).map_err(|e| format!("creating linker: {e}"))?;
        let ticker = if limit == LimitMode::Epoch {
            Some(Ticker::new(&engine)?)
        } else {
            None
        };
        Ok(Self {
            _ticker: ticker,
            engine,
            linker,
        })
    }

    fn compile(&self, wasm: &[u8]) -> Result<Component, String> {
        Component::new(&self.engine, wasm).map_err(|e| format!("compiling component: {e:#}"))
    }
}

/// The component travels with the only engine/linker allowed to instantiate
/// it. There is no engine replacement after this value can escape.
pub(super) struct Compiled<'a> {
    pub engine: &'a Engine,
    pub linker: &'a Linker<State>,
    pub component: Component,
}

#[derive(Default)]
pub(super) struct Cache {
    runtime: OnceLock<Result<Runtime, String>>,
    pub limit: LimitMode,
    selection: Option<&'static OnceLock<Kind>>,
}

impl Cache {
    pub fn new(limit: LimitMode) -> Self {
        Self {
            limit,
            ..Self::default()
        }
    }
    pub fn compile_env(&self, wasm: &[u8]) -> Result<Compiled<'_>, String> {
        self.compile(wasm, environment_choice)
    }
    fn compile(
        &self,
        wasm: &[u8],
        choice: impl FnOnce() -> Choice,
    ) -> Result<Compiled<'_>, String> {
        let mut first = None;
        let runtime = self
            .runtime
            .get_or_init(|| {
                if let Some(&kind) = self.selection.and_then(OnceLock::get) {
                    let runtime = Runtime::new(kind, self.limit)?;
                    log::info!(
                        "Horizon: mods run on {}, limit {}",
                        kind.name(),
                        self.limit.name()
                    );
                    return Ok(runtime);
                }
                let choice = choice();
                let (runtime, kind, reason) = initialize(
                    choice,
                    |kind| Runtime::new(kind, self.limit),
                    |runtime| {
                        first = Some(runtime.compile(wasm)?);
                        Ok(())
                    },
                )?;
                if let Some(selection) = self.selection {
                    let _ = selection.set(kind);
                }
                log::info!(
                    "Horizon: mods run on {}, limit {}",
                    kind.name(),
                    self.limit.name()
                );
                log::debug!("Horizon: mods engine selection: {reason}");
                Ok(runtime)
            })
            .as_ref()
            .map_err(Clone::clone)?;
        let component = match first {
            Some(component) => component,
            None => runtime.compile(wasm)?,
        };
        Ok(Compiled {
            engine: &runtime.engine,
            linker: &runtime.linker,
            component,
        })
    }
}

/// Weak ownership shares the engine across live worlds without keeping its
/// ticker alive after the last Mods is dropped. Selection stays finalized.
pub(super) fn shared() -> Arc<Cache> {
    static SELECTION: OnceLock<Kind> = OnceLock::new();
    static CACHE: Mutex<Weak<Cache>> = Mutex::new(Weak::new());
    let mut slot = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(cache) = slot.upgrade() {
        return cache;
    }
    let cache = Arc::new(Cache {
        selection: Some(&SELECTION),
        ..Cache::new(LimitMode::from_env())
    });
    *slot = Arc::downgrade(&cache);
    cache
}

fn environment_choice() -> Choice {
    let preference = std::env::var_os("HORIZON_MODS_ENGINE");
    let preference = preference.as_ref().map(|v| v.to_string_lossy());
    let preference = preference.as_deref().unwrap_or("auto");
    let choice = choose(
        cfg!(target_os = "ios"),
        platform_jit(),
        cfg!(feature = "force-pulley"),
        preference,
    );
    if choice.unknown {
        log::warn!("Horizon: unknown HORIZON_MODS_ENGINE={preference:?}; using auto");
    }
    choice
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_choice_all_platform_flag_build_and_override_combinations() {
        for ios in [false, true] {
            for jit in [false, true] {
                for force in [false, true] {
                    let auto = if force || (ios && !jit) {
                        Kind::Pulley
                    } else {
                        Kind::Native
                    };
                    for (preference, expected) in [
                        ("auto", auto),
                        ("native", Kind::Native),
                        ("pulley", Kind::Pulley),
                        ("", auto),
                        ("invalid", auto),
                        ("Native", auto),
                    ] {
                        let choice = choose(ios, jit, force, preference);
                        assert_eq!(
                            choice.kind, expected,
                            "ios={ios} jit={jit} force={force} env={preference:?}"
                        );
                        assert_eq!(
                            choice.unknown,
                            matches!(preference, "" | "invalid" | "Native")
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn native_build_and_first_compile_failures_rebuild_pulley_once() {
        for stage in ["Engine::new", "creating linker", "compiling component"] {
            let mut builds = Vec::new();
            let (runtime, kind, reason) = initialize(
                choose(true, true, false, "auto"),
                |kind| {
                    builds.push(kind);
                    if kind == Kind::Native && stage != "compiling component" {
                        Err(stage.into())
                    } else {
                        Ok(kind)
                    }
                },
                |_| Err(stage.into()),
            )
            .unwrap();
            assert_eq!(builds, [Kind::Native, Kind::Pulley]);
            assert_eq!(runtime, Kind::Pulley);
            assert_eq!(kind, Kind::Pulley);
            assert!(reason.contains(stage));
        }
    }

    #[test]
    fn fallback_failure_is_returned_without_retry() {
        let mut builds = Vec::new();
        let error = initialize(
            choose(false, false, false, "native"),
            |kind| {
                builds.push(kind);
                Err::<(), _>(format!("{} refused", kind.name()))
            },
            |_| unreachable!(),
        )
        .unwrap_err();
        assert_eq!(builds, [Kind::Native, Kind::Pulley]);
        assert!(error.contains("native refused") && error.contains("pulley refused"));
    }

    #[test]
    fn successful_native_and_forced_pulley_do_not_retry() {
        for preference in ["native", "pulley"] {
            let mut builds = Vec::new();
            let mut probes = 0;
            let (runtime, kind, _) = initialize(
                choose(false, false, false, preference),
                |kind| {
                    builds.push(kind);
                    Ok(kind)
                },
                |_| {
                    probes += 1;
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(builds, [kind]);
            assert_eq!(runtime, kind);
            assert_eq!(probes, usize::from(kind == Kind::Native));
        }
    }

    #[test]
    fn later_compile_failure_keeps_the_finalized_native_engine() {
        let cache = Cache::default();
        let wasm = b"\0asm\x0d\0\x01\0";
        let first = cache
            .compile(wasm, || choose(false, false, false, "native"))
            .unwrap();
        assert!(!first.engine.is_pulley());
        assert!(
            cache
                .compile(b"bad later component", || panic!("no reselection"))
                .is_err()
        );
        let reloaded = cache.compile(wasm, || panic!("no reselection")).unwrap();
        assert!(Engine::same(first.engine, reloaded.engine));
    }

    #[test]
    fn bad_first_mod_does_not_poison_pulley_and_reload_keeps_engine() {
        let cache = Cache::default();
        assert!(
            cache
                .compile(b"bad component", || choose(false, false, false, "native"))
                .is_err()
        );
        // Empty binary component: no WAT feature or guest build needed.
        let wasm = b"\0asm\x0d\0\x01\0";
        let compiled = cache
            .compile(wasm, || panic!("selection already finalized"))
            .unwrap();
        assert!(compiled.engine.is_pulley());
        let mut store = wasmtime::Store::new(compiled.engine, super::super::entity_tests::state());
        compiled
            .linker
            .instantiate(&mut store, &compiled.component)
            .unwrap();
        assert!(
            cache
                .compile(b"bad reload", || panic!("no reselection"))
                .is_err()
        );
        let reloaded = cache.compile(wasm, || panic!("no reselection")).unwrap();
        assert!(Engine::same(compiled.engine, reloaded.engine));
        reloaded
            .linker
            .instantiate(&mut store, &reloaded.component)
            .unwrap();
    }
}

#[cfg(test)]
mod limit_tests {
    use super::super::{Mods, entity_tests::state, reason};
    use super::*;
    use std::time::Instant;

    // Independent core Wasm fixtures: a decrementing loop returning zero,
    // or an unconditional back-edge that can never return.
    fn loop_module(engine: &Engine, infinite: bool) -> wasmtime::Module {
        let mut bytes =
            b"\0asm\x01\0\0\0\x01\x06\x01\x60\x01\x7f\x01\x7f\x03\x02\x01\0\x07\x07\x01\x03run\0\0"
                .to_vec();
        let body: &[u8] = if infinite {
            &[0, 0x03, 0x40, 0x0c, 0, 0x0b, 0x41, 0, 0x0b]
        } else {
            &[
                0, 0x03, 0x40, 0x20, 0, 0x41, 1, 0x6b, 0x22, 0, 0x0d, 0, 0x0b, 0x20, 0, 0x0b,
            ]
        };
        bytes.extend([10, (body.len() + 2) as u8, 1, body.len() as u8]);
        bytes.extend_from_slice(body);
        wasmtime::Module::new(engine, bytes).unwrap()
    }

    #[test]
    fn dropping_mods_joins_its_ticker_even_if_an_engine_clone_survives() {
        let mods = Mods::with_limit(LimitMode::Epoch);
        let cache = mods.engine_cache.as_ref().unwrap();
        let compiled = cache.compile_env(b"\0asm\x0d\0\x01\0").unwrap();
        let engine = compiled.engine.clone();
        let finished = cache
            .runtime
            .get()
            .unwrap()
            .as_ref()
            .unwrap()
            ._ticker
            .as_ref()
            .unwrap()
            .finished
            .clone();
        let weak = Arc::downgrade(cache);
        drop(compiled);
        drop(mods);
        assert!(weak.upgrade().is_none());
        assert!(
            finished.load(Ordering::Acquire),
            "drop joined the epoch thread"
        );
        drop(engine);
    }

    #[test]
    fn limits_trap_runaway_loops_but_allow_finite_work_and_rearm() {
        for kind in [Kind::Native, Kind::Pulley] {
            for mode in [LimitMode::Epoch, LimitMode::Fuel] {
                let runtime = Runtime::new(kind, mode).unwrap();
                let units = if mode == LimitMode::Epoch {
                    50_000_000
                } else {
                    1_000_000
                };
                let module = loop_module(&runtime.engine, false);
                let mut store = wasmtime::Store::new(&runtime.engine, state());
                mode.start(&mut store, 2_000_000_000).unwrap();
                let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
                let run = instance
                    .get_typed_func::<i32, i32>(&mut store, "run")
                    .unwrap();
                assert_eq!(run.call(&mut store, 10_000).unwrap(), 0);
                let spin_module = loop_module(&runtime.engine, true);
                let spin_instance = wasmtime::Instance::new(&mut store, &spin_module, &[]).unwrap();
                let spin = spin_instance
                    .get_typed_func::<i32, i32>(&mut store, "run")
                    .unwrap();
                let started = Instant::now();
                mode.start(&mut store, units).unwrap();
                let error = spin.call(&mut store, 0).unwrap_err();
                assert_eq!(reason(&error), "it took too long (an endless loop?)");
                if mode == LimitMode::Epoch {
                    assert!(
                        started.elapsed() <= Duration::from_millis(550),
                        "epoch overrun: {:?}",
                        started.elapsed()
                    );
                    assert!(
                        store.get_fuel().is_err(),
                        "epoch engines have no fuel accounting"
                    );
                } else {
                    assert_eq!(mode.remaining(&store), 0);
                }
                mode.start(&mut store, units).unwrap();
                assert_eq!(run.call(&mut store, 10_000).unwrap(), 0);
            }
        }
    }

    #[test]
    #[ignore = "timing comparison: run alone with --nocapture"]
    fn pulley_loop_throughput() {
        for mode in [LimitMode::Fuel, LimitMode::Epoch] {
            let runtime = Runtime::new(Kind::Pulley, mode).unwrap();
            let module = loop_module(&runtime.engine, false);
            let mut store = wasmtime::Store::new(&runtime.engine, state());
            mode.start(&mut store, 2_000_000_000).unwrap();
            let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
            let run = instance
                .get_typed_func::<i32, i32>(&mut store, "run")
                .unwrap();
            let mut times = Vec::new();
            for i in 0..25 {
                mode.start(&mut store, 2_000_000_000).unwrap();
                let started = Instant::now();
                assert_eq!(run.call(&mut store, 1_000_000).unwrap(), 0);
                if i >= 5 {
                    times.push(started.elapsed());
                }
            }
            times.sort();
            let median = times[times.len() / 2];
            println!(
                "pulley limit {}: 1M iterations in {median:?}, {:.0} iterations/s",
                mode.name(),
                1_000_000.0 / median.as_secs_f64()
            );
        }
    }
}
