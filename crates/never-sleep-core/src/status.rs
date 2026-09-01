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
    /// Clock that keeps running during system sleep and does not jump with NTP.
    /// On macOS this is `mach_continuous_time`; `monotonic_ms` is `Instant`.
    pub continuous_ms: u64,
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
    /// True when IOKit reports the panel is already dark (not the user intent).
    pub display_asleep: bool,
    /// True when HID idle + lid say a person is at the keyboard.
    pub user_present: bool,
    /// Elapsed seconds in the current session; `None` while idle.
    pub elapsed_secs: Option<u64>,
    /// Remaining whole seconds when a deadline is set; `None` while idle or indefinite.
    pub remaining_secs: Option<u64>,
}

pub fn build_view_model(
    cfg: &AppConfig,
    active: bool,
    elapsed_secs: Option<u64>,
    remaining_secs: Option<u64>,
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

    let status_line = if active {
        match elapsed_secs {
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
        if let Some(r) = remaining_secs {
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
        remaining_label: remaining_secs.map(|r| format_duration(lang, r)),
        warnings,
        duration: cfg.duration,
        screen_off: cfg.screen_off,
        keep_awake_on_lid_close: cfg.keep_awake_on_lid_close,
        resleep_display: cfg.resleep_display,
        lock_screen: cfg.lock_screen,
        launch_at_login: cfg.launch_at_login,
        battery_floor_label: cfg.battery_floor_label(),
        display_asleep,
        user_present: host.user_present(cfg.user_idle_resleep_ms),
        elapsed_secs: if active { elapsed_secs } else { None },
        remaining_secs: if active { remaining_secs } else { None },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppConfig, Lang};

    fn host() -> HostSnapshot {
        HostSnapshot {
            monotonic_ms: 5_000,
            continuous_ms: 5_000,
            unix_secs: 1_700_000_000,
            utc_offset_secs: 0,
            on_ac: true,
            battery_percent: Some(64),
            lid_closed: false,
            display_asleep: Some(false),
            hid_idle_ms: 1_000,
            thermal: Thermal::Nominal,
        }
    }

    #[test]
    fn user_present_requires_open_lid_and_recent_hid() {
        let mut h = host();
        assert!(h.user_present(45_000));
        h.lid_closed = true;
        assert!(!h.user_present(45_000));
        h.lid_closed = false;
        h.hid_idle_ms = 80_000;
        assert!(!h.user_present(45_000));
    }

    #[test]
    fn thermal_emergency_is_critical_only() {
        assert!(!Thermal::Nominal.is_emergency());
        assert!(!Thermal::Fair.is_emergency());
        assert!(!Thermal::Serious.is_emergency());
        assert!(Thermal::Critical.is_emergency());
    }

    #[test]
    fn json_status_serde_field_names_are_stable() {
        let st = JsonStatus {
            active: false,
            display: "awake".into(),
            lid: "open".into(),
            on_ac: false,
            battery: Some(40),
            remaining_secs: None,
            user_present: true,
            elapsed_secs: None,
            stop_reason: Some("Ended by you".into()),
            stop_reason_code: Some("user".into()),
            screen_off_enabled: true,
            lid_awake_enabled: false,
        };
        let v = serde_json::to_value(&st).unwrap();
        for key in [
            "active",
            "display",
            "lid",
            "on_ac",
            "battery",
            "remaining_secs",
            "user_present",
            "elapsed_secs",
            "stop_reason",
            "stop_reason_code",
            "screen_off_enabled",
            "lid_awake_enabled",
        ] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        let back: JsonStatus = serde_json::from_value(v).unwrap();
        assert_eq!(back.stop_reason_code.as_deref(), Some("user"));
    }

    #[test]
    fn idle_view_mentions_display_policy() {
        let cfg = AppConfig::default();
        let view = build_view_model(&cfg, false, None, None, &host(), None, false);
        assert!(!view.active);
        assert!(view.detail_line.contains("display") || view.detail_line.contains("屏幕"));
        assert_eq!(view.primary_action, cfg.tr().start_standby());
        assert!(!view.display_asleep);
        assert!(view.user_present);
        assert_eq!(view.elapsed_secs, None);
        assert_eq!(view.remaining_secs, None);
    }

    #[test]
    fn active_view_exposes_elapsed_secs() {
        let cfg = AppConfig::default();
        let mut h = host();
        h.monotonic_ms = 66_000;
        let view = build_view_model(&cfg, true, Some(65), None, &h, None, false);
        assert_eq!(view.elapsed_secs, Some(65));
        assert_eq!(view.remaining_secs, None);
        assert_eq!(view.primary_action, cfg.tr().end_standby());
    }

    #[test]
    fn remaining_secs_surface_on_the_view() {
        let cfg = AppConfig::default();
        let view = build_view_model(&cfg, true, Some(0), Some(3_598), &host(), None, false);
        assert_eq!(view.elapsed_secs, Some(0));
        assert_eq!(view.remaining_secs, Some(3_598));
        assert!(view.remaining_label.is_some());
    }

    #[test]
    fn view_model_carries_display_asleep_and_user_present() {
        let cfg = AppConfig::default();
        let mut away = host();
        away.hid_idle_ms = 80_000;
        let view = build_view_model(&cfg, true, Some(0), None, &away, None, true);
        assert!(view.display_asleep);
        assert!(!view.user_present);
        let present = build_view_model(&cfg, true, Some(0), None, &host(), None, false);
        assert!(!present.display_asleep);
        assert!(present.user_present);
    }

    #[test]
    fn last_stop_reason_surfaces_when_idle() {
        let cfg = AppConfig::default();
        let view = build_view_model(
            &cfg,
            false,
            None,
            None,
            &host(),
            Some("Battery too low"),
            false,
        );
        assert_eq!(view.warnings, vec!["Battery too low".to_string()]);
    }

    #[test]
    fn chinese_idle_status_is_localized() {
        let cfg = AppConfig {
            language: Some(Lang::Zh),
            ..AppConfig::default()
        };
        let view = build_view_model(&cfg, false, None, None, &host(), None, false);
        assert_eq!(view.primary_action, "开始关屏待命");
        assert!(view.status_line.contains("未待命"));
    }
}
