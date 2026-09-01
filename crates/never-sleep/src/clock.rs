use chrono::Local;

use never_sleep_core::HostSnapshot;

pub fn monotonic_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Clock that keeps running during system sleep and is immune to NTP.
///
/// macOS: `mach_continuous_time`. Linux stub: same as [`monotonic_ms`].
pub fn continuous_ms() -> u64 {
    #[cfg(target_os = "macos")]
    {
        mach_continuous_ms()
    }
    #[cfg(not(target_os = "macos"))]
    {
        monotonic_ms()
    }
}

#[cfg(target_os = "macos")]
fn mach_continuous_ms() -> u64 {
    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }
    extern "C" {
        fn mach_continuous_time() -> u64;
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> i32;
    }
    use std::sync::OnceLock;
    static TIMEBASE: OnceLock<(u32, u32)> = OnceLock::new();
    let (numer, denom) = *TIMEBASE.get_or_init(|| {
        let mut info = MachTimebaseInfo { numer: 0, denom: 1 };
        unsafe {
            mach_timebase_info(&mut info);
        }
        (info.numer, info.denom.max(1))
    });
    let ticks = unsafe { mach_continuous_time() };
    ((u128::from(ticks) * u128::from(numer)) / u128::from(denom) / 1_000_000) as u64
}

pub fn utc_offset_secs() -> i32 {
    Local::now().offset().local_minus_utc()
}

pub fn unix_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

pub fn base_snapshot(monotonic: u64) -> HostSnapshot {
    HostSnapshot {
        monotonic_ms: monotonic,
        continuous_ms: continuous_ms(),
        unix_secs: unix_secs(),
        utc_offset_secs: utc_offset_secs(),
        on_ac: true,
        battery_percent: None,
        lid_closed: false,
        display_asleep: None,
        hid_idle_ms: 0,
        thermal: never_sleep_core::Thermal::Nominal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_snapshot_includes_continuous_ms() {
        let snap = base_snapshot(monotonic_ms());
        #[cfg(not(target_os = "macos"))]
        assert!(
            snap.continuous_ms.abs_diff(snap.monotonic_ms) < 50,
            "Linux stub uses Instant for both clocks"
        );
        #[cfg(target_os = "macos")]
        assert!(snap.continuous_ms > 0);
    }
}
