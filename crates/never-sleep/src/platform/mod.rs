use never_sleep_core::{HostSnapshot, PowerPlan};

use crate::clock::{base_snapshot, monotonic_ms};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::MacPlatform;

pub trait Platform {
    fn snapshot(&self) -> HostSnapshot;
    fn apply_power(&mut self, plan: PowerPlan) -> Result<(), String>;
    fn release_power(&mut self) -> Result<(), String>;
    fn sleep_display(&self) -> Result<(), String>;
    fn lock_session(&self);
    fn notify(&self, title: &str, body: &str);
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String>;
    fn cleanup_orphans(&self);
    fn doctor(&self) -> String;
}

/// Linux / 测试用：只打印，不碰电源。
pub struct StubPlatform;

impl Platform for StubPlatform {
    fn snapshot(&self) -> HostSnapshot {
        base_snapshot(monotonic_ms())
    }

    fn apply_power(&mut self, plan: PowerPlan) -> Result<(), String> {
        eprintln!(
            "stub: apply_power idle={} system={} clamshell={}",
            plan.prevent_idle_sleep, plan.prevent_system_sleep, plan.disable_clamshell_sleep
        );
        Ok(())
    }

    fn release_power(&mut self) -> Result<(), String> {
        eprintln!("stub: release_power");
        Ok(())
    }

    fn sleep_display(&self) -> Result<(), String> {
        eprintln!("stub: sleep_display");
        Ok(())
    }

    fn lock_session(&self) {
        eprintln!("stub: lock_session");
    }

    fn notify(&self, title: &str, body: &str) {
        eprintln!("notify: {title} — {body}");
    }

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        eprintln!("stub: launch_at_login={enabled}");
        Ok(())
    }

    fn cleanup_orphans(&self) {}

    fn doctor(&self) -> String {
        crate::persist::load_config().tr().stub_not_macos().into()
    }
}

pub fn default_platform() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacPlatform::new())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(StubPlatform)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use never_sleep_core::PowerPlan;

    #[test]
    fn stub_platform_accepts_power_plan() {
        let mut p = StubPlatform;
        let host = p.snapshot();
        assert!(host.on_ac);
        assert!(p.apply_power(PowerPlan::off()).is_ok());
        assert!(p.release_power().is_ok());
        assert!(p.sleep_display().is_ok());
        p.lock_session();
        p.notify("t", "b");
        assert!(p.set_launch_at_login(false).is_ok());
        p.cleanup_orphans();
        assert!(!p.doctor().is_empty());
    }
}
