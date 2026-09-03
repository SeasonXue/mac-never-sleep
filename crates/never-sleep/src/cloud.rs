use std::fs;
use std::io::Read;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::Duration;

use never_sleep_core::{
    apply_remote_command, identity_from_bytes, CloudIdentity, Engine, JsonStatus, RemoteCommand,
    PUBLIC_SITE_ORIGIN,
};
use serde::{Deserialize, Serialize};

use crate::apply::apply_effects;
use crate::paths::{cloud_identity_path, ensure_data_dir};
use crate::platform::Platform;

pub const CLOUD_URL_ENV: &str = "NEVER_SLEEP_CLOUD_URL";

#[derive(Debug, Clone)]
pub enum CloudEvent {
    Pairing { code: String, url: String },
    Commands(Vec<RemoteCommand>),
}

pub struct CloudHandle {
    status_tx: SyncSender<JsonStatus>,
    events: mpsc::Receiver<CloudEvent>,
}

impl CloudHandle {
    pub fn push_status(&self, status: JsonStatus) {
        let _ = self.status_tx.try_send(status);
    }

    pub fn poll_events(&self) -> Vec<CloudEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }
}

pub fn cloud_origin() -> String {
    std::env::var(CLOUD_URL_ENV)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| PUBLIC_SITE_ORIGIN.to_string())
}

pub fn cloud_enabled() -> bool {
    cfg!(target_os = "macos") || std::env::var(CLOUD_URL_ENV).is_ok()
}

pub fn default_display_name() -> String {
    if let Ok(name) = std::env::var("NEVER_SLEEP_DEVICE_NAME") {
        if !name.trim().is_empty() {
            return name.trim().to_string();
        }
    }
    hostname_from_os().unwrap_or_else(|| "Mac".into())
}

fn hostname_from_os() -> Option<String> {
    for path in ["/etc/hostname", "/proc/sys/kernel/hostname"] {
        if let Ok(name) = fs::read_to_string(path) {
            let trimmed = name.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    std::env::var("HOSTNAME")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn fill_random(buf: &mut [u8]) {
    let mut file = fs::File::open("/dev/urandom").expect("urandom");
    file.read_exact(buf).expect("urandom read");
}

pub fn load_or_create_identity() -> CloudIdentity {
    let path = cloud_identity_path();
    if let Ok(text) = fs::read_to_string(&path) {
        if let Ok(id) = toml::from_str::<CloudIdentity>(&text) {
            if id.device_id.len() >= 16 && id.device_token.len() >= 16 {
                return id;
            }
        }
    }
    let mut id_bytes = [0u8; 16];
    let mut token_bytes = [0u8; 32];
    fill_random(&mut id_bytes);
    fill_random(&mut token_bytes);
    let identity = identity_from_bytes(&id_bytes, &token_bytes);
    save_identity(&identity);
    identity
}

fn save_identity(identity: &CloudIdentity) {
    if ensure_data_dir().is_err() {
        return;
    }
    if let Ok(text) = toml::to_string_pretty(identity) {
        let _ = fs::write(cloud_identity_path(), text);
    }
}

#[derive(Debug, Serialize)]
struct PairStartBody<'a> {
    device_id: &'a str,
    device_token: &'a str,
    display_name: &'a str,
}

#[derive(Debug, Serialize)]
struct HeartbeatBody<'a> {
    device_id: &'a str,
    device_token: &'a str,
    display_name: &'a str,
    status: &'a JsonStatus,
}

#[derive(Debug, Deserialize)]
struct PairStartResponse {
    ok: bool,
    #[serde(default)]
    pairing_code: Option<String>,
    #[serde(default)]
    pairing_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct HeartbeatResponse {
    ok: bool,
    #[serde(default)]
    pairing_code: Option<String>,
    #[serde(default)]
    pairing_url: Option<String>,
    #[serde(default)]
    commands: Vec<RemoteCommand>,
}

pub fn heartbeat_request_json(
    identity: &CloudIdentity,
    display_name: &str,
    status: &JsonStatus,
) -> String {
    serde_json::to_string(&HeartbeatBody {
        device_id: &identity.device_id,
        device_token: &identity.device_token,
        display_name,
        status,
    })
    .expect("heartbeat json")
}

pub fn parse_heartbeat_response(raw: &str) -> Result<HeartbeatOutcome, String> {
    let parsed: HeartbeatResponse = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    if !parsed.ok {
        return Err("heartbeat rejected".into());
    }
    Ok(HeartbeatOutcome {
        pairing_code: parsed.pairing_code,
        pairing_url: parsed.pairing_url,
        commands: parsed
            .commands
            .into_iter()
            .filter(|c| RemoteCommand::is_allowed_cmd(&c.cmd))
            .collect(),
    })
}

#[derive(Debug, Clone)]
pub struct HeartbeatOutcome {
    pub pairing_code: Option<String>,
    pub pairing_url: Option<String>,
    pub commands: Vec<RemoteCommand>,
}

pub fn parse_pair_start_response(raw: &str) -> Result<(String, String), String> {
    let parsed: PairStartResponse = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    match (parsed.ok, parsed.pairing_code, parsed.pairing_url) {
        (true, Some(code), Some(url)) => Ok((code, url)),
        _ => Err("pair start rejected".into()),
    }
}

pub fn apply_cloud_commands(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    commands: &[RemoteCommand],
) {
    for cmd in commands {
        let host = platform.snapshot();
        if let Ok(effects) = apply_remote_command(engine, &host, cmd) {
            apply_effects(engine, platform, &effects);
        }
    }
}

pub fn spawn_reporter(identity: CloudIdentity, display_name: String) -> CloudHandle {
    let (status_tx, status_rx) = mpsc::sync_channel::<JsonStatus>(1);
    let (event_tx, event_rx) = mpsc::channel();
    thread::Builder::new()
        .name("never-sleep-cloud".into())
        .spawn(move || {
            reporter_loop(
                identity,
                display_name,
                UreqTransport {
                    origin: cloud_origin(),
                },
                status_rx,
                event_tx,
            );
        })
        .ok();
    CloudHandle {
        status_tx,
        events: event_rx,
    }
}

fn reporter_loop(
    identity: CloudIdentity,
    display_name: String,
    transport: UreqTransport,
    status_rx: mpsc::Receiver<JsonStatus>,
    event_tx: mpsc::Sender<CloudEvent>,
) {
    post_pair_start(&transport, &identity, &display_name, &event_tx);
    let mut last: Option<JsonStatus> = None;
    loop {
        match status_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(status) => last = Some(status),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let Some(status) = last.as_ref() else {
            continue;
        };
        let body = heartbeat_request_json(&identity, &display_name, status);
        if let Ok(raw) = transport.post_json("/api/heartbeat", &body) {
            if let Ok(outcome) = parse_heartbeat_response(&raw) {
                emit_outcome(&event_tx, outcome);
            }
        }
    }
}

fn post_pair_start(
    transport: &UreqTransport,
    identity: &CloudIdentity,
    display_name: &str,
    event_tx: &mpsc::Sender<CloudEvent>,
) {
    let body = serde_json::to_string(&PairStartBody {
        device_id: &identity.device_id,
        device_token: &identity.device_token,
        display_name,
    })
    .expect("pair json");
    if let Ok(raw) = transport.post_json("/api/pair/start", &body) {
        if let Ok((code, url)) = parse_pair_start_response(&raw) {
            let _ = event_tx.send(CloudEvent::Pairing { code, url });
        }
    }
}

fn emit_outcome(event_tx: &mpsc::Sender<CloudEvent>, outcome: HeartbeatOutcome) {
    if let (Some(code), Some(url)) = (outcome.pairing_code, outcome.pairing_url) {
        let _ = event_tx.send(CloudEvent::Pairing { code, url });
    }
    if !outcome.commands.is_empty() {
        let _ = event_tx.send(CloudEvent::Commands(outcome.commands));
    }
}

struct UreqTransport {
    origin: String,
}

impl UreqTransport {
    fn post_json(&self, path: &str, body: &str) -> Result<String, String> {
        let url = format!("{}{path}", self.origin.trim_end_matches('/'));
        ureq::post(&url)
            .timeout(Duration::from_secs(3))
            .set("content-type", "application/json")
            .send_string(body)
            .map_err(|e| e.to_string())?
            .into_string()
            .map_err(|e| e.to_string())
    }
}

pub fn request_pairing_code(
    identity: &CloudIdentity,
    display_name: &str,
) -> Result<(String, String), String> {
    let transport = UreqTransport {
        origin: cloud_origin(),
    };
    let body = serde_json::to_string(&PairStartBody {
        device_id: &identity.device_id,
        device_token: &identity.device_token,
        display_name,
    })
    .map_err(|e| e.to_string())?;
    let raw = transport.post_json("/api/pair/start", &body)?;
    parse_pair_start_response(&raw)
}

/// Apply pending remote commands using the same Engine path as local IPC.
pub fn apply_polled_commands(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    handle: &CloudHandle,
    pairing: &mut Option<(String, String)>,
) {
    for event in handle.poll_events() {
        match event {
            CloudEvent::Commands(commands) => {
                apply_cloud_commands(engine, platform, &commands);
            }
            CloudEvent::Pairing { code, url } => *pairing = Some((code, url)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::TestDataDir;
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
    fn load_or_create_identity_persists_random_token() {
        let _dir = TestDataDir::install();
        let first = load_or_create_identity();
        let second = load_or_create_identity();
        assert_eq!(first, second);
        assert_eq!(first.device_id.len(), 32);
        assert_eq!(first.device_token.len(), 64);
        assert_ne!(first.device_id, first.device_token);
        let text = fs::read_to_string(cloud_identity_path()).unwrap();
        assert!(text.contains("device_id"));
        assert!(text.contains("device_token"));
    }

    #[test]
    fn heartbeat_json_uses_stable_field_names() {
        let id = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        let v: serde_json::Value =
            serde_json::from_str(&heartbeat_request_json(&id, "Studio", &sample_status())).unwrap();
        assert_eq!(v["device_id"], id.device_id);
        assert_eq!(v["device_token"], id.device_token);
        assert_eq!(v["display_name"], "Studio");
        assert_eq!(v["status"]["active"], true);
        assert_eq!(v["status"]["display"], "asleep");
        assert!(v["cmd"].is_null());
    }

    #[test]
    fn heartbeat_response_keeps_on_off_drops_toggle() {
        let raw = r#"{
            "ok": true,
            "pairing_code": "AB7K-2Q9M",
            "pairing_url": "https://xyz-ai.app/never-sleep/board/?code=AB7K-2Q9M",
            "commands": [
                {"id": "1", "cmd": "on", "duration": "8h"},
                {"id": "2", "cmd": "toggle"},
                {"id": "3", "cmd": "off"}
            ]
        }"#;
        let outcome = parse_heartbeat_response(raw).unwrap();
        assert_eq!(outcome.pairing_code.as_deref(), Some("AB7K-2Q9M"));
        assert_eq!(outcome.commands.len(), 2);
        assert_eq!(outcome.commands[0].cmd, "on");
        assert_eq!(outcome.commands[1].cmd, "off");
    }

    #[test]
    fn heartbeat_rejects_unauthorized_payload() {
        let err = parse_heartbeat_response(r#"{"ok":false,"error":"unauthorized"}"#).unwrap_err();
        assert!(err.contains("rejected"));
    }
}
