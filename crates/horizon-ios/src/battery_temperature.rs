//! Best-effort battery diagnostics; cache failures too, since the sandbox usually denies access.
use std::time::{Duration, Instant};

#[derive(Default)]
struct Cache {
    sample: Option<(Instant, Option<f32>)>,
}
impl Cache {
    fn read(&mut self, now: Instant, read: impl FnOnce() -> Option<f32>) -> Option<f32> {
        if let Some((last, value)) = self.sample {
            if now.saturating_duration_since(last) < Duration::from_secs(5) {
                return value;
            }
        }
        let value = read();
        self.sample = Some((now, value));
        value
    }
}

fn celsius(hundredths: f64) -> Option<f32> {
    let degrees = (hundredths / 100.0) as f32;
    degrees.is_finite().then_some(degrees)
}

#[cfg(target_os = "ios")]
pub(super) fn read() -> Option<f32> {
    // Shared by the overlay and log, including callers on different threads.
    static CACHE: std::sync::Mutex<Cache> = std::sync::Mutex::new(Cache { sample: None });
    CACHE.lock().ok()?.read(Instant::now(), platform::read)
}

#[cfg(target_os = "ios")]
mod platform {
    use core_foundation_sys::{
        base::{CFAllocatorRef, CFGetTypeID, CFRelease, CFTypeRef},
        dictionary::{CFDictionaryRef, CFMutableDictionaryRef},
        number::{CFNumberGetTypeID, CFNumberGetValue, kCFNumberDoubleType},
        string::{CFStringCreateWithCString, CFStringRef, kCFStringEncodingUTF8},
    };
    use std::{ffi::c_char, ptr};

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOServiceMatching(name: *const c_char) -> CFMutableDictionaryRef;
        fn IOServiceGetMatchingService(main_port: u32, matching: CFDictionaryRef) -> u32;
        fn IORegistryEntryCreateCFProperty(
            entry: u32,
            key: CFStringRef,
            allocator: CFAllocatorRef,
            options: u32,
        ) -> CFTypeRef;
        fn IOObjectRelease(object: u32) -> i32;
    }

    pub(super) fn read() -> Option<f32> {
        // SAFETY: matching consumes its dictionary; all returned objects are checked before use
        // and released. A property must be a CFNumber before conversion into writable f64 storage.
        unsafe {
            let matching = IOServiceMatching(c"AppleSmartBattery".as_ptr());
            if matching.is_null() {
                return None;
            }
            // kIOMainPortDefault: ask IOKit for the default port, without creating one.
            let service = IOServiceGetMatchingService(0, matching);
            if service == 0 {
                return None;
            }
            let value = (|| {
                let key = CFStringCreateWithCString(
                    ptr::null(),
                    c"Temperature".as_ptr(),
                    kCFStringEncodingUTF8,
                );
                if key.is_null() {
                    return None;
                }
                let property = IORegistryEntryCreateCFProperty(service, key, ptr::null(), 0);
                CFRelease(key.cast());
                if property.is_null() {
                    return None;
                }
                let mut hundredths = 0.0_f64;
                let valid = CFGetTypeID(property) == CFNumberGetTypeID()
                    && CFNumberGetValue(
                        property.cast(),
                        kCFNumberDoubleType,
                        (&raw mut hundredths).cast(),
                    );
                CFRelease(property);
                valid.then(|| super::celsius(hundredths)).flatten()
            })();
            IOObjectRelease(service);
            value
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_and_available_readings_are_cached_for_five_seconds() {
        let mut cache = Cache::default();
        let start = Instant::now();
        assert_eq!(cache.read(start, || None), None);
        assert_eq!(
            cache.read(start + Duration::from_millis(4999), || Some(34.5)),
            None
        );
        assert_eq!(
            cache.read(start + Duration::from_secs(5), || Some(34.5)),
            Some(34.5)
        );
        assert_eq!(
            cache.read(start + Duration::from_millis(9999), || None),
            Some(34.5)
        );
        assert_eq!(cache.read(start + Duration::from_secs(10), || None), None);
    }

    #[test]
    fn converts_hundredths_and_rejects_nonfinite_readings() {
        assert_eq!(celsius(3450.0), Some(34.5));
        for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert_eq!(celsius(invalid), None);
        }
    }
}
