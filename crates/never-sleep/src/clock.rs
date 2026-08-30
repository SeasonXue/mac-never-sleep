use chrono::Local;

use never_sleep_core::HostSnapshot;

pub fn monotonic_ms() -> u64 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
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
