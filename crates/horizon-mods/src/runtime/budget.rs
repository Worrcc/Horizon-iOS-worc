//! Call limits. Epoch work units are nanoseconds: 1,000,000 units = 1 ms.
use super::{State, canvas_host};
use std::time::{Duration, Instant};
use wasmtime::{AsContext, AsContextMut, Config};

/// How an engine bounds guest calls. A store always uses its engine's mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LimitMode {
    /// Wall-clock deadlines, checked at function entries and loop back-edges.
    /// Opt in with HORIZON_MODS_LIMIT=epoch: time spent in host calls counts, so a
    /// mod whose budget was sized in fuel (Diorama's joining frames) can be stopped.
    Epoch,
    /// Instruction accounting: the default, as every mod's budget was sized for it.
    #[default]
    Fuel,
}
impl LimitMode {
    pub(super) fn select(preference: Option<&str>) -> (Self, bool) {
        match preference {
            None | Some("fuel") => (Self::Fuel, false),
            Some("epoch") => (Self::Epoch, false),
            _ => (Self::Fuel, true),
        }
    }
    pub(super) fn from_env() -> Self {
        let preference = std::env::var_os("HORIZON_MODS_LIMIT");
        let preference = preference.as_ref().map(|v| v.to_string_lossy());
        let (mode, unknown) = Self::select(preference.as_deref());
        if unknown {
            log::warn!("Horizon: unknown HORIZON_MODS_LIMIT={preference:?}; using fuel");
        }
        mode
    }
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Epoch => "epoch",
            Self::Fuel => "fuel",
        }
    }
    pub(super) fn configure(self, config: &mut Config) {
        match self {
            Self::Epoch => {
                config.epoch_interruption(true);
            }
            Self::Fuel => {
                config.consume_fuel(true);
            }
        }
    }
    pub(super) fn start(
        self,
        mut store: impl AsContextMut<Data = State>,
        units: u64,
    ) -> wasmtime::Result<()> {
        let mut store = store.as_context_mut();
        match self {
            Self::Epoch => {
                let ticks = ticks(units);
                store.epoch_deadline_trap();
                store.set_epoch_deadline(ticks);
                store.data_mut().call_budget = CallBudget {
                    mode: self,
                    started: Some(Instant::now()),
                    units: ticks.saturating_mul(UNITS_PER_MS),
                };
            }
            Self::Fuel => {
                store.set_fuel(units)?;
                store.data_mut().call_budget = CallBudget {
                    mode: self,
                    started: None,
                    units,
                };
            }
        }
        Ok(())
    }
    /// The guest API keeps its normalized scale in either accounting mode.
    pub(super) fn work_left(self, store: impl AsContext<Data = State>) -> u64 {
        let store = store.as_context();
        let data = store.data();
        match self {
            Self::Epoch => data.call_budget.started.map_or(0, |start| {
                epoch_work_left(data.canvas.work, start.elapsed())
            }),
            Self::Fuel => canvas_host::work_left(data.canvas.work, self.remaining(&store)),
        }
    }
    pub(super) fn remaining(self, store: impl AsContext<Data = State>) -> u64 {
        let store = store.as_context();
        match self {
            Self::Fuel => store.get_fuel().unwrap_or(0),
            Self::Epoch => store.data().call_budget.remaining(),
        }
    }
}
const UNITS_PER_MS: u64 = 1_000_000;
fn ticks(units: u64) -> u64 {
    (units / UNITS_PER_MS).max(1)
}
fn remaining_after(units: u64, elapsed: Duration) -> u64 {
    units.saturating_sub(elapsed.as_nanos().min(u128::from(u64::MAX)) as u64)
}

fn epoch_work_left(work: canvas_host::Work, elapsed: Duration) -> u64 {
    // Elapsed time already includes host work; charging it again would make
    // chunk reads and image uploads shorten the grant twice.
    let remaining = remaining_after(work.soft, elapsed);
    canvas_host::work_left(
        canvas_host::Work {
            start: work.soft,
            host: 0,
            ..work
        },
        remaining,
    )
}

#[derive(Default)]
pub(super) struct CallBudget {
    pub mode: LimitMode,
    started: Option<Instant>,
    units: u64,
}
impl CallBudget {
    fn remaining(&self) -> u64 {
        self.remaining_within(self.units)
    }
    /// Soft frame grants may be shorter than one epoch tick. Use the clock
    /// directly so cooperative guests can yield within those grants too.
    fn remaining_within(&self, units: u64) -> u64 {
        self.started
            .map_or(0, |start| remaining_after(units, start.elapsed()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_and_conversion() {
        for (input, mode, unknown) in [
            (None, LimitMode::Fuel, false),
            (Some("epoch"), LimitMode::Epoch, false),
            (Some("fuel"), LimitMode::Fuel, false),
            (Some(""), LimitMode::Fuel, true),
            (Some("Fuel"), LimitMode::Fuel, true),
        ] {
            assert_eq!(LimitMode::select(input), (mode, unknown));
        }
        assert_eq!(ticks(0), 1);
        assert_eq!(ticks(999_999), 1);
        assert_eq!(ticks(1_999_999), 1);
        assert_eq!(ticks(50_000_000), 50);
        assert_eq!(
            remaining_after(1_000_000, Duration::from_micros(250)),
            750_000
        );
        assert_eq!(remaining_after(100_000, Duration::from_nanos(1)), 99_999);
        assert_eq!(remaining_after(1_000_000, Duration::from_secs(5)), 0);
        assert_eq!(remaining_after(1, Duration::MAX), 0);
    }
    #[test]
    fn epoch_frame_work_is_normalized_submillisecond_and_counts_host_time_once() {
        let work = canvas_host::Work {
            start: 10_000_000,
            soft: 300_000, // A 300 µs frame grant.
            host: 80_000,
            host_time: Duration::from_micros(80),
        };
        assert_eq!(epoch_work_left(work, Duration::ZERO), 1_000_000);
        assert_eq!(epoch_work_left(work, Duration::from_micros(100)), 666_666);
        assert_eq!(epoch_work_left(work, Duration::from_micros(299)), 3_333);
        assert_eq!(epoch_work_left(work, Duration::from_micros(300)), 0);
        assert_eq!(epoch_work_left(work, Duration::from_secs(1)), 0);
    }
}
