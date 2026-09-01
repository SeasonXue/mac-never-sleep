mod apply;
mod cli;
mod clock;
mod foreground;
#[cfg(any(test, target_os = "macos"))]
mod icon;
mod ipc;
mod locale;
mod paths;
mod persist;
mod platform;
mod protocol;
mod util;

#[cfg(target_os = "macos")]
mod gui;
#[cfg(target_os = "macos")]
mod native_panel;
#[cfg(any(test, target_os = "macos"))]
mod panel;

use clap::Parser;
use never_sleep_core::{Lang, StopReason, Tr, LANG_ENV};

use crate::cli::{Cli, Command};
use crate::ipc::try_send;
use crate::persist::load_config;
use crate::platform::default_platform;
use crate::protocol::{IpcRequest, IpcResponse};

fn main() {
    let cli = Cli::parse();
    apply_lang_override(cli.lang.as_deref());
    let t = ui_tr();

    if cli.menubar || cli.command.is_none() {
        #[cfg(target_os = "macos")]
        {
            gui::run();
            return;
        }
        #[cfg(not(target_os = "macos"))]
        {
            if cli.command.is_none() && !cli.menubar {
                use clap::CommandFactory;
                let mut cmd = Cli::command();
                let _ = cmd.print_help();
                println!();
                return;
            }
            eprintln!("{}", t.menubar_macos_only());
            std::process::exit(1);
        }
    }

    match cli.command.unwrap() {
        Command::On { r#for, json } => cmd_on(r#for, json),
        Command::Off { json } => cmd_simple(IpcRequest::Off, json, false),
        Command::Toggle { json } => cmd_simple(IpcRequest::Toggle, json, false),
        Command::Status { json } => cmd_status(json),
        Command::Doctor => {
            let p = default_platform();
            print!("{}", p.doctor());
        }
        Command::Cleanup => {
            let p = default_platform();
            p.cleanup_orphans();
            println!("{}", t.cleanup_done());
        }
        Command::Explain => {
            println!("{}", t.onboarding());
        }
    }
}

fn apply_lang_override(raw: Option<&str>) {
    if let Some(raw) = raw {
        if Lang::parse_opt(raw).is_some() {
            std::env::set_var(LANG_ENV, raw);
        }
    }
}

fn ui_tr() -> Tr {
    Tr::new(load_config().lang())
}

fn print_resp(resp: &IpcResponse, json: bool) {
    let t = ui_tr();
    if json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap());
        return;
    }
    if !resp.ok {
        eprintln!("{}", resp.error.as_deref().unwrap_or(t.failed()));
        std::process::exit(1);
    }
    if let Some(st) = &resp.status {
        if st.active {
            println!(
                "{}",
                t.cli_status_line(
                    &st.display,
                    if st.lid == "closed" {
                        t.lid_closed()
                    } else {
                        t.lid_open()
                    },
                    if st.on_ac {
                        t.power_ac()
                    } else {
                        t.power_battery()
                    },
                    st.battery
                )
            );
        } else {
            println!("{}", t.not_in_standby());
            if let Some(code) = &st.stop_reason_code {
                if let Some(reason) = StopReason::from_code(code) {
                    println!("{}", reason.label(load_config().lang()));
                } else if let Some(r) = &st.stop_reason {
                    println!("{r}");
                }
            } else if let Some(r) = &st.stop_reason {
                println!("{r}");
            }
        }
    }
}

fn cmd_on(for_raw: Option<String>, json: bool) {
    let t = ui_tr();
    let parse_lang = if json { Lang::En } else { load_config().lang() };
    let duration = match crate::foreground::parse_optional_duration(for_raw.as_deref(), parse_lang)
    {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let req = IpcRequest::On {
        duration: for_raw.clone(),
    };
    if let Some(resp) = try_send(&req) {
        print_resp(&resp, json);
        return;
    }
    if json {
        eprintln!("{}", t.menubar_missing_foreground_json());
    }
    let mut platform = default_platform();
    if let Err(e) = crate::foreground::run_foreground(platform.as_mut(), duration) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn cmd_simple(req: IpcRequest, json: bool, _allow_local: bool) {
    if let Some(resp) = try_send(&req) {
        print_resp(&resp, json);
        return;
    }
    if !_allow_local {
        eprintln!("{}", ui_tr().menubar_not_running());
        std::process::exit(1);
    }
}

fn cmd_status(json: bool) {
    if let Some(resp) = try_send(&IpcRequest::Status) {
        print_resp(&resp, json);
        return;
    }
    let platform = default_platform();
    let host = platform.snapshot();
    let engine = never_sleep_core::Engine::new(crate::persist::load_config());
    let st = engine.json_status(&host);
    let resp = IpcResponse::ok_status(st);
    print_resp(&resp, json);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn celestial_icons_have_distinct_pixels() {
        let (moon, w, h) = icon::celestial_icon(true);
        assert_eq!((w, h), (36, 36));
        assert_eq!(moon.len(), 36 * 36 * 4);
        assert!(moon.iter().any(|&b| b != 0));
        let (sun, _, _) = icon::celestial_icon(false);
        assert_eq!(sun.len(), moon.len());
        assert_ne!(sun, moon);
        for px in [sun.as_slice(), moon.as_slice()] {
            for pixel in px.chunks_exact(4) {
                assert_eq!(
                    &pixel[..3],
                    &[0, 0, 0],
                    "menu-bar template pixels must be black"
                );
            }
        }
        assert!(sun.chunks_exact(4).any(|p| p[3] > 0));
        assert!(moon.chunks_exact(4).any(|p| p[3] > 0));
    }

    #[test]
    fn native_panel_uses_liquid_glass_with_vibrancy_fallback() {
        let src = include_str!("native_panel.rs");
        assert!(
            src.contains("NSGlassEffectView"),
            "macOS 26 Liquid Glass is the preferred panel chrome"
        );
        assert!(
            src.contains("NSVisualEffectView"),
            "older macOS must keep NSVisualEffectView vibrancy"
        );
        assert!(
            src.contains("preferred_glass"),
            "glass selection is shared with Linux-tested policy"
        );
        assert!(
            !src.contains("popover.html"),
            "the menu-bar panel must not embed HTML"
        );
        assert!(
            !src.contains("wry::"),
            "the menu-bar panel must not use WKWebView"
        );
    }

    #[test]
    fn native_panel_keeps_help_settings_and_more() {
        let src = include_str!("native_panel.rs");
        let panel = include_str!("panel.rs");
        assert!(
            src.contains("PanelView::Help"),
            "in-panel How to use view must survive native rewrite"
        );
        assert!(
            src.contains("PanelView::Settings"),
            "settings view must survive native rewrite"
        );
        assert!(
            src.contains("show_settings"),
            "Settings stays reachable from the main view"
        );
        assert!(
            panel.contains("more_settings"),
            "Settings label is driven by Tr"
        );
        assert!(
            src.contains("section_session"),
            "the sidebar labels Session from PanelState"
        );
        assert!(
            src.contains("section_display"),
            "Display stays a sidebar destination"
        );
        assert!(
            src.contains("hotkey_hint"),
            "main view must show that the hotkey works with the display off"
        );
        assert!(
            src.contains("SidebarItem::Help"),
            "How to use is a sidebar item, not a footer link"
        );
    }

    #[test]
    fn native_panel_follows_utility_sidebar_detail() {
        let src = include_str!("native_panel.rs");
        let gui = include_str!("gui.rs");
        assert!(
            src.contains("constraintEqualToConstant(28.0)"),
            "the sun/moon is a compact status glyph, not a hero coin"
        );
        assert!(
            !src.contains("constraintEqualToConstant(88.0)"),
            "the 88pt hero coin is not a utility panel"
        );
        assert!(
            src.contains("pin_split"),
            "the panel is a sidebar plus detail split"
        );
        assert!(
            src.contains("SIDEBAR_WIDTH"),
            "sidebar width is the shared token"
        );
        assert!(
            src.contains("pin_detail_content"),
            "detail copy is leading-pinned, not trailing-hugging"
        );
        assert!(
            src.contains("imageWithSystemSymbolName"),
            "sidebar uses SF Symbols"
        );
        assert!(
            src.contains("NSVisualEffectMaterial::Sidebar"),
            "older macOS uses Sidebar vibrancy, not a card wall"
        );
        assert!(
            src.contains("section_header"),
            "sidebar groups use small caps-style headers"
        );
        assert!(
            !src.contains("grouped_card") && !src.contains("settings_card"),
            "do not rebuild the iOS settings card wall"
        );
        assert!(
            src.contains("windowBackgroundColor"),
            "the detail column is opaque so copy stays readable"
        );
        assert!(
            !src.contains("quit_main"),
            "Quit lives in the status-item menu, not the panel chrome"
        );
        assert!(
            !src.contains("quit_settings"),
            "nested Settings must not repeat Quit (HIG)"
        );
        assert!(
            src.contains("show_help"),
            "How to use stays reachable from the panel"
        );
        assert!(
            !src.contains("set_toggle_armed"),
            "End Standby must stay enabled; do not gray out the primary button"
        );
        assert!(
            src.contains("NSBezelStyle::Push"),
            "the primary action is a standard push button, not Glass (which looks disabled)"
        );
        assert!(
            !src.contains("setFillColor(Some("),
            "NSBox::setFillColor takes &NSColor, not Option"
        );
        assert!(
            gui.contains("UTILITY_WIDTH"),
            "the utility panel uses the shared 640pt token"
        );
        assert!(
            gui.contains("with_resizable(true)"),
            "the utility panel allows limited resize"
        );
        assert!(
            gui.contains("with_decorations(true)"),
            "the panel is a normal titled window"
        );
        assert!(
            !gui.contains("with_always_on_top(true)"),
            "a normal window is not pinned to the menu bar"
        );
        assert!(
            !gui.contains("toggle_at"),
            "do not anchor the window to the status item"
        );
        assert!(
            !gui.contains("Focused(false)"),
            "losing key status must not dismiss a real window"
        );
        assert!(
            gui.contains("panel_placement"),
            "placement policy is shared with Linux-tested panel.rs"
        );
        assert!(
            gui.contains("SelectPane"),
            "sidebar clicks select a pane through UiCommand"
        );
        assert!(
            gui.contains("show_window"),
            "the status-item menu can reopen the utility panel"
        );
        assert!(
            !src.contains("let mut panel"),
            "unused mut on NativePanel::attach fails macOS clippy -D warnings"
        );
        assert!(
            gui.contains("handles.show_window.id()"),
            "Show Window must take the panel by move, not reborrow a non-mut parameter"
        );
        assert!(
            !gui.contains("popover.as_mut()\n            panel.show()"),
            "handle_menu_event must not as_mut a by-value Option<&mut Popover>"
        );
    }

    #[test]
    fn menu_bar_shell_does_not_use_webview() {
        let gui = include_str!("gui.rs");
        assert!(!gui.contains("wry"), "gui.rs must not depend on wry");
        assert!(!gui.contains("WebView"), "gui.rs must not host a WebView");
        assert!(
            gui.contains("native_panel::NativePanel"),
            "the popover is an AppKit view tree"
        );
    }

    #[test]
    fn native_panel_enables_appkit_text_alignment() {
        let cargo = include_str!("../Cargo.toml");
        let src = include_str!("native_panel.rs");
        assert!(
            src.contains("NSTextAlignment"),
            "labels use NSTextAlignment for left/center copy"
        );
        assert!(
            cargo.contains("\"NSText\""),
            "NSTextAlignment and setAlignment need the objc2-app-kit NSText feature"
        );
    }

    #[test]
    fn native_panel_is_a_crate_module_not_nested_under_gui() {
        let main = include_str!("main.rs");
        let gui = include_str!("gui.rs");
        assert!(
            main.contains("mod native_panel;"),
            "native_panel.rs lives next to gui.rs so macOS can compile it"
        );
        assert!(
            !gui.contains("mod native_panel;"),
            "a submodule inside gui.rs would look for src/gui/native_panel.rs"
        );
    }

    #[test]
    fn native_panel_lid_copy_keeps_best_effort() {
        let en = Tr::new(Lang::En).lid_awake();
        let zh = Tr::new(Lang::Zh).lid_awake();
        assert!(
            en.contains("best effort"),
            "runtime English lid copy must keep the best-effort qualifier: {en}"
        );
        let panel = include_str!("panel.rs");
        assert!(
            panel.contains("lid_awake_label"),
            "native panel state must carry lid copy from Tr"
        );
        assert!(zh.contains("合盖") || zh.contains("尽力"));
    }

    #[test]
    fn panel_copy_matches_runtime_actions_and_product_name() {
        let en = Tr::new(Lang::En);
        let zh = Tr::new(Lang::Zh);
        assert_eq!(en.start_standby(), "Start Screen-Off Standby");
        assert_eq!(zh.start_standby(), "开始关屏待命");
        assert_eq!(en.panel_idle_title(), "Not Active");
        assert_eq!(zh.panel_active_title(), "关屏待命中");
        assert!(!zh.help_step1_detail().contains("熄屏待命"));
        assert!(!zh.panel_summary_active().contains("熄屏待命"));
    }

    #[test]
    fn finder_display_name_is_never_sleep_in_both_localizations() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packaging");
        for rel in [
            "en.lproj/InfoPlist.strings",
            "zh-Hans.lproj/InfoPlist.strings",
        ] {
            let text = fs::read_to_string(root.join(rel))
                .unwrap_or_else(|err| panic!("missing {}: {err}", root.join(rel).display()));
            assert!(
                text.contains("CFBundleDisplayName = \"Never Sleep\";"),
                "{rel} must show Never Sleep in Finder"
            );
            assert!(
                !text.contains("熄屏待命"),
                "{rel} must not use the old Chinese product name"
            );
        }
    }

    #[test]
    fn help_body_keeps_a_pointer_scroll_affordance() {
        let src = include_str!("native_panel.rs");
        assert!(
            src.contains("NSScrollView"),
            "help and settings copy can exceed the panel; native AppKit must keep a scroll view"
        );
        assert!(
            src.contains("setHasVerticalScroller(true)"),
            "help needs a visible vertical scroller for pointer users"
        );
        assert!(
            !src.contains("setHasVerticalScroller(false)"),
            "do not hide the help scrollbar"
        );
        assert!(
            src.contains("pin_document_width"),
            "the help document must be width-constrained to the clip view"
        );
    }

    #[test]
    fn settings_rows_fill_width_and_wrap_long_captions() {
        let src = include_str!("native_panel.rs");
        assert!(
            src.contains("NSLayoutAttribute::Width")
                || src.contains("constraintEqualToConstant(DETAIL_MAX_WIDTH"),
            "detail rows size to the Notes-like content column"
        );
        assert!(
            src.contains("row_caption"),
            "switch captions wrap instead of pushing the control off-screen"
        );
        assert!(
            src.contains("NSStackViewDistribution::Fill"),
            "rows give leftover width to the caption, not the switch"
        );
        assert!(
            src.contains("NSTextAlignment::Left"),
            "detail copy is leading-aligned, not trailing"
        );
    }

    #[test]
    fn gui_ignores_queued_toggle_clicks_until_refresh() {
        let gui = include_str!("gui.rs");
        assert!(
            gui.contains("ToggleGate"),
            "native toggle buttons need the same in-flight guard the WebView had"
        );
        assert!(
            gui.contains("take_click()"),
            "double-click must not emit a second Input::Toggle before refresh"
        );
        assert!(
            !gui.contains("set_toggle_armed"),
            "the primary button stays enabled so End Standby is never grayed out"
        );
    }
}
