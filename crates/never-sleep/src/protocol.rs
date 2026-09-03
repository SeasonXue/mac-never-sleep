use serde::{Deserialize, Serialize};

use never_sleep_core::{parse_duration_pref_in, DurationPref, JsonStatus, Lang, Tr};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    On {
        #[serde(default)]
        duration: Option<String>,
        /// Leftover seconds of a timed session. Only set on an internal handoff.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        remaining_secs: Option<u64>,
        /// Elapsed seconds of the live session. Only set on an internal handoff.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        elapsed_secs: Option<u64>,
        /// Adopt a live session with remote display semantics (do not fight HID).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        handoff: bool,
    },
    Off,
    Toggle,
    Status,
    Quit,
    Ping,
    Pair,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pairing_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
}

impl IpcRequest {
    pub fn on(duration: Option<String>) -> Self {
        Self::On {
            duration,
            remaining_secs: None,
            elapsed_secs: None,
            handoff: false,
        }
    }

    pub fn handoff(
        duration: Option<String>,
        remaining_secs: Option<u64>,
        elapsed_secs: Option<u64>,
    ) -> Self {
        Self::On {
            duration,
            remaining_secs,
            elapsed_secs,
            handoff: true,
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub fn is_handoff(&self) -> bool {
        matches!(self, Self::On { handoff: true, .. })
    }
}

impl IpcResponse {
    pub fn ok_status(status: JsonStatus) -> Self {
        Self {
            ok: true,
            error: None,
            status: Some(status),
            pong: None,
            pairing_code: None,
            pairing_url: None,
            device_id: None,
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
            status: None,
            pong: None,
            pairing_code: None,
            pairing_url: None,
            device_id: None,
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub fn pong() -> Self {
        Self {
            ok: true,
            error: None,
            status: None,
            pong: Some(true),
            pairing_code: None,
            pairing_url: None,
            device_id: None,
        }
    }

    #[cfg(any(test, target_os = "macos"))]
    pub fn ok_pairing(code: String, url: String, device_id: Option<String>) -> Self {
        Self {
            ok: true,
            error: None,
            status: None,
            pong: None,
            pairing_code: Some(code),
            pairing_url: Some(url),
            device_id,
        }
    }
}

pub fn parse_on_duration_in(raw: Option<&str>, lang: Lang) -> Result<Option<DurationPref>, String> {
    match raw {
        None => Ok(None),
        Some(s) => parse_duration_pref_in(s, lang).map(Some),
    }
}

/// Stable CLI/IPC spelling for a duration so a foreground fallback can hand
/// the live session to the menu with `IpcRequest::On`.
pub fn duration_pref_to_ipc(pref: DurationPref) -> String {
    match pref {
        DurationPref::Indefinite => "indefinite".into(),
        DurationPref::Hours { hours } => format!("{hours}h"),
        DurationPref::UntilLocal { hour, minute } => {
            format!("until={hour:02}:{minute:02}")
        }
    }
}

pub fn menu_accepted_handoff(resp: &IpcResponse) -> bool {
    resp.ok && resp.status.as_ref().is_some_and(|status| status.active)
}

/// Map stable IPC error codes to bilingual CLI text. JSON still prints the code.
pub fn human_ipc_error(code: &str, lang: Lang) -> String {
    match code {
        "" => Tr::new(lang).failed().to_string(),
        "pairing_unavailable" => Tr::new(lang).pairing_unavailable().to_string(),
        other => other.to_string(),
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
                    remaining_secs: None,
                    elapsed_secs: None,
                    handoff: false,
                },
                r#"{"cmd":"on","duration":"8h"}"#,
            ),
            (IpcRequest::Off, r#"{"cmd":"off"}"#),
            (IpcRequest::Toggle, r#"{"cmd":"toggle"}"#),
            (IpcRequest::Status, r#"{"cmd":"status"}"#),
            (IpcRequest::Quit, r#"{"cmd":"quit"}"#),
            (IpcRequest::Ping, r#"{"cmd":"ping"}"#),
            (IpcRequest::Pair, r#"{"cmd":"pair"}"#),
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
        assert_eq!(duration_pref_to_ipc(DurationPref::Indefinite), "indefinite");
        assert_eq!(duration_pref_to_ipc(DurationPref::Hours { hours: 8 }), "8h");
        assert_eq!(
            duration_pref_to_ipc(DurationPref::UntilLocal { hour: 8, minute: 0 }),
            "until=08:00"
        );
    }

    #[test]
    fn handoff_on_carries_remaining_secs() {
        let req = IpcRequest::handoff(Some("8h".into()), Some(3600), Some(7 * 3600));
        let json = serde_json::to_string(&req).unwrap();
        assert_eq!(
            json,
            r#"{"cmd":"on","duration":"8h","remaining_secs":3600,"elapsed_secs":25200,"handoff":true}"#
        );
        let back: IpcRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back, req);
        let local = IpcRequest::on(Some("8h".into()));
        assert_eq!(
            serde_json::to_string(&local).unwrap(),
            r#"{"cmd":"on","duration":"8h"}"#
        );
        let mut accepted = sample_status();
        accepted.active = true;
        assert!(menu_accepted_handoff(&IpcResponse::ok_status(
            accepted.clone()
        )));
        accepted.active = false;
        assert!(!menu_accepted_handoff(&IpcResponse::ok_status(accepted)));
        assert!(!menu_accepted_handoff(&IpcResponse::err("denied")));
        let ping = IpcResponse::pong();
        assert!(!menu_accepted_handoff(&ping));
        assert!(req.is_handoff());
        assert!(!IpcRequest::on(Some("8h".into())).is_handoff());
        assert!(!IpcRequest::Ping.is_handoff());
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
        let pair = IpcResponse::ok_pairing(
            "AB7K-2Q9M".into(),
            "https://xyz-ai.app/never-sleep/board/?code=AB7K-2Q9M".into(),
            Some("ab".repeat(16)),
        );
        let v = serde_json::to_value(&pair).unwrap();
        assert_eq!(v["pairing_code"], "AB7K-2Q9M");
        assert_eq!(
            v["pairing_url"],
            "https://xyz-ai.app/never-sleep/board/?code=AB7K-2Q9M"
        );
        assert_eq!(v["device_id"], "ab".repeat(16));
        assert!(v.get("status").is_none());
        let status_only = IpcResponse::ok_status(sample_status());
        let status_json = serde_json::to_value(&status_only).unwrap();
        assert!(status_json.get("pairing_code").is_none());
        assert!(status_json.get("pairing_url").is_none());
        assert!(status_json.get("device_id").is_none());
        let json_err = IpcResponse::err("pairing_unavailable");
        assert_eq!(
            serde_json::to_value(&json_err).unwrap()["error"],
            "pairing_unavailable"
        );
        assert_ne!(
            human_ipc_error("pairing_unavailable", Lang::Zh),
            "pairing_unavailable"
        );
        assert_eq!(
            human_ipc_error("pairing_unavailable", Lang::En),
            never_sleep_core::Tr::new(Lang::En).pairing_unavailable()
        );
    }
}
