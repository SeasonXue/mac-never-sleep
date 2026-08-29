use never_sleep_core::{Effect, Engine, HostSnapshot, Input, StopReason};

use crate::persist::save_config;
use crate::platform::Platform;

pub fn apply_effects(engine: &Engine, platform: &mut dyn Platform, effects: &[Effect]) -> bool {
    let mut power_ok = true;
    for effect in effects {
        match effect {
            Effect::ApplyPower(plan) => {
                if let Err(e) = platform.apply_power(*plan) {
                    platform.notify("熄屏待命", &format!("电源断言失败：{e}"));
                    power_ok = false;
                    break;
                }
            }
            Effect::ReleasePower => {
                let _ = platform.release_power();
            }
            Effect::SleepDisplay => {
                if let Err(e) = platform.sleep_display() {
                    eprintln!("关屏失败：{e}");
                }
            }
            Effect::LockSession => platform.lock_session(),
            Effect::Notify { title, body } => platform.notify(title, body),
        }
    }
    save_config(&engine.config);
    power_ok
}

pub fn dispatch(engine: &mut Engine, platform: &mut dyn Platform, input: Input) -> HostSnapshot {
    let host = platform.snapshot();
    let effects = engine.handle(input, &host);
    if !apply_effects(engine, platform, &effects) && engine.is_active() {
        let host = platform.snapshot();
        let stop = engine.handle(
            Input::Stop {
                reason: StopReason::AssertionFailed,
            },
            &host,
        );
        apply_effects(engine, platform, &stop);
    }
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
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = FailPower;
        dispatch(&mut engine, &mut platform, Input::Start);
        assert!(
            !engine.is_active(),
            "idle assertion failure must not leave standby on"
        );
    }
}
