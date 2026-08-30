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
    let mut cfg = AppConfig::default();
    cfg.language = Some(locale::detect_system());
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
