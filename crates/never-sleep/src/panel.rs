//! Menu-bar panel copy and navigation, kept free of AppKit so Linux CI can lock it.

use std::time::{Duration, Instant};

use never_sleep_core::{AppConfig, DurationPref, Lang, ViewModel, DEFAULT_HOTKEY_LABEL};

/// Ignore a second Start/End click that AppKit queued from the same press.
pub const TOGGLE_COOLDOWN_MS: u64 = 400;

/// Compact menu-bar popover matching `docs/screenshots`.
pub const PANEL_WIDTH: f64 = 320.0;
pub const PANEL_HEIGHT: f64 = 480.0;
pub const HERO_SIZE: f64 = 124.0;
pub const HERO_IMAGE: f64 = 104.0;
pub const CARD_RADIUS: f64 = 8.0;
pub const CONTENT_INSET: f64 = 16.0;
/// Screenshot `.row`: 32pt cell, 11pt left/right. Vertical padding lives inside the 32pt.
pub const CARD_ROW_HEIGHT: f64 = 32.0;
pub const CARD_ROW_INSET_X: f64 = 11.0;
/// Rounded panel chrome; matches the HTML-era `.panel` radius.
pub const PANEL_CORNER: f64 = 10.0;
/// Transparent window padding so the layer shadow can fade out around the card.
pub const SHADOW_INSET: f64 = 40.0;
pub const SHADOW_RADIUS: f64 = 18.0;
/// Downward layer-shadow offset (Core Animation y-up, so the layer value is negative).
pub const SHADOW_OFFSET_Y: f64 = 6.0;
pub const SHADOW_OPACITY: f32 = 0.28;
/// Coin face swap duration (HTML-era `520ms` flip).
pub const HERO_FLIP_SECS: f64 = 0.52;
/// Idle `#f5f5f7` ↔ active `#1c1c1e` wash (HTML `420ms`).
pub const PANEL_COLOR_SECS: f64 = 0.42;
pub const IDLE_FILL_RGB: [u8; 3] = [0xf5, 0xf5, 0xf7];
pub const ACTIVE_FILL_RGB: [u8; 3] = [0x1c, 0x1c, 0x1e];
/// Numbered badge / SF Symbol in How-to and Keep-in-mind rows.
pub const HELP_ROW_GLYPH: f64 = 22.0;
pub const HELP_ROW_GAP: f64 = 10.0;
pub const HELP_ROW_INSET: f64 = 12.0;
/// Vertical padding inside How-to / Keep-in-mind rows (HTML-era `9px`).
pub const HELP_ROW_PAD_Y: f64 = 9.0;
/// Extra stack space around a grouped-card hairline. Screenshot rows sit on the line.
pub const CARD_SEPARATOR_GAP: f64 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelPlacement {
    /// Anchored under the status item; hide when the panel loses key focus.
    MenuBar,
}

pub fn panel_placement() -> PanelPlacement {
    PanelPlacement::MenuBar
}

pub fn dismiss_on_focus_loss() -> bool {
    matches!(panel_placement(), PanelPlacement::MenuBar)
}

pub fn window_width() -> f64 {
    PANEL_WIDTH + SHADOW_INSET * 2.0
}

pub fn window_height() -> f64 {
    PANEL_HEIGHT + SHADOW_INSET * 2.0
}

pub fn panel_fill_rgb(active: bool) -> [u8; 3] {
    if active {
        ACTIVE_FILL_RGB
    } else {
        IDLE_FILL_RGB
    }
}

/// Idle shows the sun; standby shows the moon. Never both at once.
pub fn hero_shows_moon(active: bool) -> bool {
    active
}

/// Container Y-axis angle: sun (front) at 0, moon (back) after a half-turn.
pub fn hero_flip_radians(showing_moon: bool) -> f64 {
    if showing_moon {
        std::f64::consts::PI
    } else {
        0.0
    }
}

pub fn panel_inner_width() -> f64 {
    PANEL_WIDTH - CONTENT_INSET * 2.0
}

/// Wrapping width beside a 22pt glyph inside a grouped card.
pub fn grouped_copy_max_width() -> f64 {
    panel_inner_width() - HELP_ROW_INSET * 2.0 - HELP_ROW_GLYPH - HELP_ROW_GAP
}

/// One wrapping How-to sentence: "按 ⌥⌘P，或点菜单…" / "Press ⌥⌘P or choose…".
pub fn join_help_step3(before: &str, hotkey: &str, after: &str) -> String {
    if after.starts_with('，') || after.starts_with(',') {
        format!("{before} {hotkey}{after}")
    } else {
        format!("{before} {hotkey} {after}")
    }
}

pub fn hero_flips(reduce_motion: bool) -> bool {
    !reduce_motion
}

pub fn motion_duration_secs(reduce_motion: bool, full_secs: f64) -> f64 {
    if reduce_motion {
        0.0
    } else {
        full_secs
    }
}

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

/// Sidebar destinations. Coarse `PanelView` stays for Help / Settings reachability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarItem {
    Standby,
    Display,
    Lid,
    Safeguards,
    General,
    Help,
}

impl SidebarItem {
    pub const ALL: [Self; 6] = [
        Self::Standby,
        Self::Display,
        Self::Lid,
        Self::Safeguards,
        Self::General,
        Self::Help,
    ];

    pub fn index(self) -> isize {
        match self {
            Self::Standby => 0,
            Self::Display => 1,
            Self::Lid => 2,
            Self::Safeguards => 3,
            Self::General => 4,
            Self::Help => 5,
        }
    }

    pub fn from_index(index: isize) -> Option<Self> {
        Self::ALL.iter().copied().find(|item| item.index() == index)
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Self::Standby => "moon.zzz",
            Self::Display => "display",
            Self::Lid => "laptopcomputer",
            Self::Safeguards => "checkmark.shield",
            Self::General => "gearshape",
            Self::Help => "questionmark.circle",
        }
    }

    pub fn as_panel_view(self) -> PanelView {
        match self {
            Self::Standby => PanelView::Main,
            Self::Help => PanelView::Help,
            Self::Display | Self::Lid | Self::Safeguards | Self::General => PanelView::Settings,
        }
    }
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
    pub section_session: String,
    pub section_display: String,
    pub section_lid: String,
    pub section_safeguards: String,
    pub section_general: String,
    pub language_label: String,
    pub hotkey_hint: String,
    pub settings: String,
    pub sidebar_options: String,
    pub sidebar_guide: String,
    pub pane_display_lead: String,
    pub pane_lid_lead: String,
    pub pane_safeguards_lead: String,
    pub pane_general_lead: String,
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
    pub help_step3_before: String,
    pub help_hotkey: String,
    pub help_step3_after: String,
    pub help_step3: String,
    pub help_notes: String,
    pub help_note_lid: String,
    pub help_note_battery: String,
    pub help_note_quit: String,
}

pub fn panel_state(cfg: &AppConfig, vm: &ViewModel) -> PanelState {
    let t = cfg.tr();
    let help_step3 = join_help_step3(
        t.help_step3_before(),
        DEFAULT_HOTKEY_LABEL,
        t.help_step3_after(),
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
        section_session: t.panel_section_session().into(),
        section_display: t.panel_section_display().into(),
        section_lid: t.panel_section_lid().into(),
        section_safeguards: t.panel_section_safeguards().into(),
        section_general: t.panel_section_general().into(),
        language_label: t.language_menu().into(),
        hotkey_hint: t.panel_hotkey_hint(),
        settings: t.settings_title().into(),
        sidebar_options: t.sidebar_group_options().into(),
        sidebar_guide: t.sidebar_group_guide().into(),
        pane_display_lead: t.pane_display_lead().into(),
        pane_lid_lead: t.warn_lid_best_effort().into(),
        pane_safeguards_lead: t.pane_safeguards_lead().into(),
        pane_general_lead: t.pane_general_lead().into(),
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
        help_step3_before: t.help_step3_before().into(),
        help_hotkey: DEFAULT_HOTKEY_LABEL.into(),
        help_step3_after: t.help_step3_after().into(),
        help_step3,
        help_notes: t.help_notes().into(),
        help_note_lid: t.help_note_lid().into(),
        help_note_battery: t.help_note_battery().into(),
        help_note_quit: t.help_note_quit().into(),
    }
}

/// Ignores extra panel toggle clicks for a short cooldown.
///
/// AppKit can queue two `toggle:` actions from a double-click before the first
/// `refresh_ui` paints. The control stays enabled so End Standby is never grayed
/// out waiting for the heartbeat.
#[derive(Debug, Default)]
pub struct ToggleGate {
    locked_until: Option<Instant>,
}

impl ToggleGate {
    pub fn take_click(&mut self) -> bool {
        self.take_click_at(Instant::now())
    }

    pub fn take_click_at(&mut self, now: Instant) -> bool {
        if self.locked_until.is_some_and(|until| now < until) {
            false
        } else {
            self.locked_until = Some(now + Duration::from_millis(TOGGLE_COOLDOWN_MS));
            true
        }
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
    fn sidebar_lists_standby_options_and_help() {
        assert_eq!(SidebarItem::ALL.len(), 6);
        assert_eq!(SidebarItem::Standby.as_panel_view(), PanelView::Main);
        assert_eq!(SidebarItem::Display.as_panel_view(), PanelView::Settings);
        assert_eq!(SidebarItem::Help.as_panel_view(), PanelView::Help);
        assert_eq!(SidebarItem::Standby.symbol(), "moon.zzz");
        assert_eq!(SidebarItem::from_index(5), Some(SidebarItem::Help));
        assert_eq!(SidebarItem::from_index(9), None);
        for item in SidebarItem::ALL {
            assert_eq!(SidebarItem::from_index(item.index()), Some(item));
        }
    }

    #[test]
    fn screenshot_panel_tokens_match_docs_shots() {
        assert_eq!(PANEL_WIDTH, 320.0);
        assert_eq!(PANEL_HEIGHT, 480.0);
        assert_eq!(HERO_SIZE, 124.0);
        assert_eq!(HERO_IMAGE, 104.0);
        assert_eq!(CARD_RADIUS, 8.0);
        assert_eq!(CONTENT_INSET, 16.0);
        assert_eq!(PANEL_CORNER, 10.0);
        assert_eq!(SHADOW_INSET, 40.0);
        assert_eq!(SHADOW_RADIUS, 18.0);
        assert_eq!(SHADOW_OFFSET_Y, 6.0);
        assert_eq!(SHADOW_OPACITY, 0.28);
        assert_eq!(
            SHADOW_INSET - SHADOW_RADIUS - SHADOW_OFFSET_Y,
            16.0,
            "window padding must outlast the blur so the shadow can fade before the window edge"
        );
        assert_eq!(HERO_FLIP_SECS, 0.52);
        assert_eq!(PANEL_COLOR_SECS, 0.42);
        assert_eq!(panel_fill_rgb(false), [0xf5, 0xf5, 0xf7]);
        assert_eq!(panel_fill_rgb(true), [0x1c, 0x1c, 0x1e]);
        assert!(!hero_shows_moon(false));
        assert!(hero_shows_moon(true));
        assert_eq!(hero_flip_radians(false), 0.0);
        assert_eq!(hero_flip_radians(true), std::f64::consts::PI);
        assert!(
            grouped_copy_max_width() < panel_inner_width(),
            "help/settings row copy sits beside a glyph, not the full inner width"
        );
        assert!(
            grouped_copy_max_width() > 180.0,
            "Chinese How-to sentences must still have room to wrap: {}",
            grouped_copy_max_width()
        );
        assert_eq!(window_width(), PANEL_WIDTH + SHADOW_INSET * 2.0);
        assert_eq!(window_height(), PANEL_HEIGHT + SHADOW_INSET * 2.0);
        assert!(
            window_width() > PANEL_WIDTH,
            "the window is larger than the card so the shadow can fade around the edges"
        );
    }

    #[test]
    fn panel_placement_anchors_under_the_menu_bar() {
        assert_eq!(panel_placement(), PanelPlacement::MenuBar);
        assert!(
            dismiss_on_focus_loss(),
            "a menu-bar popover must hide when it loses key focus"
        );
    }

    #[test]
    fn grouped_menu_rows_hug_the_hairline() {
        assert_eq!(CARD_ROW_HEIGHT, 32.0);
        assert_eq!(CARD_ROW_INSET_X, 11.0);
        assert_eq!(
            CARD_SEPARATOR_GAP, 0.0,
            "screenshot rows sit on the 0.5pt hairline; extra stack gap doubles the menu spacing"
        );
        assert_eq!(
            HELP_ROW_PAD_Y, 9.0,
            "How-to / Keep-in-mind padding matches the HTML-era 9px, not 12px plus a separator gap"
        );
    }

    #[test]
    fn help_copy_wraps_inside_the_grouped_card() {
        assert_eq!(HELP_ROW_GLYPH, 22.0);
        assert_eq!(HELP_ROW_GAP, 10.0);
        assert_eq!(HELP_ROW_INSET, 12.0);
        assert_eq!(HELP_ROW_PAD_Y, 9.0);
        assert_eq!(CARD_SEPARATOR_GAP, 0.0);
        assert_eq!(panel_inner_width(), 288.0);
        assert_eq!(grouped_copy_max_width(), 232.0);
        assert_eq!(
            join_help_step3("按", "⌥⌘P", "，或点菜单「结束待命」。"),
            "按 ⌥⌘P，或点菜单「结束待命」。"
        );
        assert_eq!(
            join_help_step3("Press", "⌥⌘P", "or choose “End Standby” in the menu."),
            "Press ⌥⌘P or choose “End Standby” in the menu."
        );
    }

    #[test]
    fn standby_click_is_one_half_turn() {
        assert_eq!(hero_flip_radians(false), 0.0);
        assert_eq!(hero_flip_radians(true), std::f64::consts::PI);
        assert_eq!(
            hero_flip_radians(true) - hero_flip_radians(false),
            std::f64::consts::PI,
            "Start Screen-Off Standby rotates the coin once, 0→π, not a full spin"
        );
        assert_eq!(HERO_FLIP_SECS, 0.52);
        assert!(hero_flips(false));
    }

    #[test]
    fn hero_flip_and_color_respect_reduce_motion() {
        assert!(hero_flips(false));
        assert!(!hero_flips(true));
        assert_eq!(motion_duration_secs(false, HERO_FLIP_SECS), HERO_FLIP_SECS);
        assert_eq!(motion_duration_secs(true, HERO_FLIP_SECS), 0.0);
        assert_eq!(
            motion_duration_secs(false, PANEL_COLOR_SECS),
            PANEL_COLOR_SECS
        );
        assert_eq!(motion_duration_secs(true, PANEL_COLOR_SECS), 0.0);
    }

    #[test]
    fn toggle_gate_ignores_clicks_until_cooldown() {
        let mut gate = ToggleGate::default();
        let t0 = Instant::now();
        assert!(
            gate.take_click_at(t0),
            "the first primary click must start standby"
        );
        assert!(
            !gate.take_click_at(t0 + Duration::from_millis(10)),
            "a queued double-click must not toggle standby back off"
        );
        assert!(!gate.take_click_at(t0 + Duration::from_millis(200)));
        assert!(
            gate.take_click_at(t0 + Duration::from_millis(TOGGLE_COOLDOWN_MS)),
            "End Standby must work after a short pause without waiting for heartbeat"
        );
        let mut live = ToggleGate::default();
        assert!(live.take_click());
        assert!(
            !live.take_click(),
            "the production take_click path must share the same cooldown"
        );
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
        assert_eq!(state.section_session, t.panel_section_session());
        assert_eq!(state.language_label, t.language_menu());
        assert!(state.hotkey_hint.contains(DEFAULT_HOTKEY_LABEL));
        assert_eq!(state.help, t.help_title());
        assert_eq!(state.back, t.back());
        assert_eq!(state.help_how, t.help_how());
        assert_eq!(state.help_note_lid, t.help_note_lid());
        assert!(state.lid_awake_label.contains("best effort"));
        assert!(state.help_note_lid.contains("power"));
        assert!(state.help_step3.contains(DEFAULT_HOTKEY_LABEL));
        assert_eq!(state.help_hotkey, DEFAULT_HOTKEY_LABEL);
        assert_eq!(state.help_step3_before, t.help_step3_before());
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
        assert_eq!(state.more_settings, "设置");
        assert_eq!(state.section_session, "待命");
        assert_eq!(state.section_display, "屏幕");
        assert_eq!(state.section_lid, "合盖");
        assert_eq!(state.section_safeguards, "保护");
        assert_eq!(state.section_general, "通用");
        assert_eq!(state.language_label, "语言");
        assert!(!state.help_note_lid.contains("熄屏待命"));
        assert!(
            state.help_step3.contains("⌥⌘P，或"),
            "Chinese step 3 must not break after the hotkey: {}",
            state.help_step3
        );
    }

    #[test]
    fn panel_state_uses_grouped_macos_sections() {
        let cfg = AppConfig::default();
        let engine = Engine::new(cfg.clone());
        let state = panel_state(&cfg, &engine.view(&host()));
        let t = Tr::new(Lang::En);
        assert_eq!(state.section_session, "Session");
        assert_eq!(state.section_display, "Display");
        assert_eq!(state.section_lid, "Lid");
        assert_eq!(state.section_safeguards, "Safeguards");
        assert_eq!(state.section_general, "General");
        assert_eq!(state.language_label, "Language");
        assert_eq!(state.more_settings, "Settings");
        assert_eq!(state.sidebar_options, "Options");
        assert_eq!(state.sidebar_guide, "Guide");
        assert_eq!(state.pane_display_lead, t.pane_display_lead());
        assert_eq!(state.pane_general_lead, t.pane_general_lead());
        assert!(state.pane_lid_lead.contains("best-effort"));
        assert_eq!(state.hotkey_hint, t.panel_hotkey_hint());
        assert_eq!(state.primary_action, t.start_standby());
        assert!(state.hotkey_hint.contains("display off"));
    }
}
