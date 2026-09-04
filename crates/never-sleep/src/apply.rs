use never_sleep_core::{Effect, Engine, HostSnapshot, Input, StopReason};

use crate::persist::save_config;
use crate::platform::Platform;

pub fn apply_effects(engine: &Engine, platform: &mut dyn Platform, effects: &[Effect]) -> bool {
    let t = engine.config.tr();
    let mut power_ok = true;
    for effect in effects {
        match effect {
            Effect::ApplyPower(plan) => {
                if let Err(e) = platform.apply_power(*plan) {
                    platform.notify(t.app_display_name(), &t.power_assertion_failed(&e));
                    power_ok = false;
                    break;
                }
            }
            Effect::ReleasePower => {
                let _ = platform.release_power();
            }
            Effect::SleepDisplay => {
                if let Err(e) = platform.sleep_display() {
                    eprintln!("{}", t.sleep_display_failed(&e));
                }
            }
            Effect::LockSession => platform.lock_session(),
            Effect::Notify { title, body } => platform.notify(title, body),
        }
    }
    if crate::persist::should_persist_config(
        crate::ipc::this_process_owns_ipc(),
        crate::ipc::menu_socket_absent(),
    ) {
        save_config(&engine.config);
    }
    power_ok
}

pub fn apply_effects_or_abort(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    effects: &[Effect],
) {
    if !apply_effects(engine, platform, effects) && engine.is_active() {
        let host = platform.snapshot();
        let stop = engine.handle(
            Input::Stop {
                reason: StopReason::AssertionFailed,
            },
            &host,
        );
        apply_effects(engine, platform, &stop);
    }
}

pub fn dispatch(engine: &mut Engine, platform: &mut dyn Platform, input: Input) -> HostSnapshot {
    let host = platform.snapshot();
    let effects = engine.handle(input, &host);
    apply_effects_or_abort(engine, platform, &effects);
    platform.snapshot()
}

pub fn stop_for_quit(engine: &mut Engine, platform: &mut dyn Platform) {
    if engine.is_active() {
        dispatch(
            engine,
            platform,
            Input::Stop {
                reason: StopReason::AppQuit,
            },
        );
    } else {
        let _ = platform.release_power();
        platform.cleanup_orphans();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use never_sleep_core::{AppConfig, HostSnapshot, PowerPlan, Thermal};

    struct FailPower;

    impl Platform for FailPower {
        fn snapshot(&self) -> HostSnapshot {
            HostSnapshot {
                monotonic_ms: 0,
                continuous_ms: 0,
                unix_secs: 1_700_000_000,
                utc_offset_secs: 0,
                on_ac: true,
                battery_percent: Some(80),
                lid_closed: false,
                display_asleep: Some(false),
                hid_idle_ms: 0,
                thermal: Thermal::Nominal,
            }
        }
        fn apply_power(&mut self, _plan: PowerPlan) -> Result<(), String> {
            Err("denied".into())
        }
        fn release_power(&mut self) -> Result<(), String> {
            Ok(())
        }
        fn sleep_display(&self) -> Result<(), String> {
            Ok(())
        }
        fn lock_session(&self) {}
        fn notify(&self, _title: &str, _body: &str) {}
        fn set_launch_at_login(&self, _enabled: bool) -> Result<(), String> {
            Ok(())
        }
        fn cleanup_orphans(&self) {}
        fn doctor(&self) -> String {
            String::new()
        }
    }

    #[test]
    fn assertion_failure_aborts_session() {
        let _isolated = crate::paths::TestDataDir::install();
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = FailPower;
        dispatch(&mut engine, &mut platform, Input::Start);
        assert!(
            !engine.is_active(),
            "idle assertion failure must not leave standby on"
        );
    }

    struct Rec {
        host: HostSnapshot,
        events: std::cell::RefCell<Vec<String>>,
        fail_sleep: bool,
    }

    impl Rec {
        fn new() -> Self {
            Self {
                host: FailPower.snapshot(),
                events: std::cell::RefCell::new(Vec::new()),
                fail_sleep: false,
            }
        }
        fn push(&self, s: impl Into<String>) {
            self.events.borrow_mut().push(s.into());
        }
    }

    impl Platform for Rec {
        fn snapshot(&self) -> HostSnapshot {
            self.host.clone()
        }
        fn apply_power(&mut self, plan: PowerPlan) -> Result<(), String> {
            self.push(format!("apply:{}", plan.prevent_idle_sleep));
            Ok(())
        }
        fn release_power(&mut self) -> Result<(), String> {
            self.push("release");
            Ok(())
        }
        fn sleep_display(&self) -> Result<(), String> {
            self.push("sleep");
            if self.fail_sleep {
                Err("no display".into())
            } else {
                Ok(())
            }
        }
        fn lock_session(&self) {
            self.push("lock");
        }
        fn notify(&self, title: &str, _body: &str) {
            self.push(format!("notify:{title}"));
        }
        fn set_launch_at_login(&self, _enabled: bool) -> Result<(), String> {
            Ok(())
        }
        fn cleanup_orphans(&self) {
            self.push("cleanup");
        }
        fn doctor(&self) -> String {
            String::new()
        }
    }

    #[test]
    fn start_applies_power_and_notifies() {
        let _isolated = crate::paths::TestDataDir::install();
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = Rec::new();
        dispatch(&mut engine, &mut platform, Input::Start);
        assert!(engine.is_active());
        let events = platform.events.borrow().clone();
        assert!(events.iter().any(|e| e.starts_with("apply:")));
        assert!(events.iter().any(|e| e.starts_with("notify:")));
    }

    #[test]
    fn sleep_display_error_does_not_abort_session() {
        let _isolated = crate::paths::TestDataDir::install();
        let cfg = AppConfig {
            display_off_delay_ms: 0,
            ..AppConfig::default()
        };
        let mut engine = Engine::new(cfg);
        let mut platform = Rec::new();
        platform.fail_sleep = true;
        dispatch(&mut engine, &mut platform, Input::Start);
        assert!(engine.is_active());
        assert!(platform.events.borrow().iter().any(|e| e == "sleep"));
    }

    #[test]
    fn stop_for_quit_when_idle_releases_and_cleans() {
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = Rec::new();
        stop_for_quit(&mut engine, &mut platform);
        let events = platform.events.borrow().clone();
        assert_eq!(events, vec!["release".to_string(), "cleanup".to_string()]);
    }

    #[test]
    fn first_sleep_locks_when_configured() {
        let _isolated = crate::paths::TestDataDir::install();
        let cfg = AppConfig {
            lock_screen: true,
            display_off_delay_ms: 0,
            ..AppConfig::default()
        };
        let mut engine = Engine::new(cfg);
        let mut platform = Rec::new();
        dispatch(&mut engine, &mut platform, Input::Start);
        assert!(platform.events.borrow().iter().any(|e| e == "lock"));
    }
}
