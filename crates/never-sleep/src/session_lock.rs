/// Whether this process should restore clamshell sleep and delete `session.lock`.
///
/// A live peer (the menu, after a foreground handoff) owns the file. Releasing
/// the previous process must not globally re-enable clamshell sleep or remove
/// the lock the next launch uses to restore the flag.
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
            src.contains("pid_alive"),
            "ownership follows the pid recorded in session.lock"
        );
    }
}
