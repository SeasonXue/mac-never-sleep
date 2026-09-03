use std::fs;
use std::io;
#[cfg(any(test, target_os = "macos"))]
use std::path::Path;
use std::path::PathBuf;

/// Process override for the support directory. Empty values are ignored.
pub const DATA_DIR_ENV: &str = "NEVER_SLEEP_DATA_DIR";

#[cfg(test)]
thread_local! {
    static DATA_DIR_OVERRIDE: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
static TEST_DIR_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn data_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = DATA_DIR_OVERRIDE.with(|slot| slot.borrow().clone()) {
        return dir;
    }
    if let Ok(raw) = std::env::var(DATA_DIR_ENV) {
        if !raw.is_empty() {
            return PathBuf::from(raw);
        }
    }
    if cfg!(target_os = "macos") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("Library/Application Support/Never Sleep")
    } else {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("never-sleep")
    }
}

pub fn ensure_data_dir() -> io::Result<PathBuf> {
    let dir = data_dir();
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

pub fn cloud_identity_path() -> PathBuf {
    data_dir().join("cloud.toml")
}

pub fn ipc_socket_path() -> PathBuf {
    data_dir().join("ipc.sock")
}

pub fn session_lock_path() -> PathBuf {
    data_dir().join("session.lock")
}

#[cfg(any(test, target_os = "macos"))]
pub fn launch_agent_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents/com.seasonxue.never-sleep.plist")
}

#[cfg(target_os = "macos")]
pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("never-sleep"))
}

#[cfg(any(test, target_os = "macos"))]
pub fn is_inside_app_bundle(exe: &Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str().to_string_lossy().ends_with(".app"))
}

/// Temporary support directory for unit tests. Redirects `data_dir()` on this
/// thread so IPC / `save_config` never touch the user's real files.
#[cfg(test)]
pub struct TestDataDir {
    path: PathBuf,
}

#[cfg(test)]
impl TestDataDir {
    pub fn install() -> Self {
        let path = std::env::temp_dir().join(format!(
            "never-sleep-test-{}-{}",
            std::process::id(),
            TEST_DIR_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("temp data dir");
        DATA_DIR_OVERRIDE.with(|slot| *slot.borrow_mut() = Some(path.clone()));
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
impl Drop for TestDataDir {
    fn drop(&mut self) {
        DATA_DIR_OVERRIDE.with(|slot| {
            if slot.borrow().as_ref() == Some(&self.path) {
                *slot.borrow_mut() = None;
            }
        });
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_paths_share_the_support_directory() {
        let dir = data_dir();
        assert!(dir.ends_with("Never Sleep") || dir.ends_with("never-sleep"));
        assert_eq!(config_path(), dir.join("config.toml"));
        assert_eq!(cloud_identity_path(), dir.join("cloud.toml"));
        assert_eq!(ipc_socket_path(), dir.join("ipc.sock"));
        assert_eq!(session_lock_path(), dir.join("session.lock"));
        assert!(launch_agent_path().ends_with("com.seasonxue.never-sleep.plist"));
    }

    #[test]
    fn test_data_dir_redirects_config_and_socket() {
        let isolated = TestDataDir::install();
        assert_eq!(data_dir(), isolated.path());
        assert_eq!(config_path(), isolated.path().join("config.toml"));
        assert_eq!(cloud_identity_path(), isolated.path().join("cloud.toml"));
        assert_eq!(ipc_socket_path(), isolated.path().join("ipc.sock"));
        assert!(!ipc_socket_path().exists());
    }

    #[test]
    fn bundle_detection_looks_at_app_suffix() {
        let bundled = Path::new("/Applications/Never Sleep.app/Contents/MacOS/never-sleep");
        assert!(is_inside_app_bundle(bundled));
        assert!(!is_inside_app_bundle(Path::new(
            "/usr/local/bin/never-sleep"
        )));
    }
}
