use crate::config::AppConfig;
use crate::duration::{deadline_unix_secs, format_duration_zh};
use crate::status::{build_view_model, HostSnapshot, JsonStatus, Thermal, ViewModel};
use crate::DurationPref;
use crate::SLEEP_DISPLAY_DEBOUNCE_MS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerPlan {
    pub prevent_idle_sleep: bool,
    pub prevent_system_sleep: bool,
    pub prevent_disk_idle: bool,
    pub network_client: bool,
    pub disable_clamshell_sleep: bool,
}

impl PowerPlan {
    pub fn off() -> Self {
        Self {
            prevent_idle_sleep: false,
            prevent_system_sleep: false,
            prevent_disk_idle: false,
            network_client: false,
            disable_clamshell_sleep: false,
        }
    }

    pub fn for_session(cfg: &AppConfig, on_ac: bool) -> Self {
        Self {
            prevent_idle_sleep: true,
            // Apple 文档：PreventSystemSleep 主要在 AC 下有效
            prevent_system_sleep: cfg.keep_awake_on_lid_close && on_ac,
            prevent_disk_idle: true,
            network_client: true,
            disable_clamshell_sleep: cfg.keep_awake_on_lid_close,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    ApplyPower(PowerPlan),
    ReleasePower,
    SleepDisplay,
    LockSession,
    Notify { title: String, body: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Start,
    StartWith(DurationPref),
    Stop { reason: StopReason },
    Toggle,
    Tick,
    DisplayWoke,
    DisplaySlept,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    User,
    BatteryFloor,
    ThermalEmergency,
    DurationElapsed,
    AppQuit,
    AssertionFailed,
}

impl StopReason {
    pub fn label_zh(self) -> &'static str {
        match self {
            Self::User => "已由你结束",
            Self::BatteryFloor => "电量过低，已结束待命以免耗干电池",
            Self::ThermalEmergency => "系统过热，已结束待命",
            Self::DurationElapsed => "到达设定时长，已结束待命",
            Self::AppQuit => "应用退出，已恢复正常睡眠",
            Self::AssertionFailed => "无法阻止系统睡眠，已取消待命",
        }
    }
}

#[derive(Debug, Clone)]
struct Session {
    started_ms: u64,
    _started_unix: i64,
    _duration: DurationPref,
    deadline_unix: Option<i64>,
    initial_display_off_sent: bool,
    last_sleep_display_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Engine {
    pub config: AppConfig,
    session: Option<Session>,
    optimistic_display_asleep: bool,
    last_stop_reason: Option<String>,
    last_plan: PowerPlan,
}

impl Engine {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            session: None,
            optimistic_display_asleep: false,
            last_stop_reason: None,
            last_plan: PowerPlan::off(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.session.is_some()
    }

    pub fn handle(&mut self, input: Input, host: &HostSnapshot) -> Vec<Effect> {
        let mut effects = Vec::new();
        match input {
            Input::Start => self.start(self.config.duration, host, &mut effects),
            Input::StartWith(pref) => self.start(pref, host, &mut effects),
            Input::Toggle => {
                if self.session.is_some() {
                    self.stop(StopReason::User, host, &mut effects);
                } else {
                    self.start(self.config.duration, host, &mut effects);
                }
            }
            Input::Stop { reason } => self.stop(reason, host, &mut effects),
            Input::DisplayWoke => {
                self.optimistic_display_asleep = false;
                self.tick(host, &mut effects);
            }
            Input::DisplaySlept => {
                self.optimistic_display_asleep = true;
            }
            Input::Tick => self.tick(host, &mut effects),
        }
        effects
    }

    fn start(&mut self, pref: DurationPref, host: &HostSnapshot, effects: &mut Vec<Effect>) {
        if self.session.is_some() {
            return;
        }
        self.config.duration = pref;
        let deadline =
            deadline_unix_secs(host.unix_secs, host.utc_offset_secs, host.unix_secs, pref);
        self.session = Some(Session {
            started_ms: host.monotonic_ms,
            _started_unix: host.unix_secs,
            _duration: pref,
            deadline_unix: deadline,
            initial_display_off_sent: false,
            last_sleep_display_ms: None,
        });
        self.optimistic_display_asleep = host.display_asleep.unwrap_or(false);
        self.last_stop_reason = None;
        let plan = PowerPlan::for_session(&self.config, host.on_ac);
        self.last_plan = plan;
        effects.push(Effect::ApplyPower(plan));

        let mut body = if self.config.screen_off {
            format!(
                "约 {} 秒后关闭屏幕，电脑保持运行。按 {} 结束。",
                (self.config.display_off_delay_ms.max(1) + 999) / 1000,
                crate::DEFAULT_HOTKEY_LABEL
            )
        } else {
            format!(
                "电脑将保持运行（不强制关屏）。按 {} 结束。",
                crate::DEFAULT_HOTKEY_LABEL
            )
        };
        if let Some(d) = deadline {
            let rem = d.saturating_sub(host.unix_secs).max(0) as u64;
            body.push_str(&format!(" 剩余 {}。", format_duration_zh(rem)));
        }
        effects.push(Effect::Notify {
            title: "已进入熄屏待命".into(),
            body,
        });
        self.tick(host, effects);
    }

    fn stop(&mut self, reason: StopReason, host: &HostSnapshot, effects: &mut Vec<Effect>) {
        if self.session.take().is_none() {
            return;
        }
        self.last_plan = PowerPlan::off();
        self.optimistic_display_asleep = host.display_asleep.unwrap_or(false);
        self.last_stop_reason = Some(reason.label_zh().into());
        effects.push(Effect::ReleasePower);
        if !matches!(reason, StopReason::AppQuit | StopReason::User) {
            effects.push(Effect::Notify {
                title: "熄屏待命已结束".into(),
                body: reason.label_zh().into(),
            });
        } else if matches!(reason, StopReason::User) {
            effects.push(Effect::Notify {
                title: "熄屏待命已结束".into(),
                body: "系统恢复正常睡眠策略。".into(),
            });
        }
    }

    fn tick(&mut self, host: &HostSnapshot, effects: &mut Vec<Effect>) {
        if self.session.is_none() {
            return;
        }
        if let Some(reason) = self.should_auto_stop(host) {
            self.stop(reason, host, effects);
            return;
        }
        let plan = PowerPlan::for_session(&self.config, host.on_ac);
        if plan != self.last_plan {
            self.last_plan = plan;
            effects.push(Effect::ApplyPower(plan));
        }
        if self.should_sleep_display(host) {
            let first = self
                .session
                .as_ref()
                .is_some_and(|s| !s.initial_display_off_sent);
            if let Some(session) = self.session.as_mut() {
                session.initial_display_off_sent = true;
                session.last_sleep_display_ms = Some(host.monotonic_ms);
            }
            self.optimistic_display_asleep = true;
            effects.push(Effect::SleepDisplay);
            if first && self.config.lock_screen {
                effects.push(Effect::LockSession);
            }
        }
    }

    fn should_auto_stop(&self, host: &HostSnapshot) -> Option<StopReason> {
        let session = self.session.as_ref()?;
        if host.thermal == Thermal::Critical {
            return Some(StopReason::ThermalEmergency);
        }
        if let Some(floor) = self.config.battery_floor_percent {
            if !host.on_ac {
                if let Some(pct) = host.battery_percent {
                    if pct < floor {
                        return Some(StopReason::BatteryFloor);
                    }
                }
            }
        }
        if let Some(deadline) = session.deadline_unix {
            if host.unix_secs >= deadline {
                return Some(StopReason::DurationElapsed);
            }
        }
        None
    }

    fn display_asleep(&self, host: &HostSnapshot) -> bool {
        host.display_asleep
            .unwrap_or(self.optimistic_display_asleep)
    }

    fn should_sleep_display(&self, host: &HostSnapshot) -> bool {
        let Some(session) = self.session.as_ref() else {
            return false;
        };
        if !self.config.screen_off {
            return false;
        }
        let elapsed = host.monotonic_ms.saturating_sub(session.started_ms);
        if elapsed < self.config.display_off_delay_ms {
            return false;
        }
        if let Some(last) = session.last_sleep_display_ms {
            if host.monotonic_ms.saturating_sub(last) < SLEEP_DISPLAY_DEBOUNCE_MS {
                return false;
            }
        }
        if !session.initial_display_off_sent {
            // 第一次关屏：无论当前亮不亮都请求一次
            return true;
        }
        if !self.config.resleep_display {
            return false;
        }
        // 人在电脑前：把屏幕控制权交还，绝不抢
        if host.user_present(self.config.user_idle_resleep_ms) {
            return false;
        }
        // 人不在就周期性重申关屏。远程代理若把屏幕点亮，几秒内会被盖回去。
        // 屏幕本来就灭时，pmset displaysleepnow 基本是空操作。
        true
    }

    pub fn view(&self, host: &HostSnapshot) -> ViewModel {
        let (started, deadline) = match &self.session {
            Some(s) => (Some(s.started_ms), s.deadline_unix),
            None => (None, None),
        };
        build_view_model(
            &self.config,
            self.is_active(),
            started,
            deadline,
            host,
            self.last_stop_reason.as_deref(),
            self.display_asleep(host),
        )
    }

    pub fn json_status(&self, host: &HostSnapshot) -> JsonStatus {
        let (elapsed, remaining) = match &self.session {
            Some(s) => (
                Some(host.monotonic_ms.saturating_sub(s.started_ms) / 1000),
                s.deadline_unix
                    .map(|d| d.saturating_sub(host.unix_secs) as u64),
            ),
            None => (None, None),
        };
        JsonStatus {
            active: self.is_active(),
            display: if self.display_asleep(host) {
                "asleep".into()
            } else {
                "awake".into()
            },
            lid: if host.lid_closed {
                "closed".into()
            } else {
                "open".into()
            },
            on_ac: host.on_ac,
            battery: host.battery_percent,
            remaining_secs: remaining,
            user_present: host.user_present(self.config.user_idle_resleep_ms),
            elapsed_secs: elapsed,
            stop_reason: self.last_stop_reason.clone(),
            screen_off_enabled: self.config.screen_off,
            lid_awake_enabled: self.config.keep_awake_on_lid_close,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Thermal;

    fn host(ms: u64) -> HostSnapshot {
        HostSnapshot {
            monotonic_ms: ms,
            unix_secs: 1_700_000_000 + (ms as i64 / 1000),
            utc_offset_secs: 8 * 3600,
            on_ac: true,
            battery_percent: Some(80),
            lid_closed: false,
            display_asleep: Some(false),
            hid_idle_ms: 60_000,
            thermal: Thermal::Nominal,
        }
    }

    fn cfg() -> AppConfig {
        AppConfig {
            display_off_delay_ms: 1_500,
            user_idle_resleep_ms: 45_000,
            ..AppConfig::default()
        }
    }

    fn has_sleep(effects: &[Effect]) -> bool {
        effects.iter().any(|e| matches!(e, Effect::SleepDisplay))
    }

    fn has_release(effects: &[Effect]) -> bool {
        effects.iter().any(|e| matches!(e, Effect::ReleasePower))
    }

    fn apply_plan(effects: &[Effect]) -> Option<PowerPlan> {
        effects.iter().rev().find_map(|e| match e {
            Effect::ApplyPower(p) => Some(*p),
            _ => None,
        })
    }

    #[test]
    fn delay_then_sleep_display() {
        let mut eng = Engine::new(cfg());
        let e = eng.handle(Input::Start, &host(0));
        assert!(apply_plan(&e).unwrap().prevent_idle_sleep);
        assert!(!has_sleep(&e));
        let e = eng.handle(Input::Tick, &host(1_499));
        assert!(!has_sleep(&e));
        let e = eng.handle(Input::Tick, &host(1_500));
        assert!(has_sleep(&e));
        // debounce
        let mut h = host(2_000);
        h.display_asleep = Some(false);
        // optimistic asleep after previous sleep, host still false? engine set optimistic true
        // host Some(false) wins — but debounce 3s
        let e = eng.handle(Input::Tick, &h);
        assert!(!has_sleep(&e));
    }

    #[test]
    fn user_present_does_not_resleep() {
        let mut eng = Engine::new(cfg());
        eng.handle(Input::Start, &host(0));
        eng.handle(Input::Tick, &host(1_500));
        let mut h = host(10_000);
        h.display_asleep = Some(false);
        h.hid_idle_ms = 1_000; // typing
        h.lid_closed = false;
        let e = eng.handle(Input::DisplayWoke, &h);
        assert!(!has_sleep(&e), "must not fight a person sitting at the Mac");
    }

    #[test]
    fn agent_wake_resleeps_when_user_away() {
        let mut eng = Engine::new(cfg());
        eng.handle(Input::Start, &host(0));
        eng.handle(Input::Tick, &host(1_500));
        let mut h = host(10_000);
        h.display_asleep = Some(false);
        h.hid_idle_ms = 120_000;
        let e = eng.handle(Input::DisplayWoke, &h);
        assert!(has_sleep(&e));
    }

    #[test]
    fn watchdog_resleeps_on_tick_without_wake_event() {
        let mut eng = Engine::new(cfg());
        eng.handle(Input::Start, &host(0));
        eng.handle(Input::Tick, &host(1_500));
        let mut h = host(1_500 + SLEEP_DISPLAY_DEBOUNCE_MS);
        h.display_asleep = Some(true);
        h.hid_idle_ms = 80_000;
        let e = eng.handle(Input::Tick, &h);
        assert!(
            has_sleep(&e),
            "away + debounce elapsed => reassert display sleep"
        );
    }

    #[test]
    fn lid_closed_always_resleeps() {
        let mut eng = Engine::new(cfg());
        eng.handle(Input::Start, &host(0));
        eng.handle(Input::Tick, &host(1_500));
        let mut h = host(10_000);
        h.display_asleep = Some(false);
        h.lid_closed = true;
        h.hid_idle_ms = 0; // even if HID looks busy
        let e = eng.handle(Input::DisplayWoke, &h);
        assert!(has_sleep(&e));
    }

    #[test]
    fn battery_floor_stops_on_battery() {
        let mut eng = Engine::new(cfg());
        eng.handle(Input::Start, &host(0));
        let mut h = host(5_000);
        h.on_ac = false;
        h.battery_percent = Some(19);
        let e = eng.handle(Input::Tick, &h);
        assert!(has_release(&e));
        assert!(!eng.is_active());
    }

    #[test]
    fn battery_floor_ignored_on_ac() {
        let mut eng = Engine::new(cfg());
        eng.handle(Input::Start, &host(0));
        let mut h = host(5_000);
        h.on_ac = true;
        h.battery_percent = Some(5);
        let e = eng.handle(Input::Tick, &h);
        assert!(!has_release(&e));
        assert!(eng.is_active());
    }

    #[test]
    fn first_sleep_can_lock_session() {
        let mut cfg = cfg();
        cfg.lock_screen = true;
        let mut eng = Engine::new(cfg);
        eng.handle(Input::Start, &host(0));
        let e = eng.handle(Input::Tick, &host(1_500));
        assert!(has_sleep(&e));
        assert!(e.iter().any(|x| matches!(x, Effect::LockSession)));
        let e = eng.handle(Input::Tick, &host(1_500 + SLEEP_DISPLAY_DEBOUNCE_MS));
        assert!(has_sleep(&e));
        assert!(!e.iter().any(|x| matches!(x, Effect::LockSession)));
    }

    #[test]
    fn duration_hours_elapses() {
        let mut cfg = cfg();
        cfg.duration = DurationPref::Hours { hours: 1 };
        let mut eng = Engine::new(cfg);
        let h0 = host(0);
        eng.handle(Input::Start, &h0);
        let mut h = host(3_600_000);
        h.unix_secs = h0.unix_secs + 3600;
        let e = eng.handle(Input::Tick, &h);
        assert!(has_release(&e));
    }

    #[test]
    fn no_screen_off_never_sleeps_display() {
        let mut cfg = cfg();
        cfg.screen_off = false;
        let mut eng = Engine::new(cfg);
        eng.handle(Input::Start, &host(0));
        let e = eng.handle(Input::Tick, &host(5_000));
        assert!(!has_sleep(&e));
    }

    #[test]
    fn clamshell_and_system_assertion_policy() {
        let mut eng = Engine::new(cfg());
        let e = eng.handle(Input::Start, &host(0));
        let plan = apply_plan(&e).unwrap();
        assert!(plan.disable_clamshell_sleep);
        assert!(plan.prevent_system_sleep);
        assert!(plan.network_client);

        let mut h = host(2_000);
        h.on_ac = false;
        h.display_asleep = Some(true);
        let e = eng.handle(Input::Tick, &h);
        let plan = apply_plan(&e).unwrap();
        assert!(plan.disable_clamshell_sleep);
        assert!(!plan.prevent_system_sleep);
    }

    #[test]
    fn thermal_critical_stops() {
        let mut eng = Engine::new(cfg());
        eng.handle(Input::Start, &host(0));
        let mut h = host(2_000);
        h.thermal = Thermal::Critical;
        let e = eng.handle(Input::Tick, &h);
        assert!(has_release(&e));
    }

    #[test]
    fn toggle_and_json() {
        let mut eng = Engine::new(cfg());
        let h = host(0);
        eng.handle(Input::Toggle, &h);
        assert!(eng.is_active());
        let st = eng.json_status(&h);
        assert!(st.active);
        eng.handle(Input::Toggle, &host(200));
        assert!(!eng.is_active());
    }

    #[test]
    fn view_model_primary_action() {
        let mut eng = Engine::new(cfg());
        let h = host(0);
        assert_eq!(eng.view(&h).primary_action, "开始熄屏待命");
        eng.handle(Input::Start, &h);
        assert_eq!(eng.view(&h).primary_action, "结束待命");
    }
}
