//! Read-only process CPU, wake-up and (on iOS) energy counters, for the `perf:` log line.

use std::time::Duration;

/// A reading of the process's cumulative counters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Usage {
    /// User plus system CPU time of the whole process.
    pub cpu: Duration,
    /// Voluntary plus involuntary context switches.
    pub context_switches: u64,
    /// The kernel's power accounting, where the OS reports it.
    pub power: Option<Power>,
}

/// The kernel's per-task power counters.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Power {
    /// The task's energy estimate in nanojoules; arm64 only.
    pub energy_nj: Option<u64>,
    /// Interrupt wake-ups of the package.
    pub interrupt_wakeups: u64,
    /// Package idle exits.
    pub idle_wakeups: u64,
    /// The task's GPU time in nanoseconds.
    pub gpu_ns: u64,
}

/// The process's cumulative counters, or `None` where the OS has no way to read them.
#[cfg(any(target_vendor = "apple", target_os = "linux", target_os = "android"))]
pub fn sample() -> Option<Usage> {
    // SAFETY: all-zero is a valid value of this plain C struct, and the call writes only into it.
    let ru = unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut ru) != 0 {
            return None;
        }
        ru
    };
    let time = |t: libc::timeval| {
        Duration::new(
            t.tv_sec.max(0) as u64,
            (t.tv_usec.max(0) as u32).saturating_mul(1000),
        )
    };
    Some(Usage {
        cpu: time(ru.ru_utime) + time(ru.ru_stime),
        context_switches: (ru.ru_nvcsw.max(0) as u64) + (ru.ru_nivcsw.max(0) as u64),
        power: power(),
    })
}

/// The process's cumulative counters: not readable on this system.
#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
pub fn sample() -> Option<Usage> {
    None
}

#[cfg(not(target_os = "ios"))]
#[cfg_attr(
    not(any(target_vendor = "apple", target_os = "linux", target_os = "android")),
    allow(dead_code)
)]
fn power() -> Option<Power> {
    None
}

/// `struct task_power_info` from `<mach/task_info.h>`.
#[cfg(target_os = "ios")]
#[repr(C)]
struct TaskPowerInfo {
    total_user: u64,
    total_system: u64,
    task_interrupt_wakeups: u64,
    task_platform_idle_wakeups: u64,
    task_timer_wakeups_bin_1: u64,
    task_timer_wakeups_bin_2: u64,
}

/// `struct gpu_energy_data` from `<mach/task_info.h>`.
#[cfg(target_os = "ios")]
#[repr(C)]
struct GpuEnergyData {
    task_gpu_utilisation: u64,
    task_gpu_stat_reserved0: u64,
    task_gpu_stat_reserved1: u64,
    task_gpu_stat_reserved2: u64,
}

/// `struct task_power_info_v2` from `<mach/task_info.h>`.
#[cfg(target_os = "ios")]
#[repr(C)]
struct TaskPowerInfoV2 {
    cpu_energy: TaskPowerInfo,
    gpu_energy: GpuEnergyData,
    #[cfg(target_arch = "aarch64")]
    task_energy: u64,
    #[cfg(target_arch = "aarch64")]
    task_ptime: u64,
    #[cfg(target_arch = "aarch64")]
    task_pset_switches: u64,
}

/// The kernel's power accounting for this task (`TASK_POWER_INFO_V2`).
#[cfg(target_os = "ios")]
fn power() -> Option<Power> {
    // libc exposes this Mach symbol only on macOS, but iOS exports it too.
    unsafe extern "C" {
        static mach_task_self_: libc::mach_port_t;
    }
    const TASK_POWER_INFO_V2: libc::task_flavor_t = 26;
    let full = (std::mem::size_of::<TaskPowerInfoV2>() / std::mem::size_of::<libc::natural_t>())
        as libc::mach_msg_type_number_t;
    // SAFETY: the task is our own; all-zero is a valid value of this plain C struct, and `count`
    // describes its writable size in integer units.
    unsafe {
        let mut info: TaskPowerInfoV2 = std::mem::zeroed();
        let mut count = full;
        let result = libc::task_info(
            mach_task_self_,
            TASK_POWER_INFO_V2,
            (&raw mut info).cast(),
            &mut count,
        );
        if result != libc::KERN_SUCCESS {
            return None;
        }
        #[cfg(target_arch = "aarch64")]
        let energy_nj = (count == full).then_some(info.task_energy);
        #[cfg(not(target_arch = "aarch64"))]
        let energy_nj = None;
        Some(Power {
            energy_nj,
            interrupt_wakeups: info.cpu_energy.task_interrupt_wakeups,
            idle_wakeups: info.cpu_energy.task_platform_idle_wakeups,
            gpu_ns: info.gpu_energy.task_gpu_utilisation,
        })
    }
}

/// Per-second rates between two readings.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rates {
    pub cpu_ms_per_s: f64,
    pub context_switches_per_s: f64,
    pub energy_mw: Option<f64>,
    pub idle_wakeups_per_s: Option<f64>,
    pub interrupt_wakeups_per_s: Option<f64>,
    pub gpu_ms_per_s: Option<f64>,
}

/// The rates between two readings taken `elapsed` apart; `None` when no time passed.
pub fn rates(before: &Usage, after: &Usage, elapsed: Duration) -> Option<Rates> {
    let secs = elapsed.as_secs_f64();
    if secs <= 0.0 {
        return None;
    }
    let per_s = |delta: u64| delta as f64 / secs;
    let (power_before, power_after) = match (before.power, after.power) {
        (Some(b), Some(a)) => (Some(b), Some(a)),
        _ => (None, None),
    };
    let energy_mw = match (power_before, power_after) {
        (Some(b), Some(a)) => match (b.energy_nj, a.energy_nj) {
            (Some(b), Some(a)) => Some(per_s(a.saturating_sub(b)) / 1e6),
            _ => None,
        },
        _ => None,
    };
    Some(Rates {
        cpu_ms_per_s: after.cpu.saturating_sub(before.cpu).as_secs_f64() * 1000.0 / secs,
        context_switches_per_s: per_s(
            after
                .context_switches
                .saturating_sub(before.context_switches),
        ),
        energy_mw,
        idle_wakeups_per_s: power_before
            .zip(power_after)
            .map(|(b, a)| per_s(a.idle_wakeups.saturating_sub(b.idle_wakeups))),
        interrupt_wakeups_per_s: power_before
            .zip(power_after)
            .map(|(b, a)| per_s(a.interrupt_wakeups.saturating_sub(b.interrupt_wakeups))),
        gpu_ms_per_s: power_before
            .zip(power_after)
            .map(|(b, a)| per_s(a.gpu_ns.saturating_sub(b.gpu_ns)) / 1e6),
    })
}

#[cfg(test)]
mod rate_tests {
    use super::*;

    #[test]
    fn rates_are_per_second_of_wall_time() {
        let power = |energy_nj, idle, intr, gpu_ns| {
            Some(Power {
                energy_nj,
                interrupt_wakeups: intr,
                idle_wakeups: idle,
                gpu_ns,
            })
        };
        let before = Usage {
            cpu: Duration::from_millis(100),
            context_switches: 10,
            power: power(Some(1_000_000_000), 100, 40, 0),
        };
        let after = Usage {
            cpu: Duration::from_millis(600),
            context_switches: 1010,
            power: power(Some(3_000_000_000), 300, 140, 600_000_000),
        };
        let r = rates(&before, &after, Duration::from_secs(2)).unwrap();
        assert_eq!(r.cpu_ms_per_s, 250.0);
        assert_eq!(r.context_switches_per_s, 500.0);
        assert_eq!(r.energy_mw, Some(1000.0));
        assert_eq!(r.idle_wakeups_per_s, Some(100.0));
        assert_eq!(r.interrupt_wakeups_per_s, Some(50.0));
        assert_eq!(r.gpu_ms_per_s, Some(300.0));
    }

    #[test]
    fn missing_power_on_one_side_leaves_the_power_fields_out() {
        let with = Usage {
            power: Some(Power::default()),
            ..Usage::default()
        };
        let without = Usage::default();
        let r = rates(&without, &with, Duration::from_secs(1)).unwrap();
        assert_eq!(
            (
                r.energy_mw,
                r.idle_wakeups_per_s,
                r.interrupt_wakeups_per_s,
                r.gpu_ms_per_s
            ),
            (None, None, None, None)
        );
    }

    #[test]
    fn no_elapsed_time_gives_no_rates() {
        let u = Usage::default();
        assert_eq!(rates(&u, &u, Duration::ZERO), None);
    }
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn busy_work_counts_as_cpu_time() {
        let before = sample().unwrap();
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(150) {
            std::hint::black_box(0u64);
        }
        let after = sample().unwrap();
        let grew = after.cpu.saturating_sub(before.cpu);
        assert!(grew >= Duration::from_millis(100), "grew {grew:?}");
    }

    #[test]
    fn sleeping_counts_as_context_switches() {
        let before = sample().unwrap();
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(1));
        }
        let after = sample().unwrap();
        let grew = after
            .context_switches
            .saturating_sub(before.context_switches);
        assert!(grew >= 10, "grew {grew}");
    }
}
