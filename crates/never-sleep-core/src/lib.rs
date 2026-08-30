//! 熄屏待命的策略核心：与 macOS IOKit 解耦，便于单测。
//!
//! 设计原则见仓库 README。这里只回答三个问题：
//! 1. 现在要不要阻止系统睡眠、要不要关屏？
//! 2. 用户算不算「坐在电脑前」？
//! 3. 该不该因为电量 / 时长 / 过热而结束待命？

mod config;
mod duration;
mod engine;
mod status;
mod strings;

pub use config::{parse_duration_pref, AppConfig, DurationPref};
pub use duration::{
    deadline_unix_secs, format_duration_zh, next_until_unix_secs, next_until_wallclock,
};
pub use engine::{Effect, Engine, Input, PowerPlan, StopReason};
pub use status::{HostSnapshot, JsonStatus, Thermal, ViewModel};
pub use strings::{APP_DISPLAY_NAME, APP_NAME, BUNDLE_ID, DEFAULT_HOTKEY_LABEL, ONBOARDING};

pub const DEFAULT_BATTERY_FLOOR: u8 = 20;
pub const DEFAULT_DISPLAY_OFF_DELAY_MS: u64 = 1_500;
pub const DEFAULT_USER_IDLE_RESLEEP_MS: u64 = 45_000;
pub const SLEEP_DISPLAY_DEBOUNCE_MS: u64 = 3_000;
pub const HEARTBEAT_MS: u64 = 2_000;
