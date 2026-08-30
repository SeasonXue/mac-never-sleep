use serde::{Deserialize, Serialize};

use crate::duration::format_duration;
use crate::{AppConfig, DurationPref};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Thermal {
    Nominal,
    Fair,
    Serious,
    Critical,
}

impl Thermal {
    pub fn is_emergency(self) -> bool {
        matches!(self, Self::Critical)
    }
}

/// Instantaneous host sample. Shared by GUI and CLI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSnapshot {
    pub monotonic_ms: u64,
    pub unix_secs: i64,
    /// Seconds east of UTC.
    pub utc_offset_secs: i32,
    pub on_ac: bool,
    pub battery_percent: Option<u8>,
    pub lid_closed: bool,
    /// `None` if we cannot tell; the engine falls back to its optimistic flag.
    pub display_asleep: Option<bool>,
    pub hid_idle_ms: u64,
    pub thermal: Thermal,
}

impl HostSnapshot {
    pub fn user_present(&self, idle_threshold_ms: u64) -> bool {
        !self.lid_closed && self.hid_idle_ms < idle_threshold_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonStatus {
    pub active: bool,
    pub display: String,
    pub lid: String,
    pub on_ac: bool,
    pub battery: Option<u8>,
    pub remaining_secs: Option<u64>,
    pub user_present: bool,
    pub elapsed_secs: Option<u64>,
    pub stop_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop_reason_code: Option<String>,
    pub screen_off_enabled: bool,
    pub lid_awake_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewModel {
    pub active: bool,
    pub status_line: String,
    pub detail_line: String,
    pub primary_action: String,
    pub tooltip: String,
    pub remaining_label: Option<String>,
    pub warnings: Vec<String>,
    pub duration: DurationPref,
    pub screen_off: bool,
    pub keep_awake_on_lid_close: bool,
    pub resleep_display: bool,
    pub lock_screen: bool,
    pub launch_at_login: bool,
    pub battery_floor_label: String,
}

pub fn build_view_model(
    cfg: &AppConfig,
    active: bool,
    started_ms: Option<u64>,
    deadline_unix: Option<i64>,
    host: &HostSnapshot,
    last_stop: Option<&str>,
    display_asleep: bool,
) -> ViewModel {
    let t = cfg.tr();
    let lang = cfg.lang();
    let mut warnings = Vec::new();
    if active && cfg.keep_awake_on_lid_close && !host.on_ac {
        warnings.push(t.warn_lid_on_battery().into());
    }
    if active && host.lid_closed && cfg.keep_awake_on_lid_close {
        warnings.push(t.warn_lid_best_effort().into());
    }
    if !active {
        if let Some(reason) = last_stop {
            warnings.push(reason.to_string());
        }
    }

    let elapsed = started_ms.map(|s| host.monotonic_ms.saturating_sub(s) / 1000);
    let remaining = deadline_unix.map(|d| d.saturating_sub(host.unix_secs) as u64);

    let status_line = if active {
        match elapsed {
            Some(s) => t.standby_elapsed(&format_duration(lang, s)),
            None => t.standby_status().into(),
        }
    } else {
        t.idle_status().into()
    };

    let mut details: Vec<String> = Vec::new();
    if active {
        details.push(
            if display_asleep {
                t.display_asleep()
            } else if host.user_present(cfg.user_idle_resleep_ms) {
                t.user_controls_display()
            } else {
                t.display_pending()
            }
            .into(),
        );
        details.push(
            if host.lid_closed {
                t.lid_closed()
            } else {
                t.lid_open()
            }
            .into(),
        );
        details.push(
            if host.on_ac {
                t.power_ac()
            } else {
                t.power_battery()
            }
            .into(),
        );
        if let Some(b) = host.battery_percent {
            details.push(t.battery_percent(b));
        }
        if let Some(r) = remaining {
            details.push(t.remaining(&format_duration(lang, r)));
        }
    } else {
        details.push(
            if cfg.screen_off {
                t.will_sleep_display()
            } else {
                t.will_keep_awake_only()
            }
            .into(),
        );
    }

    let detail_line = details.join(" · ");

    let primary_action = if active {
        t.end_standby().into()
    } else {
        t.start_standby().into()
    };

    let tooltip = if active {
        t.tooltip_active()
    } else {
        t.tooltip_idle()
    };

    ViewModel {
        active,
        status_line,
        detail_line,
        primary_action,
        tooltip,
        remaining_label: remaining.map(|r| format_duration(lang, r)),
        warnings,
        duration: cfg.duration,
        screen_off: cfg.screen_off,
        keep_awake_on_lid_close: cfg.keep_awake_on_lid_close,
        resleep_display: cfg.resleep_display,
        lock_screen: cfg.lock_screen,
        launch_at_login: cfg.launch_at_login,
        battery_floor_label: cfg.battery_floor_label(),
    }
}
