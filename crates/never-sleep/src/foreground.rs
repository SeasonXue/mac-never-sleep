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
        if let Some(handle) = cloud.as_ref() {
            crate::cloud::apply_polled_commands(&mut engine, platform, handle, &mut pairing);
        }
        if !engine.is_active() {
            break;
        }
        if try_send(&IpcRequest::Ping).is_some() {
            if let Some(handle) = cloud.as_ref() {
                handle.quiesce();
                crate::cloud::apply_polled_commands(&mut engine, platform, handle, &mut pairing);
            }
            if !engine.is_active() {
                break;
            }
            let req = handoff_request(&engine, &platform.snapshot());
            if let Some(resp) = try_send(&req) {
                if crate::protocol::menu_accepted_handoff(&resp) {
                    if let Some(handle) = cloud.take() {
                        handle.detach();
                    }
                    if engine.is_active() {
                        dispatch(
                            &mut engine,
                            platform,
                            Input::Stop {
                                reason: StopReason::AppQuit,
                            },
                        );
                    }
                    stop_for_quit(&mut engine, platform);
                    println!("{}", engine.config.tr().foreground_ended());
                    return Ok(());
                }
            }
            if let Some(handle) = cloud.as_ref() {
                handle.resume();
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

fn handoff_request(engine: &Engine, host: &never_sleep_core::HostSnapshot) -> IpcRequest {
    let status = engine.json_status(host);
    IpcRequest::handoff(
        Some(crate::protocol::duration_pref_to_ipc(
            engine.config.duration,
        )),
        status.remaining_secs,
        status.elapsed_secs,
    )
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
    fn foreground_hands_off_engine_when_menu_appears() {
        let src = include_str!("foreground.rs");
        let start = src.find("pub fn run_foreground").expect("run_foreground");
        let body = src[start..]
            .split("pub fn parse_optional_duration")
            .next()
            .unwrap();
        let ping_at = body
            .find("IpcRequest::Ping")
            .expect("menu presence is detected with Ping");
        let after_ping = &body[ping_at..];
        let handoff = after_ping
            .split("return Ok(())")
            .next()
            .expect("handoff returns after a successful adopt");
        assert!(
            handoff.contains("handoff_request"),
            "hand the leftover duration, not the original Hours preference"
        );
        assert!(
            src.contains("remaining_secs") && src.contains("elapsed_secs"),
            "handoff IPC must carry leftover and elapsed seconds of the live session"
        );
        let loop_at = body.find("while running").expect("foreground loop");
        let loop_body = &body[loop_at..];
        let drain_at = loop_body
            .find("apply_polled_commands")
            .expect("drain the foreground reporter before handing off");
        let quiesce_at = loop_body
            .find("quiesce(")
            .expect("quiesce the reporter so an in-flight heartbeat cannot ack during IPC");
        let adopt_at = loop_body
            .find("handoff_request")
            .expect("handoff_request after drain");
        assert!(
            drain_at < quiesce_at,
            "apply queued commands before waiting for the in-flight heartbeat"
        );
        assert!(
            quiesce_at < adopt_at,
            "a heartbeat that returns Off during Ping must be drained before detach()"
        );
        let after_quiesce = &loop_body[quiesce_at..];
        let second_drain = after_quiesce
            .find("apply_polled_commands")
            .expect("drain commands delivered by the in-flight heartbeat");
        let adopt_after = after_quiesce
            .find("handoff_request")
            .expect("handoff after the post-quiesce drain");
        assert!(
            second_drain < adopt_after,
            "transfer the last phone Off before relinquishing the reporter"
        );
        assert!(
            handoff.contains("menu_accepted_handoff"),
            "keep the foreground session unless the menu reports ok && active"
        );
        assert!(
            handoff.contains("detach(") && !handoff.contains("publish_and_flush"),
            "a live handoff must not POST offline:true after the menu is already heartbeating"
        );
        assert!(
            handoff.contains("StopReason::AppQuit") && handoff.contains("stop_for_quit"),
            "handoff must release assertions silently; AppQuit does not notify 待命已结束"
        );
        assert!(
            !handoff.contains("StopReason::User"),
            "User stop would show an ended notification after the menu already said started"
        );
        assert!(
            after_ping.contains("return Ok(())"),
            "exit the fallback loop after handoff so Tick cannot re-apply assertions"
        );
        let accept_at = loop_body
            .find("menu_accepted_handoff")
            .expect("accepted handoff");
        let resume_at = loop_body
            .find("resume(")
            .expect("unpark the reporter when the menu does not take over");
        assert!(
            accept_at < resume_at,
            "only resume after the adopt decision, never instead of quiesce"
        );
        let after_accept = &loop_body[loop_body.find("return Ok(())").expect("success return")..];
        assert!(
            after_accept.contains("resume("),
            "Ping-then-gone or a rejected On must resume heartbeats; paused has no other exit"
        );
    }

    #[test]
    fn handoff_request_sends_remaining_secs_not_full_pref() {
        use never_sleep_core::{AppConfig, DurationPref, HostSnapshot, Input, Thermal};
        let mut engine = Engine::new(AppConfig {
            duration: DurationPref::Hours { hours: 8 },
            display_off_delay_ms: 1_500,
            ..AppConfig::default()
        });
        let host = HostSnapshot {
            monotonic_ms: 0,
            continuous_ms: 0,
            unix_secs: 1_700_000_000,
            utc_offset_secs: 0,
            on_ac: true,
            battery_percent: Some(80),
            lid_closed: false,
            display_asleep: Some(false),
            hid_idle_ms: 60_000,
            thermal: Thermal::Nominal,
        };
        engine.handle(Input::StartWith(DurationPref::Hours { hours: 8 }), &host);
        let mut later = host.clone();
        later.monotonic_ms = 7 * 3_600_000;
        later.continuous_ms = 7 * 3_600_000;
        later.unix_secs = host.unix_secs;
        let req = handoff_request(&engine, &later);
        match req {
            IpcRequest::On {
                duration,
                remaining_secs,
                elapsed_secs,
                handoff,
            } => {
                assert_eq!(duration.as_deref(), Some("8h"));
                assert_eq!(remaining_secs, Some(3600));
                assert_eq!(elapsed_secs, Some(7 * 3600));
                assert!(handoff);
            }
            other => panic!("expected On handoff, got {other:?}"),
        }
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
