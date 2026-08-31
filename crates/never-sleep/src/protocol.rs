use serde::{Deserialize, Serialize};

use never_sleep_core::{parse_duration_pref_in, DurationPref, JsonStatus, Lang};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    On {
        #[serde(default)]
        duration: Option<String>,
    },
    Off,
    Toggle,
    Status,
    Quit,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<JsonStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pong: Option<bool>,
}

impl IpcResponse {
    pub fn ok_status(status: JsonStatus) -> Self {
        Self {
            ok: true,
            error: None,
            status: Some(status),
            pong: None,
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
            status: None,
            pong: None,
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub fn pong() -> Self {
        Self {
            ok: true,
            error: None,
            status: None,
            pong: Some(true),
        }
    }
}

pub fn parse_on_duration_in(raw: Option<&str>, lang: Lang) -> Result<Option<DurationPref>, String> {
    match raw {
        None => Ok(None),
        Some(s) => parse_duration_pref_in(s, lang).map(Some),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use never_sleep_core::JsonStatus;

    fn sample_status() -> JsonStatus {
        JsonStatus {
            active: true,
            display: "asleep".into(),
            lid: "open".into(),
            on_ac: true,
            battery: Some(80),
            remaining_secs: Some(3600),
            user_present: false,
            elapsed_secs: Some(12),
            stop_reason: None,
            stop_reason_code: None,
            screen_off_enabled: true,
            lid_awake_enabled: true,
        }
    }

    #[test]
    fn request_cmd_tags_are_stable() {
        let cases = [
            (
                IpcRequest::On {
                    duration: Some("8h".into()),
                },
                r#"{"cmd":"on","duration":"8h"}"#,
            ),
            (IpcRequest::Off, r#"{"cmd":"off"}"#),
            (IpcRequest::Toggle, r#"{"cmd":"toggle"}"#),
            (IpcRequest::Status, r#"{"cmd":"status"}"#),
            (IpcRequest::Quit, r#"{"cmd":"quit"}"#),
            (IpcRequest::Ping, r#"{"cmd":"ping"}"#),
        ];
        for (req, expected) in cases {
            let json = serde_json::to_string(&req).unwrap();
            assert_eq!(json, expected);
            let back: IpcRequest = serde_json::from_str(&json).unwrap();
            assert_eq!(back, req);
        }
    }

    #[test]
    fn parse_on_duration_none_and_hours() {
        assert_eq!(parse_on_duration_in(None, Lang::En).unwrap(), None);
        assert_eq!(
            parse_on_duration_in(Some("3h"), Lang::En).unwrap(),
            Some(DurationPref::Hours { hours: 3 })
        );
        let err = parse_on_duration_in(Some("nope"), Lang::Zh).unwrap_err();
        assert!(err.contains("HH:MM"), "{err}");
        assert_ne!(
            err,
            parse_on_duration_in(Some("nope"), Lang::En).unwrap_err()
        );
    }

    #[test]
    fn response_helpers_roundtrip() {
        let ok = IpcResponse::ok_status(sample_status());
        assert!(ok.ok);
        assert_eq!(ok.status.as_ref().unwrap().display, "asleep");
        let err = IpcResponse::err("boom");
        assert!(!err.ok);
        assert_eq!(err.error.as_deref(), Some("boom"));
        let pong = IpcResponse::pong();
        assert_eq!(pong.pong, Some(true));
    }
}
