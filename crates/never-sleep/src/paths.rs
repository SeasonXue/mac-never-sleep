#![allow(dead_code)]

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

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

pub fn session_lock_path() -> PathBuf {
    data_dir().join("session.lock")
}

pub fn launch_agent_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("Library/LaunchAgents/com.seasonxue.never-sleep.plist")
}

pub fn current_exe() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("never-sleep"))
}

pub fn is_inside_app_bundle(exe: &Path) -> bool {
    exe.components()
        .any(|c| c.as_os_str().to_string_lossy().ends_with(".app"))
}
