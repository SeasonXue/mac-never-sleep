use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use never_sleep_core::{DurationPref, Engine, Input, Lang, StopReason, HEARTBEAT_MS};

use crate::apply::{dispatch, stop_for_quit};
use crate::ipc::try_send;
use crate::persist::load_config;
use crate::platform::Platform;
use crate::protocol::{self, IpcRequest};

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

    let cloud_ok = crate::cloud::cloud_enabled();
    let mut cloud = if cloud_ok && try_send(&IpcRequest::Ping).is_none() {
        match crate::cloud::load_or_create_identity() {
            Ok(identity) => Some(crate::cloud::spawn_reporter(
                identity,
                crate::cloud::default_display_name(),
                engine.config.lang(),
            )),
            Err(err) => {
                eprintln!("never-sleep cloud identity: {err}");
                None
            }
        }
    } else {
        None
    };
    let mut pairing = None;

    let t = engine.config.tr();
    println!("{}", t.foreground_started());
    println!("{}", t.foreground_status_hint());

    while running.load(Ordering::SeqCst) && engine.is_active() {
        if cloud.is_some() && try_send(&IpcRequest::Ping).is_some() {
            if let Some(handle) = cloud.take() {
                crate::cloud::publish_and_flush(
                    handle,
                    engine.json_status(&platform.snapshot()),
                    engine.config.lang(),
                );
            }
        }
        dispatch(&mut engine, platform, Input::Tick);
        if let Some(handle) = cloud.as_ref() {
            crate::cloud::sync_cloud(&mut engine, platform, handle, &mut pairing);
        }
        if !engine.is_active() {
            break;
        }
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
    if let Some(handle) = cloud {
        crate::cloud::publish_and_flush(
            handle,
            engine.json_status(&platform.snapshot()),
            engine.config.lang(),
        );
    }
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

    #[test]
    fn foreground_does_not_spawn_a_second_cloud_reporter() {
        let src = include_str!("foreground.rs");
        let start = src.find("pub fn run_foreground").expect("run_foreground");
        let body = src[start..]
            .split("pub fn parse_optional_duration")
            .next()
            .unwrap();
        assert!(
            body.contains("try_send") && body.contains("IpcRequest::Ping"),
            "foreground must not spawn a reporter while the menu process is already serving IPC"
        );
        assert!(
            body.contains("publish_and_flush") && body.contains("take()"),
            "a later menu launch must stop the foreground reporter so only one process heartbeats"
        );
    }

    #[test]
    fn foreground_joins_reporter_after_final_status() {
        let src = include_str!("foreground.rs");
        let start = src.find("pub fn run_foreground").expect("run_foreground");
        let body = src[start..]
            .split("pub fn parse_optional_duration")
            .next()
            .unwrap();
        assert!(
            body.contains("publish_and_flush") || body.contains("flush_and_join"),
            "process exit must wait for the inactive heartbeat POST"
        );
    }
}
