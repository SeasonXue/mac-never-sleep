use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use never_sleep_core::{
    parse_duration_pref, DurationPref, Engine, Input, StopReason, HEARTBEAT_MS,
};

use crate::apply::{dispatch, stop_for_quit};
use crate::persist::load_config;
use crate::platform::Platform;

pub fn run_foreground(
    platform: &mut dyn Platform,
    duration: Option<DurationPref>,
) -> Result<(), String> {
    platform.cleanup_orphans();
    let mut engine = Engine::new(load_config());
    let input = match duration {
        Some(d) => Input::StartWith(d),
        None => Input::Start,
    };
    dispatch(&mut engine, platform, input);
    if !engine.is_active() {
        return Err(engine.config.tr().foreground_failed().into());
    }

    let running = std::sync::Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let _ = ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    });

    let t = engine.config.tr();
    println!("{}", t.foreground_started());
    println!("{}", t.foreground_status_hint());

    while running.load(Ordering::SeqCst) && engine.is_active() {
        dispatch(&mut engine, platform, Input::Tick);
        thread::sleep(Duration::from_millis(HEARTBEAT_MS));
    }

    if engine.is_active() {
        dispatch(
            &mut engine,
            platform,
            Input::Stop {
                reason: StopReason::User,
            },
        );
    }
    stop_for_quit(&mut engine, platform);
    println!("{}", engine.config.tr().foreground_ended());
    Ok(())
}

pub fn parse_optional_duration(raw: Option<&str>) -> Result<Option<DurationPref>, String> {
    match raw {
        None => Ok(None),
        Some(s) => parse_duration_pref(s).map(Some),
    }
}
