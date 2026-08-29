use never_sleep_core::{Effect, Engine, HostSnapshot, Input, StopReason};

use crate::persist::save_config;
use crate::platform::Platform;

pub fn apply_effects(engine: &Engine, platform: &mut dyn Platform, effects: &[Effect]) {
    for effect in effects {
        match effect {
            Effect::ApplyPower(plan) => {
                if let Err(e) = platform.apply_power(*plan) {
                    platform.notify("熄屏待命", &format!("电源断言失败：{e}"));
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
}

pub fn dispatch(engine: &mut Engine, platform: &mut dyn Platform, input: Input) -> HostSnapshot {
    let host = platform.snapshot();
    let effects = engine.handle(input, &host);
    apply_effects(engine, platform, &effects);
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
