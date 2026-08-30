use std::fs;

use never_sleep_core::AppConfig;

use crate::locale;
use crate::paths::{config_path, ensure_data_dir};

pub fn load_config() -> AppConfig {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|_| detected_config()),
        Err(_) => detected_config(),
    }
}

fn detected_config() -> AppConfig {
    let mut cfg = AppConfig::default();
    cfg.language = locale::detect();
    cfg
}

pub fn save_config(cfg: &AppConfig) {
    if ensure_data_dir().is_err() {
        return;
    }
    if let Ok(text) = toml::to_string_pretty(cfg) {
        let _ = fs::write(config_path(), text);
    }
}
