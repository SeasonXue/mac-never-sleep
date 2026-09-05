use std::fs;

use never_sleep_core::AppConfig;

use crate::locale;
use crate::paths::{config_path, ensure_data_dir};

pub fn load_config() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) => match toml::from_str::<AppConfig>(&text) {
            Ok(mut cfg) => {
                if cfg.language.is_none() {
                    cfg.language = Some(locale::detect_system());
                    save_config(&cfg);
                }
                cfg
            }
            Err(_) => system_config(),
        },
        Err(_) => system_config(),
    }
}

fn system_config() -> AppConfig {
    AppConfig {
        language: Some(locale::detect_system()),
        ..AppConfig::default()
    }
}

/// A failed-handoff donor must not rewrite Settings the live menu just saved.
pub fn should_persist_config(ipc_owner: bool, menu_socket_absent: bool) -> bool {
    ipc_owner || menu_socket_absent
}

pub fn save_config(cfg: &AppConfig) {
    if ensure_data_dir().is_err() {
        return;
    }
    if let Ok(text) = toml::to_string_pretty(cfg) {
        let _ = fs::write(config_path(), text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_handoff_donor_does_not_persist_stale_config() {
        crate::ipc::mark_ipc_server_owned(false);
        assert!(!crate::ipc::this_process_owns_ipc());
        assert!(
            should_persist_config(true, false),
            "the menu that owns IPC still saves Settings"
        );
        assert!(
            should_persist_config(false, true),
            "a solo foreground session still saves config"
        );
        assert!(
            !should_persist_config(false, false),
            "a live menu must not have its Settings overwritten by a failed-handoff donor"
        );
        let apply = include_str!("apply.rs");
        assert!(
            apply.contains("should_persist_config")
                && apply.contains("this_process_owns_ipc")
                && apply.contains("menu_socket_absent"),
            "Tick apply_effects must not save config while a live menu owns Settings"
        );
        let gui = include_str!("gui.rs");
        assert!(
            gui.contains("mark_ipc_server_owned(true)"),
            "the menu process must record IPC ownership before apply_effects can persist"
        );
    }
}
