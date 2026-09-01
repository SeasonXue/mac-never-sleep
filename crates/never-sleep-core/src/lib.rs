//! Session policy for Never Sleep, kept free of macOS IOKit so it can be unit-tested.
//!
//! Design notes are in the repository README. This crate answers three questions:
//! 1. Should we prevent system sleep and sleep the display right now?
//! 2. Is a person sitting at the Mac?
//! 3. Should standby end because of battery, duration, or heat?

mod config;
mod duration;
mod engine;
mod i18n;
mod status;

pub use config::{parse_duration_pref, parse_duration_pref_in, AppConfig, DurationPref};
pub use duration::{
    countdown_secs, deadline_unix_secs, elapsed_secs, format_clock, format_countdown,
    format_duration, next_until_unix_secs, next_until_wallclock, remaining_ms,
    session_remaining_ms,
};
pub use engine::{Effect, Engine, Input, PowerPlan, StopReason};
pub use i18n::{
    app_display_name, onboarding, Lang, Tr, APP_NAME, BUNDLE_ID, DEFAULT_HOTKEY_LABEL, LANG_ENV,
};
pub use status::{HostSnapshot, JsonStatus, Thermal, ViewModel};

pub const DEFAULT_BATTERY_FLOOR: u8 = 20;
pub const DEFAULT_DISPLAY_OFF_DELAY_MS: u64 = 1_500;
pub const DEFAULT_USER_IDLE_RESLEEP_MS: u64 = 45_000;
pub const SLEEP_DISPLAY_DEBOUNCE_MS: u64 = 3_000;
pub const HEARTBEAT_MS: u64 = 2_000;
