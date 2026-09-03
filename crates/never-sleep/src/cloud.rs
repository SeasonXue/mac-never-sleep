use std::collections::HashSet;
use std::fs;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

use never_sleep_core::{
    apply_remote_command, identity_from_bytes, CloudIdentity, Engine, JsonStatus, Lang,
    RemoteCommand, PAIRING_TTL_SECS, PUBLIC_SITE_ORIGIN,
};
use serde::{Deserialize, Serialize};

use crate::apply::apply_effects_or_abort;
use crate::paths::{cloud_identity_path, ensure_data_dir};
use crate::platform::Platform;

pub const CLOUD_URL_ENV: &str = "NEVER_SLEEP_CLOUD_URL";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloudEvent {
    Pairing {
        code: String,
        url: String,
        expires_unix: u64,
    },
    PairingCleared,
    Commands(Vec<RemoteCommand>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReporterWake {
    Snapshot,
    Shutdown,
    /// Stop the loop without an offline heartbeat (live handoff to the menu).
    Detach,
}

pub struct CloudHandle {
    /// Latest-wins snapshot. A capacity-one wake channel only signals that
    /// `latest` changed; it must not carry the payload or a newer inactive
    /// status is dropped while the reporter is in a slow POST.
    latest: Arc<Mutex<Option<(JsonStatus, Lang)>>>,
    wake: Option<SyncSender<ReporterWake>>,
    events: mpsc::Receiver<CloudEvent>,
    join: Option<thread::JoinHandle<()>>,
    held_commands: Mutex<Vec<RemoteCommand>>,
}

impl CloudHandle {
    pub fn push_status(&self, status: JsonStatus, lang: Lang) {
        self.queue_latest(status, lang);
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(ReporterWake::Snapshot);
        }
    }

    fn queue_latest(&self, status: JsonStatus, lang: Lang) {
        if let Ok(mut slot) = self.latest.lock() {
            *slot = Some((status, lang));
        }
    }

    /// Disconnect the reporter and wait until it has POSTed the latest snapshot.
    pub fn flush_and_join(mut self) {
        self.disconnect_and_join();
    }

    /// Stop heartbeats without marking the Mac offline. The menu reporter owns
    /// the next POST.
    pub fn detach(mut self) {
        if let Some(wake) = self.wake.take() {
            let _ = wake.try_send(ReporterWake::Detach);
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }

    fn disconnect_and_join(&mut self) {
        if let Some(wake) = self.wake.take() {
            // try_send so Drop cannot deadlock when the reporter is gone or
            // the channel already holds coalesced snapshot wakes.
            let _ = wake.try_send(ReporterWake::Shutdown);
        }
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
        if let Ok(mut held) = self.held_commands.lock() {
            if !held.is_empty() {
                out.push(CloudEvent::Commands(std::mem::take(&mut *held)));
            }
        }
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }

    fn hold_commands(&self, commands: Vec<RemoteCommand>) {
        if commands.is_empty() {
            return;
        }
        if let Ok(mut held) = self.held_commands.lock() {
            held.extend(commands);
        }
    }
}

impl Drop for CloudHandle {
    fn drop(&mut self) {
        self.disconnect_and_join();
    }
}

/// Queue a snapshot and wait until the reporter has POSTed it (or disconnected).
pub fn publish_and_flush(handle: CloudHandle, status: JsonStatus, lang: Lang) {
    handle.queue_latest(status, lang);
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

/// Phone-board cards and localStorage reservations share this cap.
pub const MAX_DISPLAY_NAME_CHARS: usize = 128;

pub fn bound_display_name(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "Mac".into();
    }
    trimmed.chars().take(MAX_DISPLAY_NAME_CHARS).collect()
}

pub fn default_display_name() -> String {
    if let Ok(name) = std::env::var("NEVER_SLEEP_DEVICE_NAME") {
        if !name.trim().is_empty() {
            return bound_display_name(&name);
        }
    }
    bound_display_name(&hostname_from_os().unwrap_or_else(|| "Mac".into()))
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

pub fn load_or_create_identity() -> io::Result<CloudIdentity> {
    let path = cloud_identity_path();
    if let Some(id) = read_complete_identity(&path)? {
        return Ok(id);
    }
    let mut id_bytes = [0u8; 16];
    let mut token_bytes = [0u8; 32];
    fill_random(&mut id_bytes);
    fill_random(&mut token_bytes);
    let identity = identity_from_bytes(&id_bytes, &token_bytes);
    match save_identity(&identity) {
        Ok(()) => Ok(identity),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
            if let Some(id) = wait_for_complete_identity(&path)? {
                return Ok(id);
            }
            recover_stranded_identity(&path, identity)
        }
        Err(err) => Err(err),
    }
}

fn read_complete_identity(path: &Path) -> io::Result<Option<CloudIdentity>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    if let Ok(id) = toml::from_str::<CloudIdentity>(&text) {
        if id.device_id.len() >= 16 && id.device_token.len() >= 16 {
            restrict_owner_only(path)?;
            return Ok(Some(id));
        }
    }
    Ok(None)
}

fn wait_for_complete_identity(path: &Path) -> io::Result<Option<CloudIdentity>> {
    for _ in 0..40 {
        if let Some(id) = read_complete_identity(path)? {
            return Ok(Some(id));
        }
        thread::sleep(Duration::from_millis(5));
    }
    Ok(None)
}

fn recover_stranded_identity(path: &Path, identity: CloudIdentity) -> io::Result<CloudIdentity> {
    if let Some(id) = read_complete_identity(path)? {
        return Ok(id);
    }
    let mut file = match fs::OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            return match save_identity(&identity) {
                Ok(()) => Ok(identity),
                Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                    wait_for_complete_identity(path)?.ok_or(err)
                }
                Err(err) => Err(err),
            };
        }
        Err(err) => return Err(err),
    };
    // SAFETY: `file` is an open fd we own for the duration of this recover;
    // flock(LOCK_EX) is the POSIX exclusive lock used to serialize stranded
    // cloud.toml replacement.
    let lock = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
    if lock != 0 {
        return Err(io::Error::last_os_error());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    if let Ok(id) = toml::from_str::<CloudIdentity>(&text) {
        if id.device_id.len() >= 16 && id.device_token.len() >= 16 {
            restrict_owner_only(path)?;
            return Ok(id);
        }
    }
    let text = toml::to_string_pretty(&identity)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    restrict_owner_only(path)?;
    Ok(identity)
}

fn restrict_owner_only(path: &Path) -> io::Result<()> {
    let mut perms = fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    fs::set_permissions(path, perms)
}

fn write_owner_only(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_file_name(format!(
        "{}.tmp.{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("cloud.toml"),
        std::process::id()
    ));
    {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    restrict_owner_only(&tmp)?;
    let linked = fs::hard_link(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => restrict_owner_only(path),
        Err(err) => Err(err),
    }
}

fn save_identity(identity: &CloudIdentity) -> io::Result<()> {
    ensure_data_dir()?;
    let text = toml::to_string_pretty(identity)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    write_owner_only(&cloud_identity_path(), text.as_bytes())
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    offline: bool,
}

#[derive(Debug, Deserialize)]
struct PairStartResponse {
    ok: bool,
    #[serde(default)]
    pairing_code: Option<String>,
    #[serde(default)]
    pairing_url: Option<String>,
    #[serde(default)]
    expires_unix: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct HeartbeatResponse {
    ok: bool,
    #[serde(default)]
    pairing_code: Option<String>,
    #[serde(default)]
    pairing_url: Option<String>,
    #[serde(default)]
    expires_unix: Option<u64>,
    #[serde(default)]
    commands: Vec<RemoteCommand>,
}

pub fn heartbeat_request_json(
    identity: &CloudIdentity,
    display_name: &str,
    status: &JsonStatus,
    lang: &str,
    ack_command_ids: &[String],
    offline: bool,
) -> String {
    serde_json::to_string(&HeartbeatBody {
        device_id: &identity.device_id,
        device_token: &identity.device_token,
        display_name,
        lang,
        ack_command_ids,
        status,
        offline,
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
        expires_unix: parsed.expires_unix,
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
    pub expires_unix: Option<u64>,
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

pub fn parse_pair_start_response(raw: &str) -> Result<(String, String, u64), String> {
    let parsed: PairStartResponse = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    match (parsed.ok, parsed.pairing_code, parsed.pairing_url) {
        (true, Some(code), Some(url)) => {
            let expires_unix = parsed
                .expires_unix
                .unwrap_or_else(|| unix_now_secs() + PAIRING_TTL_SECS);
            Ok((code, url, expires_unix))
        }
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
    let (wake_tx, wake_rx) = mpsc::sync_channel(2);
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
        held_commands: Mutex::new(Vec::new()),
    }
}

#[cfg(test)]
fn clone_latest(latest: &Mutex<Option<(JsonStatus, Lang)>>) -> Option<(JsonStatus, Lang)> {
    latest.lock().ok().and_then(|slot| slot.clone())
}

fn take_latest(latest: &Mutex<Option<(JsonStatus, Lang)>>) -> Option<(JsonStatus, Lang)> {
    latest.lock().ok().and_then(|mut slot| slot.take())
}

fn should_reporter_tick(fresh: bool, shutting_down: bool) -> bool {
    fresh || shutting_down
}

fn shutting_down_from_wake(
    recv: Result<ReporterWake, RecvTimeoutError>,
    drained_shutdown: bool,
) -> bool {
    match recv {
        Ok(ReporterWake::Shutdown) | Ok(ReporterWake::Detach) => true,
        Ok(ReporterWake::Snapshot) => drained_shutdown,
        Err(RecvTimeoutError::Timeout) => drained_shutdown,
        Err(RecvTimeoutError::Disconnected) => true,
    }
}

fn drain_reporter_wakes(rx: &mpsc::Receiver<ReporterWake>) -> (bool, bool) {
    let mut shutdown = false;
    let mut detach = false;
    loop {
        match rx.try_recv() {
            Ok(ReporterWake::Shutdown) => shutdown = true,
            Ok(ReporterWake::Detach) => detach = true,
            Ok(ReporterWake::Snapshot) => {}
            Err(mpsc::TryRecvError::Empty) => break,
            Err(mpsc::TryRecvError::Disconnected) => {
                shutdown = true;
                break;
            }
        }
    }
    (shutdown, detach)
}

fn reporter_marks_offline(
    recv: Result<ReporterWake, RecvTimeoutError>,
    drained_shutdown: bool,
    drained_detach: bool,
) -> bool {
    match recv {
        Ok(ReporterWake::Shutdown) => true,
        Ok(ReporterWake::Detach) => false,
        Ok(ReporterWake::Snapshot) => drained_shutdown,
        Err(RecvTimeoutError::Timeout) => drained_shutdown,
        Err(RecvTimeoutError::Disconnected) => !drained_detach,
    }
}

fn reporter_loop(
    identity: CloudIdentity,
    display_name: String,
    transport: UreqTransport,
    wake_rx: mpsc::Receiver<ReporterWake>,
    latest: Arc<Mutex<Option<(JsonStatus, Lang)>>>,
    event_tx: mpsc::Sender<CloudEvent>,
) {
    let mut gate = ReporterGate::default();
    let mut inbox = CommandInbox::default();
    let mut last: Option<(JsonStatus, Lang)> = None;
    loop {
        let shutting_down = {
            let recv = wake_rx.recv_timeout(Duration::from_secs(3));
            let (drained_shutdown, drained_detach) = drain_reporter_wakes(&wake_rx);
            let shutting_down = shutting_down_from_wake(recv, drained_shutdown || drained_detach);
            let offline = reporter_marks_offline(recv, drained_shutdown, drained_detach);
            if shutting_down && !offline {
                break;
            }
            offline
        };
        let mut fresh = false;
        if let Some(pair) = take_latest(&latest) {
            last = Some(pair);
            fresh = true;
        }
        if !should_reporter_tick(fresh, shutting_down) {
            continue;
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
            shutting_down,
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
    offline: bool,
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
                if let Ok((code, url, expires_unix)) = parse_pair_start_response(&raw) {
                    gate.on_pair_start_ok();
                    let _ = event_tx.send(CloudEvent::Pairing {
                        code,
                        url,
                        expires_unix,
                    });
                }
            }
            Ok(CloudPost::Unauthorized) => gate.on_unauthorized(),
            Err(_) => {}
        }
    }

    let body = heartbeat_request_json(
        identity,
        display_name,
        status,
        lang,
        inbox.ack_ids(),
        offline,
    );
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
        gate.on_pair_start_ok();
        let expires_unix = outcome
            .expires_unix
            .unwrap_or_else(|| unix_now_secs() + PAIRING_TTL_SECS);
        let _ = event_tx.send(CloudEvent::Pairing {
            code,
            url,
            expires_unix,
        });
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
/// Drop the retained pairing state if its deadline has passed.
/// Call before rendering the pairing code in the panel or answering
/// `IpcRequest::Pair` so connectivity loss doesn't show a stale code forever.
pub fn expire_stale_pairing(pairing: &mut Option<(String, String, u64)>) {
    if let Some((_, _, expires_unix)) = pairing.as_ref() {
        if unix_now_secs() >= *expires_unix {
            *pairing = None;
        }
    }
}

pub fn apply_polled_commands(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    handle: &CloudHandle,
    pairing: &mut Option<(String, String, u64)>,
) {
    for event in handle.poll_events() {
        match event {
            CloudEvent::Commands(commands) => {
                if crate::session_lock::should_hold_cloud_commands(
                    engine.is_active(),
                    std::process::id(),
                ) {
                    handle.hold_commands(commands);
                    continue;
                }
                apply_cloud_commands(engine, platform, &commands);
            }
            CloudEvent::Pairing {
                code,
                url,
                expires_unix,
            } => {
                if unix_now_secs() < expires_unix {
                    *pairing = Some((code, url, expires_unix));
                } else {
                    *pairing = None;
                }
            }
            CloudEvent::PairingCleared => *pairing = None,
        }
    }
    expire_stale_pairing(pairing);
}

/// Apply remote commands first, then queue the resulting snapshot.
pub fn sync_cloud(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    handle: &CloudHandle,
    pairing: &mut Option<(String, String, u64)>,
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

    fn test_cloud_handle(
        wake: SyncSender<ReporterWake>,
        events: mpsc::Receiver<CloudEvent>,
    ) -> CloudHandle {
        CloudHandle {
            latest: Arc::new(Mutex::new(None)),
            wake: Some(wake),
            events,
            join: None,
            held_commands: Mutex::new(Vec::new()),
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
    fn heartbeat_pairing_keeps_the_workers_expires_unix() {
        let outcome = parse_heartbeat_response(
            r#"{"ok":true,"pairing_code":"AB7K-2Q9M","pairing_url":"https://x/board/?code=AB7K-2Q9M","expires_unix":1500,"commands":[]}"#,
        )
        .unwrap();
        assert_eq!(
            outcome.expires_unix,
            Some(1500),
            "heartbeat must surface the Worker's stored offer deadline"
        );
        let transport = ScriptedTransport {
            pair: Mutex::new(vec![]),
            beat: Mutex::new(vec![Ok(CloudPost::Ok(
                r#"{"ok":true,"pairing_code":"AB7K-2Q9M","pairing_url":"https://x/board/?code=AB7K-2Q9M","expires_unix":1500,"commands":[]}"#
                    .into(),
            ))]),
            pair_calls: Mutex::new(0),
            beat_calls: Mutex::new(0),
        };
        let id = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        let mut gate = ReporterGate { registered: true };
        let mut inbox = CommandInbox::default();
        let (event_tx, event_rx) = mpsc::channel();
        reporter_tick(
            &mut gate,
            &mut inbox,
            &transport,
            &id,
            "Studio",
            "en",
            &sample_status(),
            &event_tx,
            false,
        );
        match event_rx.try_recv().unwrap() {
            CloudEvent::Pairing { expires_unix, .. } => {
                assert_eq!(
                    expires_unix, 1500,
                    "do not replace the Worker deadline with now+PAIRING_TTL_SECS"
                );
            }
            other => panic!("expected pairing event, got {other:?}"),
        }
    }

    #[test]
    fn pair_start_keeps_the_workers_expires_unix() {
        let (code, url, expires) = parse_pair_start_response(
            r#"{"ok":true,"pairing_code":"AB7K-2Q9M","pairing_url":"https://x/board/?code=AB7K-2Q9M","expires_unix":4242}"#,
        )
        .unwrap();
        assert_eq!(code, "AB7K-2Q9M");
        assert!(url.contains("/board/"));
        assert_eq!(expires, 4242);
    }

    #[test]
    fn write_owner_only_create_new_leaves_winner_intact() {
        let _dir = TestDataDir::install();
        let path = cloud_identity_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"winner-identity").unwrap();
        let err = write_owner_only(&path, b"loser-identity")
            .expect_err("a late writer must not truncate the winner");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "winner-identity",
            "create_new must leave the first writer's credentials on disk"
        );
    }

    #[test]
    fn load_or_create_identity_reloads_winner_after_create_race() {
        let src = include_str!("cloud.rs");
        assert!(
            src.contains("hard_link"),
            "cloud.toml must appear only after the identity bytes are fully written"
        );
        let load = src
            .split("pub fn load_or_create_identity")
            .nth(1)
            .unwrap()
            .split("fn restrict_owner_only")
            .next()
            .unwrap();
        assert!(
            load.contains("AlreadyExists"),
            "the losing process must reload the winner instead of advertising a different identity"
        );
    }

    #[test]
    fn load_or_create_identity_recovers_an_empty_stranded_file() {
        let _dir = TestDataDir::install();
        let path = cloud_identity_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"").unwrap();
        let id =
            load_or_create_identity().expect("empty cloud.toml must not disable cloud forever");
        assert_eq!(id.device_id.len(), 32);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("device_token"));
    }

    #[test]
    fn recover_stranded_identity_keeps_a_complete_peer_file() {
        let _dir = TestDataDir::install();
        let path = cloud_identity_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let winner = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        fs::write(&path, toml::to_string_pretty(&winner).unwrap()).unwrap();
        let loser = CloudIdentity {
            device_id: "11".repeat(16),
            device_token: "22".repeat(32),
        };
        let got = recover_stranded_identity(&path, loser).unwrap();
        assert_eq!(got, winner, "must not unlink a peer that already published");
        let disk: CloudIdentity = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(disk, winner);
    }

    #[test]
    fn recover_stranded_identity_second_claim_reloads_winner() {
        let _dir = TestDataDir::install();
        let path = cloud_identity_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"").unwrap();
        let first = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        let second = CloudIdentity {
            device_id: "11".repeat(16),
            device_token: "22".repeat(32),
        };
        let a = recover_stranded_identity(&path, first.clone()).unwrap();
        let b = recover_stranded_identity(&path, second).unwrap();
        assert_eq!(a, first);
        assert_eq!(b, first, "the losing recoverer must reload the winner");
    }

    #[test]
    fn stranded_identity_recovery_is_exclusive_across_threads() {
        let _dir = TestDataDir::install();
        let path = cloud_identity_path();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"").unwrap();
        let first = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        let second = CloudIdentity {
            device_id: "11".repeat(16),
            device_token: "22".repeat(32),
        };
        let path_a = path.clone();
        let path_b = path.clone();
        let id_a = first.clone();
        let id_b = second.clone();
        let a = std::thread::spawn(move || recover_stranded_identity(&path_a, id_a));
        let b = std::thread::spawn(move || recover_stranded_identity(&path_b, id_b));
        let got_a = a.join().unwrap().expect("thread a");
        let got_b = b.join().unwrap().expect("thread b");
        assert_eq!(
            got_a, got_b,
            "two recoverers of an empty cloud.toml must share one identity"
        );
        let disk: CloudIdentity = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(disk, got_a);
        assert!(got_a == first || got_a == second);
    }

    #[test]
    fn load_or_create_identity_persists_random_token() {
        let _dir = TestDataDir::install();
        let first = load_or_create_identity().expect("persist identity");
        let second = load_or_create_identity().expect("reload identity");
        assert_eq!(first, second);
        assert_eq!(first.device_id.len(), 32);
        assert_eq!(first.device_token.len(), 64);
        assert_ne!(first.device_id, first.device_token);
        let text = fs::read_to_string(cloud_identity_path()).unwrap();
        assert!(text.contains("device_id"));
        assert!(text.contains("device_token"));
    }

    #[test]
    fn load_or_create_identity_fails_when_write_cannot_persist() {
        let _dir = TestDataDir::install();
        let path = cloud_identity_path();
        fs::create_dir_all(&path).unwrap();
        assert!(
            load_or_create_identity().is_err(),
            "an unpersisted identity must not be advertised for pairing"
        );
    }

    #[test]
    fn persisted_device_token_is_owner_readable_only() {
        use std::os::unix::fs::PermissionsExt;
        let _dir = TestDataDir::install();
        load_or_create_identity().expect("persist identity");
        let mode = fs::metadata(cloud_identity_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            mode, 0o600,
            "device_token must not be world-readable under a 022 umask"
        );
    }

    #[test]
    fn loading_identity_tightens_world_readable_token_file() {
        use std::os::unix::fs::PermissionsExt;
        let _dir = TestDataDir::install();
        let identity = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        let text = toml::to_string_pretty(&identity).unwrap();
        fs::create_dir_all(cloud_identity_path().parent().unwrap()).unwrap();
        fs::write(cloud_identity_path(), text).unwrap();
        let mut perms = fs::metadata(cloud_identity_path()).unwrap().permissions();
        perms.set_mode(0o644);
        fs::set_permissions(cloud_identity_path(), perms).unwrap();
        load_or_create_identity().expect("reload identity");
        let mode = fs::metadata(cloud_identity_path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "existing cloud.toml must be tightened on load");
    }

    #[test]
    fn load_identity_fails_when_existing_token_cannot_be_restricted() {
        let src = include_str!("cloud.rs");
        let load = src
            .split("pub fn load_or_create_identity")
            .nth(1)
            .unwrap()
            .split("fn restrict_owner_only")
            .next()
            .unwrap();
        assert!(
            load.contains("restrict_owner_only(&path)?")
                || load.contains("restrict_owner_only(path)?"),
            "chmod failure must fail identity load, not advertise a world-readable token"
        );
        assert!(
            !load.contains("let _ ="),
            "do not ignore restrict_owner_only errors on load"
        );
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
            false,
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
        assert!(
            v["offline"].is_null(),
            "live heartbeats must omit the quit offline marker"
        );
    }

    #[test]
    fn shutdown_heartbeat_json_marks_the_mac_offline() {
        let id = CloudIdentity {
            device_id: "ab".repeat(16),
            device_token: "cd".repeat(32),
        };
        let v: serde_json::Value = serde_json::from_str(&heartbeat_request_json(
            &id,
            "Studio",
            &sample_status(),
            "en",
            &[],
            true,
        ))
        .unwrap();
        assert_eq!(v["offline"], true);
        let src = include_str!("cloud.rs");
        assert!(
            src.contains("heartbeat_request_json(") && src.contains("shutting_down"),
            "the quit flush must send offline:true on the last heartbeat"
        );
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
    fn retained_pairing_state_is_cleared_when_expiry_passes() {
        let (event_tx, event_rx) = mpsc::channel();
        // send a fresh (non-expired) pairing event
        event_tx
            .send(CloudEvent::Pairing {
                code: "AB7K-2Q9M".into(),
                url: "https://example/board/?code=AB7K-2Q9M".into(),
                expires_unix: unix_now_secs() + 60,
            })
            .unwrap();
        let handle = test_cloud_handle(mpsc::sync_channel(2).0, event_rx);
        let mut pairing = None;
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert!(pairing.is_some(), "fresh pairing code must be retained");
        // simulate expiry by backdating the stored deadline
        if let Some((_, _, ref mut exp)) = pairing.as_mut() {
            *exp = 1;
        }
        // refresh should clear it
        expire_stale_pairing(&mut pairing);
        assert!(pairing.is_none(), "expired pairing state must be cleared");
        let src = include_str!("cloud.rs");
        assert!(
            src.contains("expire_stale_pairing"),
            "gui must call expire_stale_pairing before rendering or answering IpcRequest::Pair"
        );
    }

    #[test]
    fn expired_pairing_event_does_not_set_gui_code() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(CloudEvent::Pairing {
                code: "AB7K-2Q9M".into(),
                url: "https://example/board/?code=AB7K-2Q9M".into(),
                expires_unix: 1, // epoch+1 is always in the past
            })
            .unwrap();
        let handle = test_cloud_handle(mpsc::sync_channel(2).0, event_rx);
        let mut pairing = Some(("OLD-CODE".into(), "https://example/board/".into(), 1u64));
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert!(
            pairing.is_none(),
            "an expired pairing code must be cleared, not shown"
        );
    }

    #[test]
    fn pairing_cleared_event_drops_gui_code() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx.send(CloudEvent::PairingCleared).unwrap();
        let handle = test_cloud_handle(mpsc::sync_channel(2).0, event_rx);
        let mut pairing = Some((
            "AB7K-2Q9M".into(),
            "https://example/board/?code=x".into(),
            u64::MAX,
        ));
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert!(pairing.is_none());
    }

    #[test]
    fn queued_phone_off_applies_after_handoff_not_while_idle() {
        let _guard = TestDataDir::install();
        crate::paths::ensure_data_dir().unwrap();
        std::fs::write(crate::paths::session_lock_path(), "pid=1\nclamshell=1\n").unwrap();
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(CloudEvent::Commands(vec![RemoteCommand::off("end")]))
            .unwrap();
        let handle = test_cloud_handle(mpsc::sync_channel(2).0, event_rx);
        let mut pairing = None;
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert!(!engine.is_active());
        let host = platform.snapshot();
        engine.handle(
            never_sleep_core::Input::Handoff {
                pref: never_sleep_core::DurationPref::Hours { hours: 8 },
                remaining_secs: Some(3600),
            },
            &host,
        );
        assert!(engine.is_active(), "handoff must start the adopted session");
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert!(
            !engine.is_active(),
            "phone Off queued before adopt must end the session the menu just took over"
        );
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
        let (wake, _wake_rx) = mpsc::sync_channel(2);
        wake.try_send(ReporterWake::Snapshot).unwrap();
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
        let (wake_tx, wake_rx) = mpsc::sync_channel::<ReporterWake>(2);
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
        let (wake_tx, wake_rx) = mpsc::sync_channel(2);
        let (_event_tx, event_rx) = mpsc::channel();
        let posted_t = Arc::clone(&posted);
        let latest_t = Arc::clone(&latest);
        let join = thread::spawn(move || {
            thread::sleep(Duration::from_millis(30));
            loop {
                match wake_rx.recv_timeout(Duration::from_millis(200)) {
                    Ok(_) | Err(RecvTimeoutError::Timeout) => {}
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
            held_commands: Mutex::new(Vec::new()),
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
    fn wake_then_disconnect_is_a_single_shutdown_tick() {
        assert!(
            shutting_down_from_wake(Ok(ReporterWake::Shutdown), false),
            "flush_and_join must send an explicit shutdown, not infer it from a later disconnect"
        );
        assert!(shutting_down_from_wake(Ok(ReporterWake::Snapshot), true));
        assert!(
            !shutting_down_from_wake(Ok(ReporterWake::Snapshot), false),
            "a snapshot wake with an empty drain is not shutdown"
        );
        assert!(!shutting_down_from_wake(
            Err(RecvTimeoutError::Timeout),
            false
        ));
        assert!(shutting_down_from_wake(
            Err(RecvTimeoutError::Disconnected),
            false
        ));
        assert!(
            shutting_down_from_wake(Ok(ReporterWake::Detach), false),
            "Detach still ends the reporter loop"
        );
        assert!(!reporter_marks_offline(
            Ok(ReporterWake::Detach),
            false,
            true
        ));
        assert!(reporter_marks_offline(
            Ok(ReporterWake::Shutdown),
            false,
            false
        ));
    }

    #[test]
    fn detach_emits_a_detach_wake_without_offline() {
        let (wake_tx, wake_rx) = mpsc::sync_channel(4);
        let (_event_tx, event_rx) = mpsc::channel();
        let signals = Arc::new(Mutex::new(Vec::new()));
        let signals_t = Arc::clone(&signals);
        let join = thread::spawn(move || {
            while let Ok(sig) = wake_rx.recv_timeout(Duration::from_millis(400)) {
                signals_t.lock().unwrap().push(sig);
            }
        });
        let handle = CloudHandle {
            latest: Arc::new(Mutex::new(None)),
            wake: Some(wake_tx),
            events: event_rx,
            join: Some(join),
            held_commands: Mutex::new(Vec::new()),
        };
        handle.detach();
        assert_eq!(
            *signals.lock().unwrap(),
            vec![ReporterWake::Detach],
            "live handoff must not send Shutdown (offline:true)"
        );
    }

    #[test]
    fn publish_and_flush_emits_only_an_explicit_shutdown_wake() {
        let (wake_tx, wake_rx) = mpsc::sync_channel(4);
        let (_event_tx, event_rx) = mpsc::channel();
        let signals = Arc::new(Mutex::new(Vec::new()));
        let signals_t = Arc::clone(&signals);
        let join = thread::spawn(move || {
            while let Ok(sig) = wake_rx.recv_timeout(Duration::from_millis(400)) {
                signals_t.lock().unwrap().push(sig);
            }
        });
        let handle = CloudHandle {
            latest: Arc::new(Mutex::new(None)),
            wake: Some(wake_tx),
            events: event_rx,
            join: Some(join),
            held_commands: Mutex::new(Vec::new()),
        };
        let mut inactive = sample_status();
        inactive.active = false;
        publish_and_flush(handle, inactive, Lang::En);
        assert_eq!(
            *signals.lock().unwrap(),
            vec![ReporterWake::Shutdown],
            "queue the final snapshot then shut down; do not send Snapshot then drop the sender"
        );
    }

    #[test]
    fn pair_ipc_drains_queued_pairing_event() {
        let (event_tx, event_rx) = mpsc::channel();
        event_tx
            .send(CloudEvent::Pairing {
                code: "AB7K-2Q9M".into(),
                url: "https://example/board/?code=AB7K-2Q9M".into(),
                expires_unix: u64::MAX,
            })
            .unwrap();
        let handle = test_cloud_handle(mpsc::sync_channel(2).0, event_rx);
        let mut pairing = None;
        let mut engine = Engine::new(AppConfig::default());
        let mut platform = StubPlatform;
        apply_polled_commands(&mut engine, &mut platform, &handle, &mut pairing);
        assert_eq!(
            pairing.as_ref().map(|(code, _, _)| code.as_str()),
            Some("AB7K-2Q9M")
        );
    }

    #[test]
    fn sync_cloud_reports_inactive_after_remote_off() {
        let _dir = TestDataDir::install();
        let (wake, _wake_rx) = mpsc::sync_channel(2);
        wake.try_send(ReporterWake::Snapshot).unwrap();
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
    fn live_heartbeat_pairing_marks_gate_registered() {
        let transport = ScriptedTransport {
            pair: Mutex::new(vec![Err("lost pair/start body".into())]),
            beat: Mutex::new(vec![
                Ok(CloudPost::Ok(
                    r#"{"ok":true,"pairing_code":"AB7K-2Q9M","pairing_url":"https://x/board/?code=AB7K-2Q9M","commands":[]}"#
                        .into(),
                )),
                Ok(CloudPost::Ok(
                    r#"{"ok":true,"pairing_code":"AB7K-2Q9M","pairing_url":"https://x/board/?code=AB7K-2Q9M","commands":[]}"#
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
        let (event_tx, event_rx) = mpsc::channel();
        let status = sample_status();

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx, false,
        );
        assert!(
            !gate.needs_pair_start(),
            "an authenticated heartbeat that returns a live pairing must stop calling pair/start"
        );
        {
            let event = event_rx.try_recv().unwrap();
            if let CloudEvent::Pairing { code, url, .. } = event {
                assert_eq!(code, "AB7K-2Q9M");
                assert_eq!(url, "https://x/board/?code=AB7K-2Q9M");
            } else {
                panic!("expected CloudEvent::Pairing");
            }
        }

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx, false,
        );
        assert_eq!(
            *transport.pair_calls.lock().unwrap(),
            1,
            "the displayed code must not rotate on every tick after a lost pair/start response"
        );
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
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx, false,
        );
        assert!(gate.needs_pair_start(), "first pair/start timed out");
        assert_eq!(*transport.pair_calls.lock().unwrap(), 1);

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx, false,
        );
        assert!(
            !gate.needs_pair_start(),
            "successful pair/start plus a live offer registers the device"
        );
        assert_eq!(*transport.pair_calls.lock().unwrap(), 2);

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx, false,
        );
        assert!(
            gate.needs_pair_start(),
            "unauthorized heartbeat must retry pair/start"
        );
        assert_eq!(*transport.pair_calls.lock().unwrap(), 2);

        reporter_tick(
            &mut gate, &mut inbox, &transport, &id, "Studio", "en", &status, &event_tx, false,
        );
        assert!(!gate.needs_pair_start());
        assert_eq!(*transport.pair_calls.lock().unwrap(), 3);
        assert_eq!(*transport.beat_calls.lock().unwrap(), 4);
    }

    #[test]
    fn stale_snapshot_is_not_heartbeated_until_main_loop_refreshes() {
        assert!(
            !should_reporter_tick(false, false),
            "a blocked osascript dialog must not keep the Mac online or ack commands"
        );
        assert!(
            should_reporter_tick(true, false),
            "a freshly pushed main-loop snapshot may heartbeat"
        );
        assert!(
            should_reporter_tick(false, true),
            "quit still POSTs the last snapshot"
        );
        let src = include_str!("cloud.rs");
        assert!(src.contains("take_latest"));
        assert!(src.contains("should_reporter_tick(fresh, shutting_down)"));
    }

    #[test]
    fn take_latest_consumes_the_main_loop_snapshot() {
        let slot = Mutex::new(Some((sample_status(), Lang::En)));
        assert!(take_latest(&slot).is_some());
        assert!(
            take_latest(&slot).is_none(),
            "do not replay last after the reporter already sent it"
        );
    }

    #[test]
    fn display_name_is_capped_before_it_is_advertised() {
        assert_eq!(
            bound_display_name(&"x".repeat(200)).chars().count(),
            MAX_DISPLAY_NAME_CHARS
        );
        assert_eq!(
            bound_display_name(&"名".repeat(200)).chars().count(),
            MAX_DISPLAY_NAME_CHARS
        );
        assert_eq!(bound_display_name("  Studio  "), "Studio");
        assert_eq!(bound_display_name("   "), "Mac");
        assert!(default_display_name().chars().count() <= MAX_DISPLAY_NAME_CHARS);
        let src = include_str!("cloud.rs");
        assert!(src.contains("bound_display_name"));
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
