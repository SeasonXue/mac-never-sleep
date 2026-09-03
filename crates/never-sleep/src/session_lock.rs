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
    }
}
