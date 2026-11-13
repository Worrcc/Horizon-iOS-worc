//! One engine for the process, finalized before any mod can run.
use super::{State, linker};
use std::sync::OnceLock;
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

struct Runtime {
    engine: Engine,
    linker: Linker<State>,
}
impl Runtime {
    fn new(kind: Kind) -> Result<Self, String> {
        let mut config = Config::new();
        config.wasm_component_model(true).consume_fuel(true);
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
        Ok(Self { engine, linker })
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
struct Cache(OnceLock<Result<Runtime, String>>);
impl Cache {
    fn compile(
        &self,
        wasm: &[u8],
        choice: impl FnOnce() -> Choice,
    ) -> Result<Compiled<'_>, String> {
        let mut first = None;
        let runtime = self
            .0
            .get_or_init(|| {
                let (runtime, kind, reason) = initialize(choice(), Runtime::new, |runtime| {
                    first = Some(runtime.compile(wasm)?);
                    Ok(())
                })?;
                log::info!("Horizon: mods run on {} ({reason})", kind.name());
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

pub(super) fn compile(wasm: &[u8]) -> Result<Compiled<'static>, String> {
    // Shared by client/server instances and dev/world reloads: only one
    // selection log and at most one native-to-Pulley transition per process.
    static CACHE: Cache = Cache(OnceLock::new());
    CACHE.compile(wasm, || {
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
    })
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
