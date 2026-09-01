//! Menu-bar panel copy and navigation, kept free of AppKit so Linux CI can lock it.

use never_sleep_core::{AppConfig, DurationPref, Lang, ViewModel, DEFAULT_HOTKEY_LABEL};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlassKind {
    LiquidGlass,
    Vibrancy,
}

/// macOS 26+ has `NSGlassEffectView`; older releases use `NSVisualEffectView`.
pub fn preferred_glass(ns_glass_effect_view_available: bool) -> GlassKind {
    if ns_glass_effect_view_available {
        GlassKind::LiquidGlass
    } else {
        GlassKind::Vibrancy
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelView {
    Main,
    Settings,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationKey {
    Indefinite,
    Hours1,
    Hours3,
    Hours8,
    Until0800,
}

impl DurationKey {
    pub fn from_pref(pref: DurationPref) -> Self {
        match pref {
            DurationPref::Hours { hours: 1 } => Self::Hours1,
            DurationPref::Hours { hours: 3 } => Self::Hours3,
            DurationPref::Hours { hours: 8 } => Self::Hours8,
            DurationPref::UntilLocal { hour: 8, minute: 0 } => Self::Until0800,
            DurationPref::Indefinite
            | DurationPref::Hours { .. }
            | DurationPref::UntilLocal { .. } => Self::Indefinite,
        }
    }

    pub fn index(self) -> isize {
        match self {
            Self::Indefinite => 0,
            Self::Hours1 => 1,
            Self::Hours3 => 2,
            Self::Hours8 => 3,
            Self::Until0800 => 4,
        }
    }

    pub fn from_index(index: isize) -> Option<Self> {
        match index {
            0 => Some(Self::Indefinite),
            1 => Some(Self::Hours1),
            2 => Some(Self::Hours3),
            3 => Some(Self::Hours8),
            4 => Some(Self::Until0800),
            _ => None,
        }
    }

    pub fn as_ipc(self) -> &'static str {
        match self {
            Self::Indefinite => "indefinite",
            Self::Hours1 => "1h",
            Self::Hours3 => "3h",
            Self::Hours8 => "8h",
            Self::Until0800 => "until_0800",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelState {
    pub active: bool,
    pub lang: Lang,
    pub duration: DurationKey,
    pub warning: String,
    pub screen_off: bool,
    pub lid_awake: bool,
    pub resleep_display: bool,
    pub lock_screen: bool,
    pub battery_floor: bool,
    pub launch_at_login: bool,
    pub status_title: String,
    pub summary: String,
    pub primary_action: String,
    pub duration_label: String,
    pub duration_indefinite: String,
    pub duration_1h: String,
    pub duration_3h: String,
    pub duration_8h: String,
    pub duration_until: String,
    pub resleep: String,
    pub battery: String,
    pub more_settings: String,
    pub settings: String,
    pub back: String,
    pub screen_off_label: String,
    pub lid_awake_label: String,
    pub lock_screen_label: String,
    pub launch_at_login_label: String,
    pub help: String,
    pub quit: String,
    pub help_kicker: String,
    pub help_lead: String,
    pub help_how: String,
    pub help_step1_title: String,
    pub help_step1_detail: String,
    pub help_step2_title: String,
    pub help_step2_detail: String,
    pub help_step3_title: String,
    pub help_step3: String,
    pub help_notes: String,
    pub help_note_lid: String,
    pub help_note_battery: String,
    pub help_note_quit: String,
}

pub fn panel_state(cfg: &AppConfig, vm: &ViewModel) -> PanelState {
    let t = cfg.tr();
    let help_step3 = format!(
        "{} {} {}",
        t.help_step3_before(),
        DEFAULT_HOTKEY_LABEL,
        t.help_step3_after()
    );
    PanelState {
        active: vm.active,
        lang: cfg.lang(),
        duration: DurationKey::from_pref(vm.duration),
        warning: vm.warnings.first().cloned().unwrap_or_default(),
        screen_off: vm.screen_off,
        lid_awake: vm.keep_awake_on_lid_close,
        resleep_display: vm.resleep_display,
        lock_screen: vm.lock_screen,
        battery_floor: cfg.battery_floor_percent.is_some(),
        launch_at_login: vm.launch_at_login,
        status_title: if vm.active {
            t.panel_active_title()
        } else {
            t.panel_idle_title()
        }
        .into(),
        summary: if vm.active {
            t.panel_summary_active()
        } else {
            t.panel_summary_idle()
        }
        .into(),
        primary_action: vm.primary_action.clone(),
        duration_label: t.duration_menu().into(),
        duration_indefinite: t.indefinite().into(),
        duration_1h: t.hours(1),
        duration_3h: t.hours(3),
        duration_8h: t.hours(8),
        duration_until: t.until_clock(8, 0),
        resleep: t.resleep_display().into(),
        battery: vm.battery_floor_label.clone(),
        more_settings: t.more_settings().into(),
        settings: t.settings_title().into(),
        back: t.back().into(),
        screen_off_label: t.screen_off_now().into(),
        lid_awake_label: t.lid_awake().into(),
        lock_screen_label: t.lock_screen().into(),
        launch_at_login_label: t.launch_at_login().into(),
        help: t.help_title().into(),
        quit: t.quit().into(),
        help_kicker: t.help_kicker().into(),
        help_lead: t.help_lead().into(),
        help_how: t.help_how().into(),
        help_step1_title: t.help_step1_title().into(),
        help_step1_detail: t.help_step1_detail().into(),
        help_step2_title: t.help_step2_title().into(),
        help_step2_detail: t.help_step2_detail().into(),
        help_step3_title: t.help_step3_title().into(),
        help_step3,
        help_notes: t.help_notes().into(),
        help_note_lid: t.help_note_lid().into(),
        help_note_battery: t.help_note_battery().into(),
        help_note_quit: t.help_note_quit().into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use never_sleep_core::{Engine, HostSnapshot, Thermal, Tr, DEFAULT_BATTERY_FLOOR};

    fn host() -> HostSnapshot {
        HostSnapshot {
            monotonic_ms: 5_000,
            unix_secs: 1_700_000_000,
            utc_offset_secs: 0,
            on_ac: true,
            battery_percent: Some(64),
            lid_closed: false,
            display_asleep: Some(false),
            hid_idle_ms: 1_000,
            thermal: Thermal::Nominal,
        }
    }

    #[test]
    fn preferred_glass_uses_liquid_glass_when_class_exists() {
        assert_eq!(preferred_glass(true), GlassKind::LiquidGlass);
        assert_eq!(preferred_glass(false), GlassKind::Vibrancy);
    }

    #[test]
    fn panel_views_are_main_settings_and_help() {
        assert_ne!(PanelView::Main, PanelView::Settings);
        assert_ne!(PanelView::Settings, PanelView::Help);
        assert_ne!(PanelView::Help, PanelView::Main);
    }

    #[test]
    fn duration_key_roundtrip_covers_menu_presets() {
        let cases = [
            (
                DurationPref::Indefinite,
                DurationKey::Indefinite,
                "indefinite",
            ),
            (DurationPref::Hours { hours: 1 }, DurationKey::Hours1, "1h"),
            (DurationPref::Hours { hours: 3 }, DurationKey::Hours3, "3h"),
            (DurationPref::Hours { hours: 8 }, DurationKey::Hours8, "8h"),
            (
                DurationPref::UntilLocal { hour: 8, minute: 0 },
                DurationKey::Until0800,
                "until_0800",
            ),
        ];
        for (pref, key, ipc) in cases {
            assert_eq!(DurationKey::from_pref(pref), key);
            assert_eq!(key.as_ipc(), ipc);
            assert_eq!(DurationKey::from_index(key.index()), Some(key));
        }
        assert_eq!(DurationKey::from_index(9), None);
    }

    #[test]
    fn panel_state_keeps_help_settings_and_more() {
        let cfg = AppConfig::default();
        let engine = Engine::new(cfg.clone());
        let state = panel_state(&cfg, &engine.view(&host()));
        let t = Tr::new(Lang::En);
        assert_eq!(state.more_settings, t.more_settings());
        assert_eq!(state.settings, t.settings_title());
        assert_eq!(state.help, t.help_title());
        assert_eq!(state.back, t.back());
        assert_eq!(state.help_how, t.help_how());
        assert_eq!(state.help_note_lid, t.help_note_lid());
        assert!(state.lid_awake_label.contains("best effort"));
        assert!(state.help_note_lid.contains("power"));
        assert!(state.help_step3.contains(DEFAULT_HOTKEY_LABEL));
        assert_eq!(state.battery, t.battery_floor_on(DEFAULT_BATTERY_FLOOR));
        assert!(!state.active);
        assert_eq!(state.status_title, t.panel_idle_title());
        assert_eq!(state.summary, t.panel_summary_idle());
    }

    #[test]
    fn panel_state_follows_language_and_standby() {
        let cfg = AppConfig {
            language: Some(Lang::Zh),
            ..AppConfig::default()
        };
        let mut engine = Engine::new(cfg.clone());
        let _ = engine.handle(never_sleep_core::Input::Start, &host());
        let vm = engine.view(&host());
        let state = panel_state(&engine.config, &vm);
        let t = Tr::new(Lang::Zh);
        assert!(state.active);
        assert_eq!(state.lang, Lang::Zh);
        assert_eq!(state.status_title, t.panel_active_title());
        assert_eq!(state.summary, t.panel_summary_active());
        assert_eq!(state.primary_action, t.end_standby());
        assert_eq!(state.more_settings, "更多设置");
        assert!(!state.help_note_lid.contains("熄屏待命"));
    }
}
