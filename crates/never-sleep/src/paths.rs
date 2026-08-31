use std::fs;
use std::io;
#[cfg(any(test, target_os = "macos"))]
use std::path::Path;
use std::path::PathBuf;

pub fn data_dir() -> PathBuf {
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

pub fn ipc_socket_path() -> PathBuf {
    data_dir().join("ipc.sock")
}

#[cfg(any(test, target_os = "macos"))]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_paths_share_the_support_directory() {
        let dir = data_dir();
        assert!(dir.ends_with("Never Sleep") || dir.ends_with("never-sleep"));
        assert_eq!(config_path(), dir.join("config.toml"));
        assert_eq!(ipc_socket_path(), dir.join("ipc.sock"));
        assert_eq!(session_lock_path(), dir.join("session.lock"));
        assert!(launch_agent_path().ends_with("com.seasonxue.never-sleep.plist"));
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
