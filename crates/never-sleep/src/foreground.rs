use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use never_sleep_core::{DurationPref, Engine, Input, Lang, StopReason, HEARTBEAT_MS};

use crate::apply::{dispatch, stop_for_quit};
use crate::persist::load_config;
use crate::platform::Platform;
use crate::protocol;

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

pub fn parse_optional_duration(
    raw: Option<&str>,
    lang: Lang,
) -> Result<Option<DurationPref>, String> {
    protocol::parse_on_duration_in(raw, lang)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_optional_duration_passthrough() {
        assert_eq!(parse_optional_duration(None, Lang::En).unwrap(), None);
        assert_eq!(
            parse_optional_duration(Some("1h"), Lang::En).unwrap(),
            Some(DurationPref::Hours { hours: 1 })
        );
        assert!(parse_optional_duration(Some("0h"), Lang::En).is_err());
    }
}
