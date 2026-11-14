//! One sleep to an absolute deadline.
//!
//! A relative sleep can be stretched by the OS's timer coalescing (macOS runs
//! them up to a quarter of their length late); a sleep to an absolute deadline
//! on the monotonic clock is woken at that time, so a thread that paces frames
//! wakes once per frame instead of once per millisecond.

use std::time::Instant;

/// Whether [`sleep_until`] sleeps to an absolute deadline on this system.
pub const ABSOLUTE: bool = cfg!(any(
    target_vendor = "apple",
    target_os = "linux",
    target_os = "android"
));

/// Sleeps once until `deadline`; returns at once if it has passed.
pub fn sleep_until(deadline: Instant) {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return;
    }
    sleep_for_deadline(left);
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn sleep_for_deadline(left: std::time::Duration) {
    // SAFETY: `now` is a plain C struct that `clock_gettime` fills in; `clock_nanosleep` reads
    // `ts` and, with a null remainder, writes nothing. It returns the error number itself.
    unsafe {
        let mut now: libc::timespec = std::mem::zeroed();
        if libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now) != 0 {
            std::thread::sleep(left);
            return;
        }
        let mut nanos = now.tv_nsec as i64 + i64::from(left.subsec_nanos());
        let mut secs = now.tv_sec as i64 + left.as_secs() as i64;
        if nanos >= 1_000_000_000 {
            nanos -= 1_000_000_000;
            secs += 1;
        }
        let ts = libc::timespec {
            tv_sec: secs as libc::time_t,
            tv_nsec: nanos as _,
        };
        loop {
            let r = libc::clock_nanosleep(
                libc::CLOCK_MONOTONIC,
                libc::TIMER_ABSTIME,
                &ts,
                std::ptr::null_mut(),
            );
            if r != libc::EINTR {
                break;
            }
        }
    }
}

#[cfg(target_vendor = "apple")]
fn sleep_for_deadline(left: std::time::Duration) {
    use std::sync::OnceLock;

    #[repr(C)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }
    // libc declares these only for macOS, but iOS exports them too.
    unsafe extern "C" {
        fn mach_absolute_time() -> u64;
        fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
        fn mach_wait_until(deadline: u64) -> libc::c_int;
    }
    const KERN_ABORTED: libc::c_int = 14;
    static BASE: OnceLock<(u32, u32)> = OnceLock::new();
    let (numer, denom) = *BASE.get_or_init(|| {
        let mut info = Timebase { numer: 1, denom: 1 };
        // SAFETY: `info` is a writable C struct of the declared layout.
        let ok = unsafe { mach_timebase_info(&mut info) } == 0 && info.numer != 0;
        if ok { (info.numer, info.denom) } else { (1, 1) }
    });
    let ticks = left.as_nanos() * u128::from(denom) / u128::from(numer);
    // SAFETY: these Mach calls take and return plain integers.
    unsafe {
        let target = mach_absolute_time().saturating_add(ticks.min(u128::from(u64::MAX)) as u64);
        while mach_wait_until(target) == KERN_ABORTED && mach_absolute_time() < target {}
    }
}

#[cfg(not(any(target_vendor = "apple", target_os = "linux", target_os = "android")))]
fn sleep_for_deadline(left: std::time::Duration) {
    std::thread::sleep(left);
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sleeps_to_the_deadline_and_not_much_past_it() {
        for _ in 0..5 {
            let deadline = Instant::now() + Duration::from_millis(5);
            sleep_until(deadline);
            let late = Instant::now().saturating_duration_since(deadline);
            assert!(Instant::now() >= deadline, "woke early");
            assert!(late < Duration::from_millis(5), "{late:?} late");
        }
    }
}
