//! English-first bilingual strings (en + zh-Hans).
//!
//! English is the default and the fallback. Chinese is used when the UI language
//! is `zh` (system Chinese, `--lang zh`, or `NEVER_SLEEP_LANG=zh`).

use crate::DurationPref;

pub const APP_NAME: &str = "Never Sleep";
pub const BUNDLE_ID: &str = "com.seasonxue.never-sleep";
pub const DEFAULT_HOTKEY_LABEL: &str = "⌥⌘P";
pub const LANG_ENV: &str = "NEVER_SLEEP_LANG";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lang {
    #[default]
    En,
    Zh,
}

impl Lang {
    pub fn parse_opt(raw: &str) -> Option<Self> {
        let s = raw.trim().to_ascii_lowercase().replace('_', "-");
        let primary = s.split(['-', '.', '@']).next().unwrap_or(&s);
        match primary {
            "zh" | "chi" | "chinese" | "cn" => Some(Self::Zh),
            "en" | "eng" | "english" => Some(Self::En),
            _ => None,
        }
    }

    /// `NEVER_SLEEP_LANG=en|zh` process override.
    pub fn from_override_env() -> Option<Self> {
        std::env::var(LANG_ENV)
            .ok()
            .as_deref()
            .and_then(Self::parse_opt)
    }

    /// Unix locale (`LANG` / `LC_*`). Unknown locales fall back to English.
    pub fn from_unix_locale() -> Self {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(key) {
                if let Some(lang) = Self::parse_opt(&v) {
                    return lang;
                }
                if !v.is_empty() {
                    return Self::En;
                }
            }
        }
        Self::En
    }

    /// First preferred language tag wins; non-Chinese tags resolve to English.
    pub fn from_preferred_tags<S: AsRef<str>>(tags: &[S]) -> Option<Self> {
        let first = tags.first()?.as_ref();
        Some(if Self::parse_opt(first) == Some(Self::Zh) {
            Self::Zh
        } else {
            Self::En
        })
    }

    pub fn override_or(self) -> Self {
        Self::from_override_env().unwrap_or(self)
    }

    pub fn is_chinese(self) -> bool {
        matches!(self, Self::Zh)
    }
}

/// Translator for a concrete UI language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tr {
    pub lang: Lang,
}

impl Tr {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, en: &'static str, zh: &'static str) -> &'static str {
        match self.lang {
            Lang::En => en,
            Lang::Zh => zh,
        }
    }

    pub fn app_display_name(self) -> &'static str {
        self.pick(APP_NAME, "熄屏待命")
    }

    pub fn onboarding(self) -> &'static str {
        self.pick(ONBOARDING_EN, ONBOARDING_ZH)
    }

    pub fn welcome_title(self) -> &'static str {
        self.pick("Welcome to Never Sleep", "欢迎使用熄屏待命")
    }

    pub fn help_title(self) -> &'static str {
        self.pick("How to use", "使用说明")
    }

    pub fn help_kicker(self) -> &'static str {
        self.pick("Display off · Mac online", "关屏护屏 · 电脑在线")
    }

    pub fn help_lead(self) -> &'static str {
        self.pick(
            "Remote clients such as ChatGPT and Codex can keep connecting.",
            "ChatGPT / Codex 等远程客户端仍可连上这台电脑。",
        )
    }

    pub fn help_how(self) -> &'static str {
        self.pick("Get started", "怎么用")
    }

    pub fn help_step1_title(self) -> &'static str {
        self.pick("Start standby", "开始待命")
    }

    pub fn help_step1_detail(self) -> &'static str {
        self.pick(
            "Click “Start Screen-Off Standby”. The display sleeps after about 1.5 seconds.",
            "点「开始熄屏待命」，约 1.5 秒后屏幕关闭。",
        )
    }

    pub fn help_step2_title(self) -> &'static str {
        self.pick("Stays out of your way", "不抢屏幕")
    }

    pub fn help_step2_detail(self) -> &'static str {
        self.pick(
            "It will not fight you for the screen while you type; it sleeps again after you leave.",
            "人在电脑前绝不强制关屏；走开后再自动关闭。",
        )
    }

    pub fn help_step3_title(self) -> &'static str {
        self.pick("Come back any time", "随时回来")
    }

    pub fn help_step3_before(self) -> &'static str {
        self.pick("Press", "按")
    }

    pub fn help_step3_after(self) -> &'static str {
        self.pick(
            "or choose “End Standby” in the menu.",
            "，或点菜单「结束待命」。",
        )
    }

    pub fn help_notes(self) -> &'static str {
        self.pick("Keep in mind", "请留意")
    }

    pub fn help_note_lid(self) -> &'static str {
        self.pick(
            "Closed-lid stay-awake is best-effort, and more reliable on power. Lid open + display asleep is the reliable path.",
            "合盖保活是尽力而为，插电更稳；最可靠仍是开盖熄屏。",
        )
    }

    pub fn help_note_battery(self) -> &'static str {
        self.pick(
            "Standby ends automatically on low battery so the pack is not drained.",
            "电量过低会自动结束，避免把电池耗干。",
        )
    }

    pub fn help_note_quit(self) -> &'static str {
        self.pick(
            "Quitting restores normal sleep. Energy Saver settings are never rewritten.",
            "退出后立即恢复系统睡眠，不会改写节能设置。",
        )
    }

    pub fn dialog_ok(self) -> &'static str {
        self.pick("OK", "好")
    }

    pub fn idle_status(self) -> &'static str {
        self.pick("Idle · click to start", "未待命 · 点击开始")
    }

    pub fn standby_status(self) -> &'static str {
        self.pick("Standby", "待命中")
    }

    pub fn standby_elapsed(self, duration: &str) -> String {
        match self.lang {
            Lang::En => format!("Standby · {duration} elapsed"),
            Lang::Zh => format!("待命中 · 已 {duration}"),
        }
    }

    pub fn will_sleep_display(self) -> &'static str {
        self.pick(
            "Will sleep the display and keep the Mac running",
            "将关闭屏幕、保持系统运行",
        )
    }

    pub fn will_keep_awake_only(self) -> &'static str {
        self.pick(
            "Will keep the Mac running (display stays under system control)",
            "将保持系统运行（不强制关屏）",
        )
    }

    pub fn start_standby(self) -> &'static str {
        self.pick("Start Screen-Off Standby", "开始熄屏待命")
    }

    pub fn end_standby(self) -> &'static str {
        self.pick("End Standby", "结束待命")
    }

    pub fn duration_menu(self) -> &'static str {
        self.pick("Duration", "时长")
    }

    pub fn indefinite(self) -> &'static str {
        self.pick("Indefinite", "无限期")
    }

    pub fn hours(self, hours: u32) -> String {
        match self.lang {
            Lang::En if hours == 1 => "1 hour".into(),
            Lang::En => format!("{hours} hours"),
            Lang::Zh => format!("{hours} 小时"),
        }
    }

    pub fn until_clock(self, hour: u8, minute: u8) -> String {
        match self.lang {
            Lang::En => format!("Until {hour:02}:{minute:02}"),
            Lang::Zh => format!("到 {hour:02}:{minute:02}"),
        }
    }

    pub fn duration_pref(self, pref: DurationPref) -> String {
        match pref {
            DurationPref::Indefinite => self.indefinite().into(),
            DurationPref::Hours { hours } => self.hours(hours),
            DurationPref::UntilLocal { hour, minute } => self.until_clock(hour, minute),
        }
    }

    pub fn screen_off_now(self) -> &'static str {
        self.pick("Sleep display immediately", "立即关闭屏幕")
    }

    pub fn lid_awake(self) -> &'static str {
        self.pick(
            "Keep running when the lid is closed (best effort)",
            "合盖尽量保持运行",
        )
    }

    pub fn resleep_display(self) -> &'static str {
        self.pick("Re-sleep the display after you leave", "人离开后自动再关屏")
    }

    pub fn lock_screen(self) -> &'static str {
        self.pick(
            "Lock the session when the display sleeps (breaks remote GUI)",
            "关屏时锁定登录（远程 GUI 会受影响）",
        )
    }

    pub fn battery_floor_on(self, percent: u8) -> String {
        match self.lang {
            Lang::En => format!("End when battery is below {percent}%"),
            Lang::Zh => format!("电量低于 {percent}% 时结束"),
        }
    }

    pub fn battery_floor_off(self) -> &'static str {
        self.pick(
            "Do not end automatically on low battery",
            "电量过低时不自动结束",
        )
    }

    pub fn launch_at_login(self) -> &'static str {
        self.pick("Launch at login", "登录时启动")
    }

    pub fn language_menu(self) -> &'static str {
        self.pick("Language", "语言")
    }

    pub fn language_english(self) -> &'static str {
        "English"
    }

    pub fn language_chinese(self) -> &'static str {
        "简体中文"
    }

    pub fn quit(self) -> &'static str {
        self.pick("Quit", "退出")
    }

    pub fn display_asleep(self) -> &'static str {
        self.pick("Display asleep", "屏幕已关")
    }

    pub fn user_controls_display(self) -> &'static str {
        self.pick(
            "You are using it · display is yours",
            "你正在用，屏幕由你控制",
        )
    }

    pub fn display_pending(self) -> &'static str {
        self.pick("Display will sleep", "屏幕待关")
    }

    pub fn lid_closed(self) -> &'static str {
        self.pick("Lid closed", "合盖")
    }

    pub fn lid_open(self) -> &'static str {
        self.pick("Lid open", "开盖")
    }

    pub fn power_ac(self) -> &'static str {
        self.pick("Power adapter", "电源适配器")
    }

    pub fn power_battery(self) -> &'static str {
        self.pick("Battery", "电池")
    }

    pub fn battery_percent(self, percent: u8) -> String {
        match self.lang {
            Lang::En => format!("Battery {percent}%"),
            Lang::Zh => format!("电量 {percent}%"),
        }
    }

    pub fn remaining(self, duration: &str) -> String {
        match self.lang {
            Lang::En => format!("{duration} left"),
            Lang::Zh => format!("剩余 {duration}"),
        }
    }

    pub fn tooltip_active(self) -> String {
        format!(
            "{} · {}",
            self.app_display_name(),
            self.pick("running", "运行中")
        )
    }

    pub fn tooltip_idle(self) -> String {
        format!(
            "{} · {}",
            self.app_display_name(),
            self.pick("idle", "已关闭")
        )
    }

    pub fn warn_lid_on_battery(self) -> &'static str {
        self.pick(
            "Closed-lid stay-awake is unreliable on battery; plug in if you can",
            "合盖保活在电池供电下不太可靠，建议插电",
        )
    }

    pub fn warn_lid_best_effort(self) -> &'static str {
        self.pick(
            "Closed-lid standby is best-effort; lid open + display asleep is the reliable path",
            "合盖待命是尽力而为；最稳妥是开盖熄屏",
        )
    }

    pub fn notify_started_title(self) -> &'static str {
        self.pick("Screen-off standby is on", "已进入熄屏待命")
    }

    pub fn notify_started_body_screen_off(self, seconds: u64, hotkey: &str) -> String {
        match self.lang {
            Lang::En => format!(
                "Display will sleep in about {seconds} seconds; the Mac stays awake. Press {hotkey} to end."
            ),
            Lang::Zh => format!("约 {seconds} 秒后关闭屏幕，电脑保持运行。按 {hotkey} 结束。"),
        }
    }

    pub fn notify_started_body_keep_awake(self, hotkey: &str) -> String {
        match self.lang {
            Lang::En => format!(
                "The Mac will stay awake (display is not forced off). Press {hotkey} to end."
            ),
            Lang::Zh => format!("电脑将保持运行（不强制关屏）。按 {hotkey} 结束。"),
        }
    }

    pub fn remaining_clause(self, duration: &str) -> String {
        match self.lang {
            Lang::En => format!(" {duration} left."),
            Lang::Zh => format!(" 剩余 {duration}。"),
        }
    }

    pub fn notify_ended_title(self) -> &'static str {
        self.pick("Standby ended", "熄屏待命已结束")
    }

    pub fn notify_ended_user_body(self) -> &'static str {
        self.pick("Normal sleep policy restored.", "系统恢复正常睡眠策略。")
    }

    pub fn stop_user(self) -> &'static str {
        self.pick("Ended by you", "已由你结束")
    }

    pub fn stop_battery(self) -> &'static str {
        self.pick(
            "Battery too low; standby ended to protect remaining charge",
            "电量过低，已结束待命以免耗干电池",
        )
    }

    pub fn stop_thermal(self) -> &'static str {
        self.pick("System overheating; standby ended", "系统过热，已结束待命")
    }

    pub fn stop_duration(self) -> &'static str {
        self.pick(
            "The set duration elapsed; standby ended",
            "到达设定时长，已结束待命",
        )
    }

    pub fn stop_quit(self) -> &'static str {
        self.pick(
            "App quit; normal sleep restored",
            "应用退出，已恢复正常睡眠",
        )
    }

    pub fn stop_assertion(self) -> &'static str {
        self.pick(
            "Could not prevent system sleep; standby cancelled",
            "无法阻止系统睡眠，已取消待命",
        )
    }

    pub fn already_running(self) -> &'static str {
        self.pick(
            "Never Sleep is already running in the menu bar.",
            "熄屏待命已在菜单栏运行。",
        )
    }

    pub fn ipc_not_started(self, err: &str) -> String {
        match self.lang {
            Lang::En => format!("IPC did not start: {err} (CLI will run in the foreground)"),
            Lang::Zh => format!("IPC 未启动：{err}（命令行将以前台模式工作）"),
        }
    }

    pub fn hotkey_failed(self, hotkey: &str) -> String {
        match self.lang {
            Lang::En => format!("Could not register {hotkey}; use the menu instead."),
            Lang::Zh => format!("快捷键 {hotkey} 注册失败，仍可通过菜单操作。"),
        }
    }

    pub fn login_item_title(self) -> &'static str {
        self.pick("Login item", "登录项")
    }

    pub fn menubar_macos_only(self) -> &'static str {
        self.pick(
            "The menu bar is only available on macOS.",
            "菜单栏仅支持 macOS。",
        )
    }

    pub fn cleanup_done(self) -> &'static str {
        self.pick(
            "Tried to restore clamshell sleep and clear leftover locks.",
            "已尝试还原合盖睡眠标志并清除残留锁。",
        )
    }

    pub fn failed(self) -> &'static str {
        self.pick("Failed", "失败")
    }

    pub fn not_in_standby(self) -> &'static str {
        self.pick("Not in standby.", "未待命。")
    }

    pub fn menubar_missing_foreground_json(self) -> &'static str {
        self.pick(
            "Menu bar is not running; starting in the foreground (query JSON status from another terminal).",
            "菜单栏未运行，以前台模式启动（JSON 状态请另开终端查询）。",
        )
    }

    pub fn menubar_not_running(self) -> String {
        match self.lang {
            Lang::En => format!(
                "Menu bar is not running. Open {}, or run `never-sleep on` in the foreground.",
                self.app_display_name()
            ),
            Lang::Zh => {
                "菜单栏未运行。请先打开「熄屏待命」，或使用 never-sleep on 以前台方式启动。".into()
            }
        }
    }

    pub fn cli_status_line(
        self,
        display: &str,
        lid: &str,
        power: &str,
        battery: Option<u8>,
    ) -> String {
        let batt = battery
            .map(|b| match self.lang {
                Lang::En => format!(" · battery {b}%"),
                Lang::Zh => format!(" · 电量 {b}%"),
            })
            .unwrap_or_default();
        match self.lang {
            Lang::En => format!("Standby · display {display} · {lid} · {power}{batt}"),
            Lang::Zh => format!("待命中 · 屏幕 {display} · {lid} · {power}{batt}"),
        }
    }

    pub fn foreground_failed(self) -> &'static str {
        self.pick("Could not enter standby", "未能进入待命")
    }

    pub fn foreground_started(self) -> &'static str {
        self.pick(
            "Standby is on. The display will sleep; the Mac stays awake. Press Ctrl-C to end.",
            "熄屏待命已开启。屏幕将关闭，电脑保持运行。按 Ctrl-C 结束。",
        )
    }

    pub fn foreground_status_hint(self) -> &'static str {
        self.pick(
            "Query status with `never-sleep status --json` if the menu bar is running.",
            "状态可用 `never-sleep status --json` 查询（若菜单栏正在运行）。",
        )
    }

    pub fn foreground_ended(self) -> &'static str {
        self.pick("Standby ended.", "已结束待命。")
    }

    pub fn power_assertion_failed(self, err: &str) -> String {
        match self.lang {
            Lang::En => format!("Power assertion failed: {err}"),
            Lang::Zh => format!("电源断言失败：{err}"),
        }
    }

    pub fn sleep_display_failed(self, err: &str) -> String {
        match self.lang {
            Lang::En => format!("Could not sleep the display: {err}"),
            Lang::Zh => format!("关屏失败：{err}"),
        }
    }

    pub fn assertion_reason(self) -> &'static str {
        self.pick(
            "Never Sleep: keep the Mac awake for remote clients",
            "熄屏待命：保持系统运行供远程客户端连接",
        )
    }

    pub fn idle_assertion_failed(self) -> &'static str {
        self.pick(
            "Could not create PreventUserIdleSystemSleep assertion",
            "无法创建 PreventUserIdleSystemSleep 断言",
        )
    }

    pub fn displaysleep_and_wrangler_failed(self) -> &'static str {
        self.pick(
            "pmset displaysleepnow failed, and IODisplayWrangler was not found",
            "pmset displaysleepnow 失败，且找不到 IODisplayWrangler",
        )
    }

    pub fn displaysleep_both_failed(self, ret: i32) -> String {
        match self.lang {
            Lang::En => format!(
                "Could not sleep the display: pmset and IORequestIdle both failed (IOReturn {ret})"
            ),
            Lang::Zh => format!("关屏失败：pmset 与 IORequestIdle 均未成功 (IOReturn {ret})"),
        }
    }

    pub fn launchctl_load_failed(self) -> &'static str {
        self.pick("launchctl load failed", "launchctl load 失败")
    }

    pub fn doctor_title(self) -> &'static str {
        self.pick("Never Sleep diagnostics", "熄屏待命诊断")
    }

    pub fn doctor_snapshot(
        self,
        on_ac: bool,
        battery: Option<u8>,
        lid_closed: bool,
        display_asleep: Option<bool>,
        hid_idle_ms: u64,
        thermal: &str,
    ) -> String {
        let power = if on_ac { "AC" } else { self.power_battery() };
        match self.lang {
            Lang::En => format!(
                "Power: {power}\nBattery: {battery:?}\nLid closed: {lid_closed}\nDisplay asleep: {display_asleep:?}\nHID idle: {hid_idle_ms} ms\nThermal: {thermal}\n"
            ),
            Lang::Zh => format!(
                "电源: {power}\n电量: {battery:?}\n合盖: {lid_closed}\n屏幕休眠: {display_asleep:?}\nHID空闲: {hid_idle_ms} ms\n过热: {thermal}\n"
            ),
        }
    }

    pub fn stub_not_macos(self) -> &'static str {
        self.pick(
            "This platform is not macOS. Run `never-sleep doctor` on a Mac.",
            "当前平台不是 macOS。请在 Mac 上运行 `never-sleep doctor`。",
        )
    }

    pub fn ipc_timeout(self) -> &'static str {
        self.pick("Timed out", "超时")
    }

    pub fn parse_duration_error(self, raw: &str) -> String {
        match self.lang {
            Lang::En => format!("Could not parse duration: {raw}"),
            Lang::Zh => format!("无法解析时长: {raw}"),
        }
    }

    pub fn parse_duration_min_hour(self) -> &'static str {
        self.pick(
            "Duration must be at least 1 hour, or use indefinite",
            "时长至少 1 小时，或使用 indefinite",
        )
    }

    pub fn parse_time_format(self, raw: &str) -> String {
        match self.lang {
            Lang::En => format!("Time must be HH:MM, got {raw}"),
            Lang::Zh => format!("时间格式应为 HH:MM，收到 {raw}"),
        }
    }

    pub fn parse_invalid_hour(self, raw: &str) -> String {
        match self.lang {
            Lang::En => format!("Invalid hour: {raw}"),
            Lang::Zh => format!("无效小时: {raw}"),
        }
    }

    pub fn parse_invalid_minute(self, raw: &str) -> String {
        match self.lang {
            Lang::En => format!("Invalid minute: {raw}"),
            Lang::Zh => format!("无效分钟: {raw}"),
        }
    }

    pub fn parse_invalid_time(self, raw: &str) -> String {
        match self.lang {
            Lang::En => format!("Invalid time: {raw}"),
            Lang::Zh => format!("无效时间: {raw}"),
        }
    }

    pub fn cli_about(self) -> &'static str {
        self.pick(
            "Never Sleep: turn the Mac display off and keep the machine awake for ChatGPT / Codex remote sessions",
            "熄屏待命：关掉 Mac 屏幕、不让电脑睡眠，方便 ChatGPT / Codex 远程连接",
        )
    }
}

pub fn app_display_name(lang: Lang) -> &'static str {
    Tr::new(lang).app_display_name()
}

pub fn onboarding(lang: Lang) -> &'static str {
    Tr::new(lang).onboarding()
}

const ONBOARDING_EN: &str = "\
Never Sleep turns the display off while the Mac stays awake, so ChatGPT / Codex can keep connecting.

Get started
1. Start standby — click “Start Screen-Off Standby”. The display sleeps after about 1.5 seconds.
2. Stays out of your way — it will not fight you for the screen while you type; it sleeps again after you leave.
3. Come back — press ⌥⌘P, or choose “End Standby” in the menu.

Keep in mind
• Closed-lid stay-awake is best-effort (more reliable on power). Lid open + display asleep is the reliable path.
• Standby ends automatically on low battery so the pack is not drained.
• Quitting restores normal sleep. Energy Saver settings are never rewritten.\
";

const ONBOARDING_ZH: &str = "\
熄屏待命会关掉屏幕，同时不让 Mac 进入睡眠。ChatGPT / Codex 等远程客户端仍可连上这台电脑。\n\n\
怎么用\n\
1. 开始待命 — 点「开始熄屏待命」，约 1.5 秒后屏幕关闭。\n\
2. 不抢屏幕 — 人在电脑前绝不强制关屏；走开后再自动关闭。\n\
3. 随时回来 — 按 ⌥⌘P，或点菜单「结束待命」。\n\n\
请留意\n\
• 合盖保活是尽力而为，插电更稳；最可靠仍是开盖熄屏。\n\
• 电量过低会自动结束，避免把电池耗干。\n\
• 退出后立即恢复系统睡眠，不会改写节能设置。\
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_language_tags() {
        assert_eq!(Lang::parse_opt("en"), Some(Lang::En));
        assert_eq!(Lang::parse_opt("en_US.UTF-8"), Some(Lang::En));
        assert_eq!(Lang::parse_opt("zh-Hans-CN"), Some(Lang::Zh));
        assert_eq!(Lang::parse_opt("zh_CN"), Some(Lang::Zh));
        assert_eq!(Lang::parse_opt("fr_FR"), None);
        assert_eq!(Lang::parse_opt("C"), None);
    }

    #[test]
    fn preferred_tags_use_first() {
        assert_eq!(
            Lang::from_preferred_tags(&["zh-Hans-CN", "en-US"]),
            Some(Lang::Zh)
        );
        assert_eq!(
            Lang::from_preferred_tags(&["en-US", "zh-Hans"]),
            Some(Lang::En)
        );
        assert_eq!(Lang::from_preferred_tags(&["fr-FR"]), Some(Lang::En));
    }

    #[test]
    fn english_is_default() {
        assert_eq!(Lang::default(), Lang::En);
        assert_eq!(
            Tr::new(Lang::En).start_standby(),
            "Start Screen-Off Standby"
        );
        assert_eq!(Tr::new(Lang::Zh).start_standby(), "开始熄屏待命");
        assert_eq!(Tr::new(Lang::En).app_display_name(), "Never Sleep");
        assert_eq!(Tr::new(Lang::Zh).app_display_name(), "熄屏待命");
    }

    #[test]
    fn help_copy_is_sectioned() {
        let en = Tr::new(Lang::En);
        let zh = Tr::new(Lang::Zh);
        assert_eq!(en.help_how(), "Get started");
        assert_eq!(zh.help_how(), "怎么用");
        assert!(en.onboarding().contains("Get started"));
        assert!(en.onboarding().contains("Keep in mind"));
        assert!(en.onboarding().contains(DEFAULT_HOTKEY_LABEL));
        assert!(zh.onboarding().contains("怎么用"));
        assert!(zh.onboarding().contains("请留意"));
        assert!(zh.onboarding().contains(DEFAULT_HOTKEY_LABEL));
    }
}
