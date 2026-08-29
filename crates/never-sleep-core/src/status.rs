use serde::{Deserialize, Serialize};

use crate::{format_duration_zh, AppConfig, DurationPref};

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

/// 从系统采样的瞬时状态。GUI 与 CLI 共用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostSnapshot {
    pub monotonic_ms: u64,
    pub unix_secs: i64,
    /// 本地相对 UTC 的秒偏移
    pub utc_offset_secs: i32,
    pub on_ac: bool,
    pub battery_percent: Option<u8>,
    pub lid_closed: bool,
    /// `None` 表示探测不到，由引擎自己的乐观状态兜底
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
    let mut warnings = Vec::new();
    if active && cfg.keep_awake_on_lid_close && !host.on_ac {
        warnings.push("合盖保活在电池供电下不太可靠，建议插电".into());
    }
    if active && host.lid_closed && cfg.keep_awake_on_lid_close {
        warnings.push("合盖待命是尽力而为；最稳妥是开盖熄屏".into());
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
            Some(s) => format!("待命中 · 已 {}", format_duration_zh(s)),
            None => "待命中".into(),
        }
    } else {
        "未待命 · 点击开始".into()
    };

    let mut details: Vec<String> = Vec::new();
    if active {
        details.push(
            if display_asleep {
                "屏幕已关"
            } else if host.user_present(cfg.user_idle_resleep_ms) {
                "你正在用，屏幕由你控制"
            } else {
                "屏幕待关"
            }
            .into(),
        );
        details.push(if host.lid_closed { "合盖" } else { "开盖" }.into());
        details.push(
            if host.on_ac {
                "电源适配器"
            } else {
                "电池"
            }
            .into(),
        );
        if let Some(b) = host.battery_percent {
            details.push(format!("电量 {b}%"));
        }
        if let Some(r) = remaining {
            details.push(format!("剩余 {}", format_duration_zh(r)));
        }
    } else {
        details.push(
            if cfg.screen_off {
                "将关闭屏幕、保持系统运行"
            } else {
                "将保持系统运行（不强制关屏）"
            }
            .into(),
        );
    }

    let detail_line = details.join(" · ");

    let primary_action = if active {
        "结束待命".into()
    } else {
        "开始熄屏待命".into()
    };

    let tooltip = if active {
        format!("{} · 运行中", crate::APP_DISPLAY_NAME)
    } else {
        format!("{} · 已关闭", crate::APP_DISPLAY_NAME)
    };

    ViewModel {
        active,
        status_line,
        detail_line,
        primary_action,
        tooltip,
        remaining_label: remaining.map(format_duration_zh),
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
