use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::thread;
use std::time::Duration;

use never_sleep_core::{
    apply_remote_command, identity_from_bytes, CloudIdentity, Engine, JsonStatus, Lang,
    RemoteCommand, PUBLIC_SITE_ORIGIN,
};
use serde::{Deserialize, Serialize};

use crate::apply::apply_effects;
use crate::paths::{cloud_identity_path, ensure_data_dir};
use crate::platform::Platform;

pub const CLOUD_URL_ENV: &str = "NEVER_SLEEP_CLOUD_URL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudEvent {
    Pairing { code: String, url: String },
    PairingCleared,
    Commands(Vec<RemoteCommand>),
}

pub struct CloudHandle {
    status_tx: SyncSender<(JsonStatus, Lang)>,
    events: mpsc::Receiver<CloudEvent>,
}

impl CloudHandle {
    pub fn push_status(&self, status: JsonStatus, lang: Lang) {
        let _ = self.status_tx.try_send((status, lang));
    }

    pub fn poll_events(&self) -> Vec<CloudEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }
}

/// Retry pair/start until it succeeds, and again after unauthorized heartbeats
/// or an expired pairing offer.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReporterGate {
    registered: bool,
}

impl ReporterGate {
    pub fn needs_pair_start(&self) -> bool {
        !self.registered
    }

    pub fn on_pair_start_ok(&mut self) {
        self.registered = true;
    }

    pub fn on_unauthorized(&mut self) {
        self.registered = false;
    }

    pub fn on_pairing_cleared(&mut self) {
        self.registered = false;
    }
}

/// Dedup remote commands by id and remember ids to ack on the next heartbeat.
#[derive(Debug, Default)]
pub struct CommandInbox {
    seen: Vec<String>,
    known: HashSet<String>,
}

impl CommandInbox {
    pub fn take_new(&mut self, commands: Vec<RemoteCommand>) -> Vec<RemoteCommand> {
        let mut out = Vec::new();
        for cmd in commands {
            if self.known.insert(cmd.id.clone()) {
                self.seen.push(cmd.id.clone());
                out.push(cmd);
            }
        }
        const CAP: usize = 64;
        if self.seen.len() > CAP {
            let drop_n = self.seen.len() - CAP;
            for id in self.seen.drain(..drop_n) {
                self.known.remove(&id);
            }
        }
        out
    }

    pub fn ack_ids(&self) -> &[String] {
        &self.seen
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

/// Parse a gethostname / C-string buffer. Used so tests can lock the Mac name
/// path without touching /etc/hostname.
pub fn hostname_from_c_buffer(buf: &[u8]) -> Option<String> {
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let s = std::str::from_utf8(&buf[..nul]).ok()?.trim();
    if s.is_empty() {
        return None;
    }
    Some(s.to_string())
}

fn hostname_from_os() -> Option<String> {
    #[cfg(unix)]
    {
        let mut buf = [0u8; 256];
        // SAFETY: POSIX gethostname writes a NUL-terminated name into `buf`.
        let rc = unsafe { libc::gethostname(buf.as_mut_ptr() as *mut libc::c_char, buf.len()) };
        if rc == 0 {
            return hostname_from_c_buffer(&buf);
        }
    }
    None
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
    lang: &'a str,
}

#[derive(Debug, Serialize)]
struct HeartbeatBody<'a> {
    device_id: &'a str,
    device_token: &'a str,
    display_name: &'a str,
    lang: &'a str,
    ack_command_ids: &'a [String],
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
    lang: &str,
    ack_command_ids: &[String],
) -> String {
    serde_json::to_string(&HeartbeatBody {
        device_id: &identity.device_id,
        device_token: &identity.device_token,
        display_name,
        lang,
        ack_command_ids,
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

impl HeartbeatOutcome {
    pub fn pairing_present(&self) -> bool {
        self.pairing_code.is_some() && self.pairing_url.is_some()
    }

    pub fn pairing_cleared(&self) -> bool {
        !self.pairing_present()
    }
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

#[derive(Debug)]
pub(crate) enum CloudPost {
    Ok(String),
    Unauthorized,
}

pub(crate) trait CloudTransport {
    fn post_json(&self, path: &str, body: &str) -> Result<CloudPost, String>;
}

pub fn spawn_reporter(identity: CloudIdentity, display_name: String, lang: Lang) -> CloudHandle {
    let (status_tx, status_rx) = mpsc::sync_channel::<(JsonStatus, Lang)>(1);
    let (event_tx, event_rx) = mpsc::channel();
    thread::Builder::new()
        .name("never-sleep-cloud".into())
        .spawn(move || {
            reporter_loop(
                identity,
                display_name,
                lang,
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
    mut lang: Lang,
    transport: UreqTransport,
    status_rx: mpsc::Receiver<(JsonStatus, Lang)>,
    event_tx: mpsc::Sender<CloudEvent>,
) {
    let mut gate = ReporterGate::default();
    let mut inbox = CommandInbox::default();
    let mut last: Option<JsonStatus> = None;
    loop {
        match status_rx.recv_timeout(Duration::from_secs(3)) {
            Ok((status, next_lang)) => {
                last = Some(status);
                lang = next_lang;
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let Some(status) = last.as_ref() else {
            continue;
        };
        reporter_tick(
            &mut gate,
            &mut inbox,
            &transport,
            &identity,
            &display_name,
            lang.cloud_tag(),
            status,
            &event_tx,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reporter_tick(
    gate: &mut ReporterGate,
    inbox: &mut CommandInbox,
    transport: &impl CloudTransport,
    identity: &CloudIdentity,
    display_name: &str,
    lang: &str,
    status: &JsonStatus,
    event_tx: &mpsc::Sender<CloudEvent>,
) {
    if gate.needs_pair_start() {
        let body = serde_json::to_string(&PairStartBody {
            device_id: &identity.device_id,
            device_token: &identity.device_token,
            display_name,
            lang,
        })
        .expect("pair json");
        match transport.post_json("/api/pair/start", &body) {
            Ok(CloudPost::Ok(raw)) => {
                if let Ok((code, url)) = parse_pair_start_response(&raw) {
                    gate.on_pair_start_ok();
                    let _ = event_tx.send(CloudEvent::Pairing { code, url });
                }
            }
            Ok(CloudPost::Unauthorized) => gate.on_unauthorized(),
            Err(_) => {}
        }
    }

    let body = heartbeat_request_json(identity, display_name, status, lang, inbox.ack_ids());
    match transport.post_json("/api/heartbeat", &body) {
        Ok(CloudPost::Unauthorized) => gate.on_unauthorized(),
        Ok(CloudPost::Ok(raw)) => {
            if let Ok(outcome) = parse_heartbeat_response(&raw) {
                emit_outcome(gate, inbox, event_tx, outcome);
            }
        }
        Err(_) => {}
    }
}

fn emit_outcome(
    gate: &mut ReporterGate,
    inbox: &mut CommandInbox,
    event_tx: &mpsc::Sender<CloudEvent>,
    outcome: HeartbeatOutcome,
) {
    if outcome.pairing_cleared() {
        gate.on_pairing_cleared();
        let _ = event_tx.send(CloudEvent::PairingCleared);
    } else if let (Some(code), Some(url)) = (outcome.pairing_code, outcome.pairing_url) {
        let _ = event_tx.send(CloudEvent::Pairing { code, url });
    }
    let commands = inbox.take_new(outcome.commands);
    if !commands.is_empty() {
        let _ = event_tx.send(CloudEvent::Commands(commands));
    }
}

struct UreqTransport {
    origin: String,
}

impl CloudTransport for UreqTransport {
    fn post_json(&self, path: &str, body: &str) -> Result<CloudPost, String> {
        let url = format!("{}{path}", self.origin.trim_end_matches('/'));
        match ureq::post(&url)
            .timeout(Duration::from_secs(3))
            .set("content-type", "application/json")
            .send_string(body)
        {
            Ok(resp) => resp
                .into_string()
                .map(CloudPost::Ok)
                .map_err(|e| e.to_string()),
            Err(ureq::Error::Status(401, _)) => Ok(CloudPost::Unauthorized),
            Err(ureq::Error::Status(code, resp)) => {
                if code == 401 {
                    return Ok(CloudPost::Unauthorized);
                }
                let text = resp.into_string().unwrap_or_default();
                Err(format!("{code}: {text}"))
            }
            Err(e) => Err(e.to_string()),
        }
    }
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
            CloudEvent::PairingCleared => *pairing = None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::TestDataDir;
    use crate::platform::StubPlatform;
    use never_sleep_core::{AppConfig, Engine, JsonStatus};
    use std::sync::Mutex;

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
        let v: serde_json::Value = serde_json::from_str(&heartbeat_request_json(
            &id,
            "Studio",
            &sample_status(),
            "zh",
            &["cmd-1".into()],
        ))
        .unwrap();
        assert_eq!(v["device_id"], id.device_id);
        assert_eq!(v["device_token"], id.device_token);
        assert_eq!(v["display_name"], "Studio");
        assert_eq!(v["lang"], "zh");
        assert_eq!(v["ack_command_ids"][0], "cmd-1");
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
        assert!(outcome.pairing_present());
        assert_eq!(outcome.commands.len(), 2);
        assert_eq!(outcome.commands[0].cmd, "on");
        assert_eq!(outcome.commands[1].cmd, "off");
    }

    #[test]
    fn heartbeat_rejects_unauthorized_payload() {
        let err = parse_heartbeat_response(r#"{"ok":false,"error":"unauthorized"}"#).unwrap_err();
        assert!(err.contains("rejected"));
    }

    #[test]
    fn expired_pairing_fields_are_a_clear() {
        let outcome = parse_heartbeat_response(
            r#"{"ok":true,"pairing_code":null,"pairing_url":null,"commands":[]}"#,
        )
        .unwrap();
        assert!(outcome.pairing_cleared());
        let (event_tx, event_rx) = mpsc::channel();
        let mut gate = ReporterGate::default();
        gate.on_pair_start_ok();
        let mut inbox = CommandInbox::default();
        emit_outcome(&mut gate, &mut inbox, &event_tx, outcome);
        assert!(gate.needs_pair_start(), "expired offer must re-register");
        assert_eq!(event_rx.try_recv().unwrap(), CloudEvent::PairingCleared);
    }

    #[test]
    fn pairing_cleared_event_drops_gui_code() {
        let (status_tx, _status_rx) = mpsc::sync_channel(1);
        let (event_tx, event_rx) = mpsc::channel();
        event_tx.send(CloudEvent::PairingCleared).unwrap();
        let handle = CloudHandle {
            status_tx,
            events: event_rx,
        };
        let mut pairing = Some(("AB7K-2Q9M".into(), "https://example/board/?code=x".into()));
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert!(pairing.is_none());
    }

    #[test]
    fn hostname_from_c_buffer_reads_gethostname_bytes() {
        assert_eq!(
            hostname_from_c_buffer(b"Studio.local\0trailing"),
            Some("Studio.local".into())
        );
        assert_eq!(hostname_from_c_buffer(b"\0"), None);
        assert!(hostname_from_c_buffer(b"MacBook-Pro.local\0").is_some());
        let src = include_str!("cloud.rs");
        assert!(src.contains("libc::gethostname"));
    }

    #[test]
    fn command_inbox_dedups_and_acks() {
        let mut inbox = CommandInbox::default();
        let first = inbox.take_new(vec![RemoteCommand::on("c1", None)]);
        assert_eq!(first.len(), 1);
        let again = inbox.take_new(vec![RemoteCommand::on("c1", None)]);
        assert!(again.is_empty());
        assert_eq!(inbox.ack_ids(), ["c1"]);
    }

    struct ScriptedTransport {
        pair: Mutex<Vec<Result<CloudPost, String>>>,
        beat: Mutex<Vec<Result<CloudPost, String>>>,
        pair_calls: Mutex<usize>,
        beat_calls: Mutex<usize>,
    }

    impl CloudTransport for ScriptedTransport {
        fn post_json(&self, path: &str, _body: &str) -> Result<CloudPost, String> {
            if path.ends_with("/pair/start") {
                *self.pair_calls.lock().unwrap() += 1;
                self.pair.lock().unwrap().remove(0)
            } else {
                *self.beat_calls.lock().unwrap() += 1;
                self.beat.lock().unwrap().remove(0)
            }
        }
    }

    #[test]
    fn reporter_retries_pair_start_after_transient_failure_and_401() {
        let transport = ScriptedTransport {
            pair: Mutex::new(vec![
                Err("timeout".into()),
                Ok(CloudPost::Ok(
                    r#"{"ok":true,"pairing_code":"AB7K-2Q9M","pairing_url":"https://x/board/?code=AB7K-2Q9M"}"#
                        .into(),
                )),
                Ok(CloudPost::Ok(
                    r#"{"ok":true,"pairing_code":"ZZZZ-YYYY","pairing_url":"https://x/board/?code=ZZZZ-YYYY"}"#
                        .into(),
                )),
            ]),
            beat: Mutex::new(vec![
                Ok(CloudPost::Ok(
                    r#"{"ok":true,"pairing_code":null,"pairing_url":null,"commands":[]}"#.into(),
                )),
                Ok(CloudPost::Ok(
                    r#"{"ok":true,"pairing_code":"AB7K-2Q9M","pairing_url":"https://x/board/?code=AB7K-2Q9M","commands":[]}"#
                        .into(),
                )),
                Ok(CloudPost::Unauthorized),
                Ok(CloudPost::Ok(
                    r#"{"ok":true,"pairing_code":"ZZZZ-YYYY","pairing_url":"https://x/board/?code=ZZZZ-YYYY","commands":[]}"#
                        .into(),
                )),
            ]),
            pair_calls: Mutex::new(0),
            beat_calls: Mutex::new(0),
        };
        let id = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        let mut gate = ReporterGate::default();
        let mut inbox = CommandInbox::default();
        let (event_tx, _event_rx) = mpsc::channel();
        let status = sample_status();

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx,
        );
        assert!(gate.needs_pair_start(), "first pair/start timed out");
        assert_eq!(*transport.pair_calls.lock().unwrap(), 1);

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx,
        );
        assert!(
            !gate.needs_pair_start(),
            "successful pair/start plus a live offer registers the device"
        );
        assert_eq!(*transport.pair_calls.lock().unwrap(), 2);

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx,
        );
        assert!(
            gate.needs_pair_start(),
            "unauthorized heartbeat must retry pair/start"
        );
        assert_eq!(*transport.pair_calls.lock().unwrap(), 2);

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx,
        );
        assert!(!gate.needs_pair_start());
        assert_eq!(*transport.pair_calls.lock().unwrap(), 3);
        assert_eq!(*transport.beat_calls.lock().unwrap(), 4);
    }

    #[test]
    fn reporter_source_does_not_pair_start_once() {
        let src = include_str!("cloud.rs");
        assert!(src.contains("needs_pair_start"));
        assert!(src.contains("on_unauthorized"));
    }
}
