use std::cell::RefCell;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::fd::AsRawFd;
use std::path::PathBuf;

/// Whether this process should restore clamshell sleep and delete `session.lock`.
///
/// A live peer (the menu, after a foreground handoff) owns the file. Releasing
/// the previous process must not globally re-enable clamshell sleep or remove
/// the lock the next launch uses to restore the flag.
#[cfg(any(test, target_os = "macos"))]
pub fn should_release_clamshell_lock(
    our_pid: u32,
    lock_pid: Option<u32>,
    lock_holder_alive: bool,
) -> bool {
    match lock_pid {
        None => true,
        Some(pid) if pid == our_pid => true,
        Some(_) => !lock_holder_alive,
    }
}

/// Whether ApplyPower may replace `session.lock` with this process's PID.
///
/// `already_holding` is this process's `owns_power` *before* the current
/// ApplyPower. The adopting menu is not yet holding, so it may take a live
/// donor's lock. A timed-out donor is already holding, so it must not steal
/// the successor's lock back on Tick.
#[cfg(any(test, target_os = "macos"))]
pub fn should_claim_session_lock(
    our_pid: u32,
    lock_pid: Option<u32>,
    lock_holder_alive: bool,
    already_holding: bool,
) -> bool {
    match lock_pid {
        None => true,
        Some(pid) if pid == our_pid => true,
        Some(_) if !lock_holder_alive => true,
        Some(_) => !already_holding,
    }
}

/// Whether this process should call `set_clamshell_sleep_disabled(false)`.
///
/// A live peer that recorded `clamshell=1` owns the global flag. A live peer
/// that recorded `clamshell=0` did not take ownership, so the previous process
/// must still restore clamshell sleep even while leaving the lock file in place.
#[cfg(any(test, target_os = "macos"))]
pub fn should_restore_clamshell(
    our_pid: u32,
    lock: Option<(u32, bool)>,
    lock_holder_alive: bool,
) -> bool {
    match lock {
        None => true,
        Some((pid, _)) if pid == our_pid => true,
        Some((_, claimed)) if lock_holder_alive => !claimed,
        Some(_) => true,
    }
}

/// Whether the process applying this plan should restore clamshell sleep.
///
/// A successor that does not claim the flag must clear an inherited disable
/// before acknowledging handoff. If the donor dies during `detach()`, the
/// menu never recorded `clamshell_on` and would otherwise leave the flag stuck.
#[cfg(any(test, target_os = "macos"))]
pub fn should_clear_unclaimed_clamshell(claiming: bool) -> bool {
    !claiming
}

/// Whether a failed `set_clamshell_sleep_disabled(false)` must abort adopt.
///
/// Ordinary lid-open / display-off starts also call restore because they are
/// not claiming the flag. The private selector is often unavailable there; that
/// must not tear down a successful idle assertion. Abort when a foreign lock
/// recorded `clamshell=1`, including after the donor has already died — otherwise
/// this process writes `clamshell=0` and never retries the failed restore.
#[cfg(any(test, target_os = "macos"))]
pub fn should_fail_unclaimed_clamshell_restore(
    claiming: bool,
    lock: Option<(u32, bool)>,
    _lock_holder_alive: bool,
    our_pid: u32,
) -> bool {
    if claiming {
        return false;
    }
    matches!(lock, Some((pid, true)) if pid != our_pid)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionLockRecord {
    pub pid: u32,
    pub clamshell: bool,
    pub starttime: Option<u64>,
}

#[cfg(any(test, target_os = "macos"))]
pub fn parse_lock_text(s: &str) -> (u32, bool) {
    let rec = parse_lock_record(s);
    (rec.pid, rec.clamshell)
}

pub fn parse_lock_record(s: &str) -> SessionLockRecord {
    let mut pid = 0u32;
    let mut clamshell = false;
    let mut starttime = None;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("pid=") {
            pid = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = line.strip_prefix("clamshell=") {
            clamshell = v.trim() == "1";
        }
        if let Some(v) = line.strip_prefix("starttime=") {
            starttime = v.trim().parse().ok();
        }
    }
    SessionLockRecord {
        pid,
        clamshell,
        starttime,
    }
}

pub fn format_lock_text(pid: u32, clamshell: bool, starttime: Option<u64>) -> String {
    let mut body = format!("pid={pid}\nclamshell={}\n", u8::from(clamshell));
    if let Some(start) = starttime {
        body.push_str(&format!("starttime={start}\n"));
    }
    body
}

pub fn read_lock_record() -> Option<SessionLockRecord> {
    let mut s = String::new();
    std::fs::File::open(crate::paths::session_lock_path())
        .ok()?
        .read_to_string(&mut s)
        .ok()?;
    Some(parse_lock_record(&s))
}

pub fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// A live pid is not enough: SIGKILL can leave `session.lock` for a reused pid.
pub fn lock_holder_is_live(
    pid_alive: bool,
    recorded_start: Option<u64>,
    observed_start: Option<u64>,
) -> bool {
    if !pid_alive {
        return false;
    }
    match recorded_start {
        None => true,
        Some(want) => observed_start == Some(want),
    }
}

pub fn parse_proc_stat_starttime(stat: &str) -> Option<u64> {
    let rest = stat.rsplit_once(')')?.1;
    rest.split_whitespace().nth(19)?.parse().ok()
}

pub fn process_starttime(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        parse_proc_stat_starttime(&raw)
    }
    #[cfg(target_os = "macos")]
    {
        macos_proc_starttime(pid)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_proc_starttime(pid: u32) -> Option<u64> {
    unsafe {
        let mut mib = [
            libc::CTL_KERN,
            libc::KERN_PROC,
            libc::KERN_PROC_PID,
            pid as libc::c_int,
        ];
        let mut info: libc::kinfo_proc = std::mem::zeroed();
        let mut size = std::mem::size_of::<libc::kinfo_proc>();
        let rc = libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            &mut info as *mut _ as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        );
        if rc != 0 || size == 0 {
            return None;
        }
        Some(info.kp_proc.p_starttime.tv_sec as u64)
    }
}

/// Hold phone On/Off while another live process still owns `session.lock`.
/// Applying Off against an idle menu is a no-op and would drop the command.
pub fn should_hold_cloud_commands(engine_active: bool, our_pid: u32) -> bool {
    if engine_active {
        return false;
    }
    match read_lock_record() {
        Some(rec) if rec.pid != our_pid => lock_holder_is_live(
            pid_is_alive(rec.pid),
            rec.starttime,
            process_starttime(rec.pid),
        ),
        _ => false,
    }
}

/// Do not start a local session while a live donor still owns standby.
/// ⌥⌘P / Toggle during that overlap would otherwise race the handoff.
#[cfg(any(test, target_os = "macos"))]
pub fn should_defer_local_controls(engine_active: bool, our_pid: u32) -> bool {
    should_hold_cloud_commands(engine_active, our_pid)
}

/// Remember ⌥⌘P / Toggle while a live donor still owns standby.
#[cfg(any(test, target_os = "macos"))]
pub fn note_deferred_escape(deferred: bool, pending_stop: &mut bool) {
    if deferred {
        *pending_stop = true;
    }
}

/// After a successful adopt or a stop_donor reply, apply the remembered escape.
#[cfg(any(test, target_os = "macos"))]
pub fn take_pending_stop_on_adopt(adopted: bool, pending_stop: &mut bool) -> bool {
    take_pending_stop_after_handoff(adopted, false, pending_stop)
}

/// Consume the escape after the held-command drain when adopt succeeded or the
/// donor was told to stop (remaining_secs already zero, etc.).
#[cfg(any(test, target_os = "macos"))]
pub fn take_pending_stop_after_handoff(
    adopted: bool,
    stop_donor: bool,
    pending_stop: &mut bool,
) -> bool {
    let apply = *pending_stop && (adopted || stop_donor);
    if apply {
        *pending_stop = false;
    }
    apply
}

/// `never-sleep off` while idle behind a live donor is the same escape as ⌥⌘P.
#[cfg(any(test, target_os = "macos"))]
pub fn should_record_deferred_off(engine_active: bool, deferred: bool) -> bool {
    !engine_active && deferred
}

/// Quit before adopt: the donor will resume. Do not POST `offline:true`.
#[cfg(any(test, target_os = "macos"))]
pub fn should_detach_cloud_on_quit(engine_active: bool, our_pid: u32) -> bool {
    should_defer_local_controls(engine_active, our_pid)
}

/// A remembered Off / ⌥⌘P must still stop the donor if this process cannot adopt.
#[cfg(any(test, target_os = "macos"))]
pub fn should_stop_donor_on_failed_handoff(
    handoff: bool,
    adopted: bool,
    pending_stop: bool,
) -> bool {
    handoff && !adopted && pending_stop
}

/// Only one foreground process may poll commands for the persisted identity.
#[cfg(test)]
pub fn should_claim_reporter_lock(
    our_pid: u32,
    lock_pid: Option<u32>,
    lock_holder_alive: bool,
) -> bool {
    match lock_pid {
        None => true,
        Some(pid) if pid == our_pid => true,
        Some(_) => !lock_holder_alive,
    }
}

/// Cloud polling for `never-sleep on` when no menu owns IPC.
pub fn should_claim_foreground_reporter_lock(cloud_ok: bool, menu_socket_absent: bool) -> bool {
    cloud_ok && menu_socket_absent
}

/// Do not Start a second local session if this process cannot own the reporter.
pub fn should_abort_foreground_without_reporter_lock(needs_reporter: bool, claimed: bool) -> bool {
    needs_reporter && !claimed
}

#[cfg(test)]
fn read_reporter_lock() -> Option<SessionLockRecord> {
    let text = std::fs::read_to_string(crate::paths::reporter_lock_path()).ok()?;
    Some(parse_lock_record(&text))
}

thread_local! {
    static REPORTER_LOCK: RefCell<Option<(PathBuf, File)>> = const { RefCell::new(None) };
}

fn lock_reporter_file(file: &File) -> bool {
    #[cfg(target_os = "linux")]
    {
        let mut lock = libc::flock {
            l_type: libc::F_WRLCK as i16,
            l_whence: libc::SEEK_SET as i16,
            l_start: 0,
            l_len: 0,
            l_pid: 0,
        };
        // SAFETY: `file` is an fd we own; F_OFD_SETLK takes a non-blocking
        // exclusive open-file-description lock so two opens cannot both succeed.
        unsafe { libc::fcntl(file.as_raw_fd(), libc::F_OFD_SETLK, &mut lock) == 0 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        // SAFETY: `file` is an fd we own; flock(LOCK_EX|LOCK_NB) excludes other
        // processes until this fd is dropped.
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) == 0 }
    }
}

fn holding_current_reporter_lock() -> bool {
    let path = crate::paths::reporter_lock_path();
    REPORTER_LOCK.with(|slot| {
        let mismatch = slot
            .borrow()
            .as_ref()
            .is_some_and(|(held, _)| held != &path);
        if mismatch {
            slot.borrow_mut().take();
            false
        } else {
            slot.borrow().is_some()
        }
    })
}

/// Exclusive cloud polling for `never-sleep on` when no menu owns IPC.
///
/// Ownership is the held flock / OFD lock, not pid liveness. A leftover file
/// from a crash is taken over without unlink-then-create_new.
pub fn try_claim_reporter_lock(our_pid: u32) -> bool {
    if our_pid == 0 {
        return false;
    }
    if holding_current_reporter_lock() {
        return true;
    }
    let Ok(_) = crate::paths::ensure_data_dir() else {
        return false;
    };
    let path = crate::paths::reporter_lock_path();
    let mut file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    {
        Ok(file) => file,
        Err(_) => return false,
    };
    if !lock_reporter_file(&file) {
        return false;
    }
    let body = format_lock_text(our_pid, false, process_starttime(our_pid));
    if file.set_len(0).is_err()
        || file.seek(SeekFrom::Start(0)).is_err()
        || file.write_all(body.as_bytes()).is_err()
    {
        return false;
    }
    REPORTER_LOCK.with(|slot| {
        *slot.borrow_mut() = Some((path, file));
    });
    true
}

pub fn release_reporter_lock(_our_pid: u32) {
    REPORTER_LOCK.with(|slot| {
        slot.borrow_mut().take();
    });
}

#[cfg(test)]
fn hold_reporter_lock_for_test() -> File {
    let _ = crate::paths::ensure_data_dir();
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(crate::paths::reporter_lock_path())
        .expect("open reporter.lock for the competing test holder");
    assert!(
        lock_reporter_file(&file),
        "test holder must take the exclusive reporter lock"
    );
    file
}

/// Keep a foreign clamshell=1 lock when inherited restore failed, even if the
/// donor is already dead. Next launch still has something to recover from.
#[cfg(any(test, target_os = "macos"))]
pub fn should_keep_inherited_clamshell_lock(
    failed_restore: bool,
    lock: Option<(u32, bool)>,
    our_pid: u32,
) -> bool {
    failed_restore && matches!(lock, Some((pid, true)) if pid != our_pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timed_out_donor_does_not_overwrite_live_successor_lock() {
        assert!(
            !should_claim_session_lock(11, Some(22), true, true),
            "after a lost adopt reply, Tick/ApplyPower must not replace the menu's session.lock"
        );
        assert!(
            should_claim_session_lock(11, Some(22), true, false),
            "the adopting menu must take the live donor's lock during handoff"
        );
        assert!(should_claim_session_lock(11, Some(11), true, true));
        assert!(should_claim_session_lock(11, Some(22), false, true));
        assert!(should_claim_session_lock(11, None, false, false));
        let macos = include_str!("platform/macos.rs");
        let apply = macos.split("fn apply_power").nth(1).expect("apply_power");
        assert!(
            apply.contains("already_holding"),
            "ApplyPower must snapshot owns_power before assigning it, so adopt is not treated as a donor retry"
        );
        let claim_at = apply
            .find("should_claim_session_lock")
            .expect("ApplyPower must consult successor ownership before write_lock");
        let clamshell_at = apply
            .find("should_clear_unclaimed_clamshell")
            .expect("clamshell selector follows ownership");
        let write_at = apply
            .rfind("write_lock")
            .expect("success path still records the lock when this process owns it");
        assert!(
            claim_at < clamshell_at,
            "a live successor's clamshell flag must not be restored by a timed-out donor Tick"
        );
        assert!(
            claim_at < write_at,
            "a live menu lock must survive a timed-out donor's later ApplyPower"
        );
    }

    #[test]
    fn live_menu_lock_survives_foreground_release() {
        assert!(
            !should_release_clamshell_lock(11, Some(22), true),
            "the handing-off process must not drop a live menu's clamshell ownership"
        );
        assert!(should_release_clamshell_lock(11, Some(11), true));
        assert!(should_release_clamshell_lock(11, Some(22), false));
        assert!(should_release_clamshell_lock(11, None, false));
        assert!(
            should_defer_local_controls(false, 11) == should_hold_cloud_commands(false, 11),
            "⌥⌘P before adopt must follow the same live-donor lock as held phone commands"
        );
        let gui = include_str!("gui.rs");
        assert!(
            gui.contains("should_defer_local_controls") && gui.contains("ok_adopted"),
            "handoff IPC must confirm this process adopted, and Toggle must not start over a live donor"
        );
        assert!(
            gui.contains("ipc_owned") && gui.contains("spawn_reporter"),
            "menu reporter starts only after this process owns the IPC socket"
        );
    }

    #[test]
    fn macos_release_power_defers_to_live_peer_lock() {
        let src = include_str!("platform/macos.rs");
        assert!(
            src.contains("should_release_clamshell_lock"),
            "MacPlatform::release_power must not blindly restore clamshell sleep"
        );
        assert!(
            src.contains("should_restore_clamshell"),
            "a live peer that did not claim clamshell still needs the flag restored"
        );
        assert!(
            src.contains("pid_alive"),
            "ownership follows the pid recorded in session.lock"
        );
        let panic_hook = src
            .split("fn install_panic_cleanup")
            .nth(1)
            .expect("panic cleanup");
        assert!(
            panic_hook.contains("should_restore_clamshell")
                && panic_hook.contains("should_release_clamshell_lock"),
            "panic while OWNS_POWER must not wipe a successor's session.lock"
        );
    }

    #[test]
    fn restores_clamshell_when_successor_did_not_claim_it() {
        assert!(
            should_restore_clamshell(10, Some((20, false)), true),
            "menu adopted without lid-awake: the handing-off process must clear the global flag"
        );
        assert!(
            !should_release_clamshell_lock(10, Some(20), true),
            "keep the menu's session.lock even when it did not take clamshell"
        );
    }

    #[test]
    fn leaves_clamshell_when_successor_claimed_it() {
        assert!(!should_restore_clamshell(10, Some((20, true)), true));
        assert!(!should_release_clamshell_lock(10, Some(20), true));
    }

    #[test]
    fn restores_clamshell_for_own_or_dead_lock() {
        assert!(should_restore_clamshell(10, Some((10, true)), true));
        assert!(should_restore_clamshell(10, Some((20, true)), false));
        assert!(should_restore_clamshell(10, None, false));
    }

    #[test]
    fn successor_clears_unclaimed_inherited_clamshell_before_ack() {
        assert!(
            should_clear_unclaimed_clamshell(false),
            "menu adopted without lid-awake must restore the inherited flag itself"
        );
        assert!(
            !should_clear_unclaimed_clamshell(true),
            "menu that claims clamshell keeps the flag disabled"
        );
        let macos = include_str!("platform/macos.rs");
        assert!(
            macos.contains("should_clear_unclaimed_clamshell"),
            "apply_power must restore an inherited flag when this process is not claiming it"
        );
        let apply = macos.split("fn apply_power").nth(1).expect("apply_power");
        let fail_at = apply
            .find("idle_assertion_failed")
            .expect("assertion failure path");
        let clear_at = apply
            .find("should_clear_unclaimed_clamshell")
            .expect("inherited clear");
        assert!(
            fail_at < clear_at,
            "do not restore inherited clamshell before PreventUserIdleSystemSleep succeeds"
        );
        let ipc = include_str!("gui.rs");
        let on_arm = ipc
            .split("IpcRequest::On")
            .nth(1)
            .expect("On arm")
            .split("IpcRequest::Off")
            .next()
            .unwrap();
        let dispatch_at = on_arm.find("dispatch(").expect("handoff dispatch");
        let reply_at = on_arm.find("ok_status").expect("handoff ack");
        assert!(
            dispatch_at < reply_at,
            "restore via ApplyPower must happen before the IPC ok that lets the donor detach"
        );
    }

    #[test]
    fn unclaimed_clamshell_restore_failure_does_not_write_lock() {
        let macos = include_str!("platform/macos.rs");
        let apply = macos.split("fn apply_power").nth(1).expect("apply_power");
        let clear_at = apply
            .find("should_clear_unclaimed_clamshell")
            .expect("inherited clear");
        let clear_arm = &apply[clear_at..];
        let restore_fail = clear_arm
            .find("if !set_clamshell_sleep_disabled(false)")
            .expect("restore failure must be checked");
        let fail_arm = &clear_arm[restore_fail..];
        let err_at = fail_arm
            .find("return Err")
            .expect("adopt fails if restore fails");
        let write_at = apply
            .find("write_lock")
            .expect("success path still records the lock");
        let fail_abs = clear_at + restore_fail + err_at;
        assert!(
            fail_abs < write_at,
            "a failed IOKit restore must not write clamshell=0 before returning Err"
        );
        assert!(
            fail_arm.contains("owns_power = true") && fail_arm.contains("release_power"),
            "keep ownership so release_power sees a live donor lock and does not clear the flag"
        );
        assert!(
            fail_arm.contains("should_fail_unclaimed_clamshell_restore"),
            "restore failure is fatal when a foreign lock recorded clamshell=1"
        );
        assert!(
            fail_arm.contains("should_keep_inherited_clamshell_lock")
                || fail_arm.contains("keep_inherited_lock"),
            "failed restore must mark the inherited lock so release_power does not delete it"
        );
        assert!(
            should_keep_inherited_clamshell_lock(true, Some((20, true)), 10),
            "a dead donor clamshell=1 lock must survive a failed restore"
        );
        assert!(!should_keep_inherited_clamshell_lock(
            false,
            Some((20, true)),
            10
        ));
        assert!(!should_keep_inherited_clamshell_lock(
            true,
            Some((10, true)),
            10
        ));
        assert!(!should_keep_inherited_clamshell_lock(
            true,
            Some((20, false)),
            10
        ));
        let release = macos
            .split("fn release_power")
            .nth(1)
            .expect("release_power");
        assert!(
            release.contains("should_keep_inherited_clamshell_lock"),
            "release_power must not delete the recovery lock after a failed inherited restore"
        );
    }

    #[test]
    fn restore_failure_is_fatal_only_with_live_inherited_clamshell() {
        assert!(
            !should_fail_unclaimed_clamshell_restore(false, None, false, 10),
            "lid-open display-off with no donor must still start if IOKit restore is unavailable"
        );
        assert!(!should_fail_unclaimed_clamshell_restore(
            false,
            Some((10, true)),
            true,
            10
        ));
        assert!(!should_fail_unclaimed_clamshell_restore(
            false,
            Some((20, false)),
            true,
            10
        ));
        assert!(!should_fail_unclaimed_clamshell_restore(
            true,
            Some((20, true)),
            true,
            10
        ));
        assert!(
            should_fail_unclaimed_clamshell_restore(false, Some((20, true)), true, 10),
            "handoff must not ack if inherited clamshell=1 cannot be restored"
        );
        assert!(
            should_fail_unclaimed_clamshell_restore(false, Some((20, true)), false, 10),
            "a dead donor lock that recorded clamshell=1 still needs a successful restore"
        );
    }

    #[test]
    fn deferred_escape_stops_after_adopt() {
        let mut pending = false;
        note_deferred_escape(false, &mut pending);
        assert!(
            !pending,
            "Toggle while this process owns standby is not an overlapping escape"
        );
        note_deferred_escape(true, &mut pending);
        assert!(pending, "⌥⌘P during the donor overlap must not be dropped");
        note_deferred_escape(false, &mut pending);
        assert!(
            pending,
            "a later non-deferred action must not forget the escape"
        );
        assert!(
            !take_pending_stop_on_adopt(false, &mut pending),
            "failed adopt must keep the escape for a later handoff"
        );
        assert!(pending);
        assert!(
            take_pending_stop_after_handoff(false, true, &mut pending),
            "stop_donor after a zero-remaining handoff must still apply the remembered Off"
        );
        assert!(!pending);
        note_deferred_escape(true, &mut pending);
        assert!(take_pending_stop_on_adopt(true, &mut pending));
        assert!(!pending);
        assert!(
            !take_pending_stop_on_adopt(true, &mut pending),
            "the remembered escape is one-shot"
        );
        let gui = include_str!("gui.rs");
        assert!(
            gui.contains("note_deferred_escape") && gui.contains("take_pending_stop_after_handoff"),
            "menu Toggle / ⌥⌘P must record a pending stop and apply it after adopt or stop_donor"
        );
        let loop_src = gui
            .split("while let Ok(incoming) = ipc_rx.try_recv()")
            .nth(1)
            .expect("ipc loop")
            .split("match event")
            .next()
            .unwrap();
        let drain_at = loop_src
            .rfind("apply_polled_commands")
            .expect("post-handoff drain");
        let stop_at = loop_src
            .find("take_pending_stop_after_handoff")
            .expect("escape after held commands");
        assert!(
            drain_at < stop_at,
            "held phone On must not restart standby after the remembered escape"
        );
        let handle = gui
            .split("fn handle_ipc")
            .nth(1)
            .expect("handle_ipc")
            .split("fn local_controls_deferred")
            .next()
            .unwrap();
        let send_at = handle.find("reply.send").expect("IPC reply");
        assert!(
            !handle.contains("take_pending_stop_after_handoff")
                && !handle.contains("take_pending_stop_on_adopt"),
            "do not Stop inside handle_ipc before the handoff_first drain"
        );
        assert!(
            loop_src.find("handle_ipc").expect("handle_ipc call") < stop_at && send_at > 0,
            "donor must see adopted+active before the menu applies the remembered escape"
        );
        let off = gui
            .split("IpcRequest::Off")
            .nth(1)
            .expect("Off")
            .split("IpcRequest::Toggle")
            .next()
            .unwrap();
        assert!(
            off.contains("note_deferred_escape")
                && (off.contains("should_record_deferred_off")
                    || off.contains("local_controls_deferred")),
            "never-sleep off during the donor overlap must record the same pending stop"
        );
        assert!(
            should_record_deferred_off(false, true),
            "idle menu + live donor: Off is an escape, not a no-op"
        );
        assert!(!should_record_deferred_off(true, false));
        assert!(!should_record_deferred_off(false, false));
        let flush = gui
            .split("fn flush_cloud_on_quit")
            .nth(1)
            .expect("flush_cloud_on_quit")
            .split("fn handle_menu_event")
            .next()
            .unwrap();
        assert!(
            flush.contains("should_detach_cloud_on_quit") && flush.contains("detach"),
            "quit before adopt must detach so the donor can resume without an offline heartbeat"
        );
        assert!(
            should_detach_cloud_on_quit(false, 11) == should_defer_local_controls(false, 11),
            "pre-adopt quit follows the same live-donor lock as deferred Toggle"
        );
        assert!(
            should_stop_donor_on_failed_handoff(true, false, true),
            "failed adopt must still deliver a deferred Off to the live donor"
        );
        assert!(!should_stop_donor_on_failed_handoff(true, true, true));
        assert!(!should_stop_donor_on_failed_handoff(true, false, false));
        assert!(!should_stop_donor_on_failed_handoff(false, false, true));
        assert!(
            handle.contains("should_stop_donor_on_failed_handoff")
                && (handle.contains("stop_donor") || gui.contains("stop_donor")),
            "handoff IPC must tell the donor to stop when adopt cannot apply the deferred Off"
        );
        let fg = include_str!("foreground.rs");
        assert!(
            fg.contains("donor_should_stop"),
            "foreground must honor stop_donor instead of resuming standby after a failed adopt"
        );
        assert!(
            gui.contains("menu_confirms_prior_handoff"),
            "a lost handoff reply must still confirm this donor's already-adopted session"
        );
        assert!(
            gui.contains("menu_already_processed_handoff")
                && handle.contains("should_stop_donor_after_ended_prior_handoff"),
            "matching handoff id after the menu stopped must stop the donor, not dispatch again"
        );
        let persist_at = handle
            .find("write_handoff_ack")
            .expect("persist the adopt/stop decision before the IPC reply");
        let send_at = handle.rfind("reply.send").expect("final IPC reply");
        assert!(
            persist_at < send_at,
            "a timed-out donor must be able to read the ack after the menu has already quit"
        );
        assert!(
            handle.contains("should_reject_adopt_if_ack_unpersisted")
                && handle.contains("handoff_ack_failed"),
            "adopt must roll back when handoff.ack cannot be written"
        );
        assert!(
            handle.contains("should_reject_stop_if_ack_unpersisted"),
            "a deferred Off must persist Stop before the donor can rely on the IPC reply"
        );
        assert!(
            loop_src.contains("should_skip_handoff_drain_after_ack_failure")
                && loop_src.contains("skip_drain"),
            "held phone On must not restart standby after an unpersisted adopt rollback"
        );
        assert!(
            flush.contains("mark_handoff_ack_reporter_gone"),
            "Quit must clear ack reporter ownership before the socket disappears"
        );
        assert!(
            handle.contains("handoff_ack_reporter")
                && handle.contains("identity.is_some()")
                && !handle.contains("identity.is_some() && adopted"),
            "Stop acks must record a surviving menu reporter, not only Adopted"
        );
        assert!(
            loop_src.contains("stop_donor"),
            "apply the remembered escape after drain when the response stops the donor"
        );
    }

    #[test]
    fn reused_pid_is_not_a_live_lock_holder() {
        assert!(
            lock_holder_is_live(true, None, Some(99)),
            "locks written before starttime still treat a live pid as the owner"
        );
        assert!(lock_holder_is_live(true, Some(10), Some(10)));
        assert!(
            !lock_holder_is_live(true, Some(10), Some(99)),
            "SIGKILL leftover must not follow a later process that reused the pid"
        );
        assert!(!lock_holder_is_live(true, Some(10), None));
        assert!(!lock_holder_is_live(false, Some(10), Some(10)));
        let rec = parse_lock_record("pid=22\nclamshell=1\nstarttime=4242\n");
        assert_eq!(rec.pid, 22);
        assert!(rec.clamshell);
        assert_eq!(rec.starttime, Some(4242));
        let body = format_lock_text(22, true, Some(4242));
        assert_eq!(parse_lock_text(&body), (22, true));
        assert!(
            body.contains("starttime=4242"),
            "session.lock must record a start token, not only pid"
        );
        let macos = include_str!("platform/macos.rs");
        assert!(
            macos.contains("format_lock_text") && macos.contains("process_starttime"),
            "apply_power must write pid+starttime so orphan cleanup can reject pid reuse"
        );
        assert!(
            macos.contains("lock_holder_is_live"),
            "cleanup / release_power must validate starttime, not only kill(pid, 0)"
        );
        let own = process_starttime(std::process::id());
        assert!(
            own.is_some(),
            "this process must be able to read its own start token"
        );
        assert!(lock_holder_is_live(
            pid_is_alive(std::process::id()),
            own,
            own
        ));
        assert_eq!(
            parse_proc_stat_starttime(
                "1 (init) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 42 0 0 0 0 0 0 0 0 0 0 0"
            ),
            Some(42)
        );
    }

    #[test]
    fn second_foreground_does_not_claim_a_live_reporter_lock() {
        assert!(
            !should_claim_reporter_lock(11, Some(22), true),
            "two never-sleep on processes must not both poll the same identity"
        );
        assert!(should_claim_reporter_lock(11, Some(22), false));
        assert!(should_claim_reporter_lock(11, Some(11), true));
        assert!(should_claim_reporter_lock(11, None, false));
        assert!(
            should_abort_foreground_without_reporter_lock(true, false),
            "a second never-sleep on must not Start when reporter.lock is denied"
        );
        assert!(!should_abort_foreground_without_reporter_lock(true, true));
        assert!(
            !should_abort_foreground_without_reporter_lock(false, false),
            "a live menu still accepts handoff; lock denial must not reject that path"
        );
        assert!(should_claim_foreground_reporter_lock(true, true));
        assert!(!should_claim_foreground_reporter_lock(true, false));
        assert!(!should_claim_foreground_reporter_lock(false, true));
        let claim_src = include_str!("session_lock.rs")
            .split("pub fn try_claim_reporter_lock")
            .nth(1)
            .expect("try_claim_reporter_lock")
            .split("pub fn release_reporter_lock")
            .next()
            .unwrap();
        assert!(
            !claim_src.contains("remove_file"),
            "replacing a leftover reporter.lock must not unlink then create_new"
        );
        let _dir = crate::paths::TestDataDir::install();
        let ours = std::process::id();
        assert!(
            try_claim_reporter_lock(ours),
            "the first foreground reporter may start cloud polling"
        );
        assert!(
            try_claim_reporter_lock(ours),
            "the owning process may re-enter the lock"
        );
        release_reporter_lock(ours);
        std::fs::write(
            crate::paths::reporter_lock_path(),
            format_lock_text(1, false, process_starttime(1)),
        )
        .unwrap();
        assert!(
            try_claim_reporter_lock(ours),
            "a leftover reporter.lock without a held flock must be taken over atomically"
        );
        release_reporter_lock(ours);
        let foreign = hold_reporter_lock_for_test();
        assert!(
            !try_claim_reporter_lock(ours),
            "a second foreground must not steal a held reporter lock"
        );
        drop(foreign);
        assert!(
            try_claim_reporter_lock(ours),
            "releasing the held lock must allow the next foreground to poll"
        );
        release_reporter_lock(ours);
        assert!(
            read_reporter_lock().is_some(),
            "release must drop the fd without unlinking, so a racer cannot lock a new inode"
        );
        let release_src = include_str!("session_lock.rs")
            .split("pub fn release_reporter_lock")
            .nth(1)
            .expect("release_reporter_lock")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !release_src.contains("remove_file"),
            "ownership is the held fd; unlinking during release splits the inode"
        );
        let fg = include_str!("foreground.rs");
        assert!(
            fg.contains("try_claim_reporter_lock") && fg.contains("release_reporter_lock"),
            "foreground must take exclusive reporter ownership before heartbeats"
        );
        let start = fg.find("pub fn run_foreground").expect("run_foreground");
        let before_loop = fg[start..]
            .split("while running")
            .next()
            .expect("foreground loop");
        let claim_at = before_loop
            .find("try_claim_reporter_lock")
            .expect("claim reporter.lock before Start");
        let refuse_at = before_loop
            .find("should_refuse_foreground_while_menu_live")
            .expect("refuse Start while ipc.sock is live");
        let dispatch_at = before_loop
            .find("dispatch(")
            .expect("Start dispatch after the reporter claim");
        assert!(
            refuse_at < dispatch_at
                && claim_at < dispatch_at
                && before_loop.contains("should_abort_foreground_without_reporter_lock"),
            "a live menu or denied reporter.lock must abort before dispatching Start"
        );
        let take = fg
            .split("fn take_foreground_reporter")
            .nth(1)
            .expect("take_foreground_reporter")
            .split("fn spawn_foreground_reporter")
            .next()
            .unwrap();
        assert!(
            !take.contains("release_reporter_lock"),
            "the lock must stay held until detach or publish_and_flush has joined"
        );
        for marker in ["handle.detach();", "publish_and_flush("] {
            let at = fg.find(marker).unwrap_or_else(|| panic!("{marker}"));
            let after = &fg[at + marker.len()..];
            let release_at = after
                .find("release_reporter_lock")
                .unwrap_or_else(|| panic!("release after {marker}"));
            let next_fn = after.find("\nfn ").unwrap_or(after.len());
            assert!(
                release_at < next_fn,
                "{marker} must drop the lock only after the reporter has stopped"
            );
        }
    }
}
