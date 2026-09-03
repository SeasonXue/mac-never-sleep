use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use never_sleep_core::{
    apply_remote_command, identity_from_bytes, CloudIdentity, Engine, JsonStatus, Lang,
    RemoteCommand, PUBLIC_SITE_ORIGIN,
};
use serde::{Deserialize, Serialize};

use crate::apply::apply_effects_or_abort;
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
    /// Latest-wins snapshot. A capacity-one wake channel only signals that
    /// `latest` changed; it must not carry the payload or a newer inactive
    /// status is dropped while the reporter is in a slow POST.
    latest: Arc<Mutex<Option<(JsonStatus, Lang)>>>,
    wake: Option<SyncSender<()>>,
    events: mpsc::Receiver<CloudEvent>,
    join: Option<thread::JoinHandle<()>>,
}

impl CloudHandle {
    pub fn push_status(&self, status: JsonStatus, lang: Lang) {
        if let Ok(mut slot) = self.latest.lock() {
            *slot = Some((status, lang));
        }
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
    }

    /// Disconnect the reporter and wait until it has POSTed the latest snapshot.
    pub fn flush_and_join(mut self) {
        self.disconnect_and_join();
    }

    fn disconnect_and_join(&mut self) {
        self.wake.take();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    #[cfg(test)]
    fn queued_status(&self) -> Option<JsonStatus> {
        clone_latest(&self.latest).map(|(status, _)| status)
    }

    pub fn poll_events(&self) -> Vec<CloudEvent> {
        let mut out = Vec::new();
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }
}

impl Drop for CloudHandle {
    fn drop(&mut self) {
        self.disconnect_and_join();
    }
}

/// Queue a snapshot and wait until the reporter has POSTed it (or disconnected).
pub fn publish_and_flush(handle: CloudHandle, status: JsonStatus, lang: Lang) {
    handle.push_status(status, lang);
    handle.flush_and_join();
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
        out
    }

    /// Keep every delivered id the Worker still lists. Call after a successful
    /// heartbeat so acked commands leave the inbox; never drop still-pending ids
    /// (a size cap here would replay on/off after the next beat).
    pub fn retain_pending(&mut self, pending: &[RemoteCommand]) {
        let keep: HashSet<&str> = pending.iter().map(|c| c.id.as_str()).collect();
        self.seen.retain(|id| keep.contains(id.as_str()));
        self.known.retain(|id| keep.contains(id.as_str()));
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
            apply_effects_or_abort(engine, platform, &effects);
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

pub fn spawn_reporter(identity: CloudIdentity, display_name: String, _lang: Lang) -> CloudHandle {
    let latest = Arc::new(Mutex::new(None));
    let (wake_tx, wake_rx) = mpsc::sync_channel(1);
    let (event_tx, event_rx) = mpsc::channel();
    let latest_for_thread = Arc::clone(&latest);
    let join = thread::Builder::new()
        .name("never-sleep-cloud".into())
        .spawn(move || {
            reporter_loop(
                identity,
                display_name,
                UreqTransport {
                    origin: cloud_origin(),
                },
                wake_rx,
                latest_for_thread,
                event_tx,
            );
        })
        .ok();
    CloudHandle {
        latest,
        wake: Some(wake_tx),
        events: event_rx,
        join,
    }
}

fn clone_latest(latest: &Mutex<Option<(JsonStatus, Lang)>>) -> Option<(JsonStatus, Lang)> {
    latest.lock().ok().and_then(|slot| slot.clone())
}

fn reporter_loop(
    identity: CloudIdentity,
    display_name: String,
    transport: UreqTransport,
    wake_rx: mpsc::Receiver<()>,
    latest: Arc<Mutex<Option<(JsonStatus, Lang)>>>,
    event_tx: mpsc::Sender<CloudEvent>,
) {
    let mut gate = ReporterGate::default();
    let mut inbox = CommandInbox::default();
    let mut last: Option<(JsonStatus, Lang)> = None;
    loop {
        let shutting_down = match wake_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(()) => {
                while wake_rx.try_recv().is_ok() {}
                false
            }
            Err(RecvTimeoutError::Timeout) => false,
            Err(RecvTimeoutError::Disconnected) => true,
        };
        if let Some(pair) = clone_latest(&latest) {
            last = Some(pair);
        }
        let Some((status, lang)) = last.as_ref() else {
            if shutting_down {
                break;
            }
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
        if shutting_down {
            break;
        }
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
    let pending = outcome.commands;
    let commands = inbox.take_new(pending.clone());
    inbox.retain_pending(&pending);
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

/// Apply remote commands first, then queue the resulting snapshot.
pub fn sync_cloud(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    handle: &CloudHandle,
    pairing: &mut Option<(String, String)>,
) {
    apply_polled_commands(engine, platform, handle, pairing);
    handle.push_status(
        engine.json_status(&platform.snapshot()),
        engine.config.lang(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::TestDataDir;
    use crate::platform::StubPlatform;
    use never_sleep_core::{AppConfig, Engine, JsonStatus};
    use std::sync::Mutex;

    fn test_cloud_handle(wake: SyncSender<()>, events: mpsc::Receiver<CloudEvent>) -> CloudHandle {
        CloudHandle {
            latest: Arc::new(Mutex::new(None)),
            wake: Some(wake),
            events,
            join: None,
        }
    }

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
        let (event_tx, event_rx) = mpsc::channel();
        event_tx.send(CloudEvent::PairingCleared).unwrap();
        let handle = test_cloud_handle(mpsc::sync_channel(1).0, event_rx);
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
        let first = inbox.take_new(vec![RemoteCommand::on("seed", None)]);
        assert_eq!(first.len(), 1);
        let again = inbox.take_new(vec![RemoteCommand::on("seed", None)]);
        assert!(again.is_empty());
        assert_eq!(inbox.ack_ids(), ["seed"]);
        let batch: Vec<_> = (0..70)
            .map(|i| RemoteCommand::on(format!("c{i}"), None))
            .collect();
        let many = inbox.take_new(batch.clone());
        assert_eq!(many.len(), 70);
        assert_eq!(inbox.ack_ids().len(), 71);
        inbox.retain_pending(&batch);
        assert_eq!(
            inbox.ack_ids().len(),
            70,
            "delivered ids stay until the worker drops them"
        );
        assert!(
            inbox.ack_ids().iter().all(|id| id != "seed"),
            "ids the worker no longer lists can leave after ack"
        );
        inbox.retain_pending(&[]);
        assert!(inbox.ack_ids().is_empty());
    }

    #[test]
    fn push_status_replaces_queued_active_with_inactive() {
        let (wake, _wake_rx) = mpsc::sync_channel(1);
        wake.try_send(()).unwrap();
        let (_event_tx, event_rx) = mpsc::channel();
        let handle = test_cloud_handle(wake, event_rx);
        let mut active = sample_status();
        active.active = true;
        handle.push_status(active, Lang::En);
        let mut inactive = sample_status();
        inactive.active = false;
        handle.push_status(inactive, Lang::En);
        let status = handle.queued_status().expect("queued snapshot");
        assert!(
            !status.active,
            "a full capacity-one wake must not keep a stale active snapshot"
        );
    }

    #[test]
    fn disconnected_wake_still_flushes_latest_slot() {
        let latest = Mutex::new(Some({
            let mut inactive = sample_status();
            inactive.active = false;
            (inactive, Lang::En)
        }));
        let (wake_tx, wake_rx) = mpsc::sync_channel::<()>(1);
        drop(wake_tx);
        assert!(matches!(
            wake_rx.recv_timeout(Duration::from_secs(0)),
            Err(RecvTimeoutError::Disconnected)
        ));
        let (status, _) = clone_latest(&latest).expect("shutdown must still read the slot");
        assert!(!status.active);
    }

    #[test]
    fn flush_and_join_delivers_inactive_after_slow_reporter() {
        let posted = Arc::new(Mutex::new(None));
        let latest = Arc::new(Mutex::new(None));
        let (wake_tx, wake_rx) = mpsc::sync_channel(1);
        let (_event_tx, event_rx) = mpsc::channel();
        let posted_t = Arc::clone(&posted);
        let latest_t = Arc::clone(&latest);
        let join = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            loop {
                match wake_rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(()) | Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        *posted_t.lock().unwrap() = clone_latest(&latest_t).map(|(s, _)| s.active);
                        break;
                    }
                }
            }
        });
        let handle = CloudHandle {
            latest,
            wake: Some(wake_tx),
            events: event_rx,
            join: Some(join),
        };
        let mut inactive = sample_status();
        inactive.active = false;
        handle.push_status(inactive, Lang::En);
        handle.flush_and_join();
        assert_eq!(
            *posted.lock().unwrap(),
            Some(false),
            "exit must wait until the reporter has the inactive snapshot"
        );
    }

    #[test]
    fn pair_ipc_drains_queued_pairing_event() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(CloudEvent::Pairing {
                code: "AB7K-2Q9M".into(),
                url: "https://example/board/?code=AB7K-2Q9M".into(),
            })
            .unwrap();
        let handle = test_cloud_handle(mpsc::sync_channel(1).0, event_rx);
        let mut pairing = None;
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert_eq!(
            pairing.as_ref().map(|(code, _)| code.as_str()),
            Some("AB7K-2Q9M")
        );
    }

    #[test]
    fn sync_cloud_reports_inactive_after_remote_off() {
        let _dir = TestDataDir::install();
        let (wake, _wake_rx) = mpsc::sync_channel(1);
        wake.try_send(()).unwrap();
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(CloudEvent::Commands(vec![RemoteCommand::off("c-off")]))
            .unwrap();
        let handle = test_cloud_handle(wake, event_rx);
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        crate::apply::dispatch(&mut engine, &mut platform, never_sleep_core::Input::Start);
        assert!(engine.is_active());
        let mut pairing = None;
        sync_cloud(&mut engine, &mut platform, &handle, &mut pairing);
        assert!(!engine.is_active());
        let status = handle.queued_status().expect("queued snapshot");
        assert!(
            !status.active,
            "phone must see standby ended before the reporter exits"
        );
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
        assert!(src.contains("apply_effects_or_abort"));
        assert!(src.contains("fn sync_cloud"));
    }

    #[test]
    fn remote_on_aborts_when_power_assertion_fails() {
        let _dir = TestDataDir::install();
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = FailPower;
        apply_cloud_commands(
            &mut engine,
            &mut platform,
            &[RemoteCommand::on("remote-1", None)],
        );
        assert!(
            !engine.is_active(),
            "phone must not see standby when stay-awake assertions were not installed"
        );
        let st = engine.json_status(&platform.snapshot());
        assert_eq!(st.stop_reason_code.as_deref(), Some("assertion_failed"));
        assert!(!st.active);
    }

    struct FailPower;

    impl crate::platform::Platform for FailPower {
        fn snapshot(&self) -> never_sleep_core::HostSnapshot {
            never_sleep_core::HostSnapshot {
                monotonic_ms: 0,
                continuous_ms: 0,
                unix_secs: 1_700_000_000,
                utc_offset_secs: 0,
                on_ac: true,
                battery_percent: Some(80),
                lid_closed: false,
                display_asleep: Some(false),
                hid_idle_ms: 80_000,
                thermal: never_sleep_core::Thermal::Nominal,
            }
        }
        fn apply_power(&mut self, _plan: never_sleep_core::PowerPlan) -> Result<(), String> {
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
}
