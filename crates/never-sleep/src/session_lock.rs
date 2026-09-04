use std::io::Read;

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

pub fn parse_lock_text(s: &str) -> (u32, bool) {
    let mut pid = 0u32;
    let mut clamshell = false;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("pid=") {
            pid = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = line.strip_prefix("clamshell=") {
            clamshell = v.trim() == "1";
        }
    }
    (pid, clamshell)
}

pub fn read_lock() -> Option<(u32, bool)> {
    let mut s = String::new();
    std::fs::File::open(crate::paths::session_lock_path())
        .ok()?
        .read_to_string(&mut s)
        .ok()?;
    Some(parse_lock_text(&s))
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

/// Hold phone On/Off while another live process still owns `session.lock`.
/// Applying Off against an idle menu is a no-op and would drop the command.
pub fn should_hold_cloud_commands(engine_active: bool, our_pid: u32) -> bool {
    if engine_active {
        return false;
    }
    matches!(
        read_lock(),
        Some((pid, _)) if pid != our_pid && pid_is_alive(pid)
    )
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

/// After a successful adopt, apply the remembered escape instead of keeping standby.
#[cfg(any(test, target_os = "macos"))]
pub fn take_pending_stop_on_adopt(adopted: bool, pending_stop: &mut bool) -> bool {
    let apply = adopted && *pending_stop;
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(take_pending_stop_on_adopt(true, &mut pending));
        assert!(!pending);
        assert!(
            !take_pending_stop_on_adopt(true, &mut pending),
            "the remembered escape is one-shot"
        );
        let gui = include_str!("gui.rs");
        assert!(
            gui.contains("note_deferred_escape") && gui.contains("take_pending_stop_on_adopt"),
            "menu Toggle / ⌥⌘P must record a pending stop and apply it after adopt"
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
            .find("take_pending_stop_on_adopt")
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
            !handle.contains("take_pending_stop_on_adopt"),
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
    }
}
