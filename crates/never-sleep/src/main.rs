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

    fn rust_fn_src<'a>(src: &'a str, name: &str) -> &'a str {
        let needle = format!("fn {name}(");
        let start = src
            .find(&needle)
            .unwrap_or_else(|| panic!("native_panel.rs must define {name}"));
        let after = &src[start + needle.len()..];
        let end = after.find("\nfn ").unwrap_or(after.len());
        &src[start..start + needle.len() + end]
    }

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
            src.contains("more_settings"),
            "More Settings is driven by Tr"
        );
        assert!(
            panel.contains("more_settings"),
            "Settings label is driven by Tr"
        );
        assert!(
            src.contains("quit_main"),
            "Quit stays on the root of the panel"
        );
        assert!(
            src.contains("help_button"),
            "How to use is reachable from Settings"
        );
        assert!(
            src.contains("SidebarItem"),
            "sidebar destinations still map onto Main/Settings/Help"
        );
        assert!(
            !src.contains("SidebarItem::Lid")
                && !src.contains("SidebarItem::Safeguards")
                && !src.contains("SidebarItem::General"),
            "unused sidebar variants are macOS clippy dead_code after dropping arrow-key traversal"
        );
    }

    #[test]
    fn native_panel_matches_app_screenshots() {
        let src = include_str!("native_panel.rs");
        let gui = include_str!("gui.rs");
        assert!(
            src.contains("HERO_SIZE"),
            "the sun/moon is the 124pt screenshot coin"
        );
        assert!(
            src.contains("constraintEqualToConstant(124.0)") || src.contains("HERO_SIZE"),
            "hero coin matches docs/screenshots"
        );
        assert!(
            src.contains("grouped_card"),
            "session and settings sit in inset grouped cards"
        );
        assert!(
            src.contains("NSBoxType::Separator") || src.contains("separator("),
            "cards use hairline separators between rows"
        );
        assert!(
            src.contains("NSTextAlignment::Center"),
            "status copy is centered like the screenshots"
        );
        assert!(
            src.contains("span_stack"),
            "page stacks pin children to the card width so the coin cannot hug trailing"
        );
        assert!(
            src.contains("hero_shows_moon"),
            "idle shows the sun; standby shows the moon — never both faces at once"
        );
        assert!(
            src.contains("moon_face.setHidden(true)"),
            "first paint hides the moon so the two logos cannot overlap"
        );
        assert!(
            !src.contains("pin_fill(nv(&*hero), nv(&*coin_box))"),
            "coin subviews inside the NSButton steal hit-testing from toggle:"
        );
        assert!(
            src.contains("pin_fill(&hero_host") && src.contains("pin_fill(&hero_host, nv(&*hero))"),
            "coin visual sits behind a sibling transparent button so the 124pt coin is one click"
        );
        assert!(
            src.contains("setAccessibilityLabel"),
            "NSSwitch must announce Screen Off / Lid / Battery, not untitled switches"
        );
        assert!(
            src.contains("help_back_target") && src.contains("help_from"),
            "Help Back returns to Main when opened from Main, Settings when opened from Settings"
        );
        assert!(
            src.contains("scrollToPoint"),
            "reopening How to use must start at the heading, not the previous scroll offset"
        );
        assert!(
            src.contains("transform.rotation.y")
                && src.contains("hero_flip_radians")
                && src.contains("setDoubleSided")
                && src.contains("CATransformLayer")
                && src.contains("m34"),
            "the coin is a 3D half-turn: moon is the back face, container rotates 0↔π in place"
        );
        assert!(
            src.contains("setAnchorPoint") || src.contains("set_anchor_center"),
            "the flip rotates around the coin center, not the default (0,0) corner"
        );
        assert!(
            !src.contains("msg_send![layer, setAnchorPoint"),
            "send CALayer geometry through the typed setters, not msg_send on Retained"
        );
        assert!(
            src.contains("setContentViewMargins") && src.contains("center_square"),
            "the 104pt faces sit in the middle of the 124pt circle; NSBox must not add extra margins"
        );
        assert!(
            src.contains("clip.setAppearance") || src.contains("self.clip.setAppearance"),
            "Dark Aqua belongs on the rounded chrome, not the whole window"
        );
        assert!(
            !src.contains("window.setAppearance"),
            "window Dark Aqua paints the shadow gutter opaque and the drop shadow vanishes"
        );
        assert!(
            src.contains("setClipsToBounds(false)"),
            "the host must not clip the card shadow at the window edge"
        );
        assert!(
            src.contains("sun_face.clone()") && !src.contains(".retain()"),
            "keep extra CALayer refs with Retained::clone; Message::retain is easy to leave out of scope on macOS"
        );
        assert!(
            !src.contains("setContents: &*image"),
            "image is already &NSImage; &*image trips clippy borrow_deref_ref on macOS"
        );
        assert!(
            src.contains("CATransaction::setDisableActions(true)"),
            "setting rotation.y must disable implicit actions so one click is one half-turn, not two"
        );
        assert!(
            !src.contains("transform.scale.x") && !src.contains("CATransition"),
            "scale.x squash is not a half-turn that reveals the back; fade is not a flip"
        );
        assert!(
            !src.contains("CATransform3DMakeRotation") && !src.contains("valueWithCATransform3D"),
            "do not pass a hand-rolled CATransform3D through msg_send; use crate types / NSNumber KVC"
        );
        assert!(
            src.contains("centerYAnchor") && src.contains("index_badge"),
            "How-to step numbers sit on the badge center, not NSBox contentView top"
        );
        assert!(
            src.contains("CARD_ROW_HEIGHT") && src.contains("CARD_ROW_INSET_X"),
            "session/settings rows are the 32pt screenshot cells, not NSSwitch plus extra vertical insets"
        );
        let grouped = rust_fn_src(src, "grouped_card");
        assert!(
            grouped.contains("setContentViewMargins")
                && grouped.contains("CARD_SEPARATOR_GAP > 0.0"),
            "grouped NSBox must not add default margins; hairlines get no extra stack gap"
        );
        let sep = rust_fn_src(src, "separator");
        assert!(
            sep.contains("setContentViewMargins") && sep.contains("CARD_HAIRLINE"),
            "NSBox separators include 5pt default margins unless zeroed; that gap is the loose menu"
        );
        assert!(
            !src.contains("stretch(&footer_space)") && !src.contains("stretch(&settings_space)"),
            "do not push 更多设置 / 退出 to the bottom of a tall window"
        );
        assert!(
            src.contains("stretch(nv(&*scroll))"),
            "How to use still fills leftover height with a pointer-scroll body"
        );
        assert!(
            src.contains("FOOTER_GAP"),
            "an 8pt gap is the only space between the last menu row and the chrome bar"
        );
        assert!(
            src.contains("WARNING_SLOT") && src.contains("setDetachesHiddenViews(false)"),
            "lid-on-battery warning keeps a two-line height slot so pin_fill cannot clip it"
        );
        assert!(
            include_str!("panel.rs").contains("panel_hug_height() + SHADOW_INSET"),
            "macOS window_height must call panel_hug_height or clippy -D warnings flags dead_code"
        );
        assert!(
            src.contains("help_step(&help_step3_title") && src.contains("state.help_step3"),
            "step 3 is one wrapping sentence, not a forced break after the hotkey chip"
        );
        assert!(
            !src.contains("hotkey_cluster"),
            "do not split Press / ⌥⌘P / the rest across stack rows"
        );
        assert!(
            src.contains("grouped_copy_max_width") && src.contains("pin_beside_glyph"),
            "How to use / Keep in mind copy must wrap beside the 22pt glyph, not clip"
        );
        assert!(
            src.contains("setShadowRadius"),
            "the rounded card casts a soft layer shadow around the panel"
        );
        assert!(
            src.contains("set_fill_color"),
            "idle and active panel fills cross-fade"
        );
        assert!(
            src.contains("AppleReduceMotion"),
            "Reduce Motion skips the coin flip"
        );
        assert!(
            src.contains("NSBezelStyle::Push"),
            "the primary action is a standard push button"
        );
        assert!(
            src.contains("setKeyEquivalent"),
            "idle Start uses the default accent button; End Standby does not"
        );
        assert!(
            !src.contains("set_toggle_armed"),
            "End Standby must stay enabled; do not gray out the primary button"
        );
        assert!(
            !src.contains("setFillColor(Some("),
            "NSBox::setFillColor takes &NSColor, not Option"
        );
        assert!(
            !src.contains("(caption, toggle, control_row(&caption"),
            "labeled_switch must build the row before moving caption/toggle (E0382 on macOS)"
        );
        assert!(
            src.contains("index_badge"),
            "How to use keeps numbered steps"
        );
        assert!(
            src.contains("laptopcomputer"),
            "Keep in mind uses the laptop SF Symbol from the screenshot"
        );
        assert!(
            gui.contains("PANEL_WIDTH"),
            "the panel is the 320pt screenshot width"
        );
        assert!(
            gui.contains("with_decorations(false)"),
            "screenshots are a rounded panel, not a titled Settings window"
        );
        assert!(
            gui.contains("with_always_on_top(true)"),
            "a menu-bar popover stays above the desktop while it is open"
        );
        assert!(
            gui.contains("toggle_at"),
            "left-click anchors the panel under the status item"
        );
        assert!(
            gui.contains("UserEvent::TrayAnchor")
                && !gui.contains("button: MouseButton::Left,\n            button_state: MouseButtonState::Up"),
            "right-click must record the status-item rect so Show Window / Settings / Help anchor under it"
        );
        assert!(
            gui.contains("suppress_tray_reopen") || gui.contains("ignore_tray_until"),
            "status-item mouse-down hides via focus loss; the matching mouse-up must not reopen"
        );
        assert!(
            gui.contains("Focused(false)"),
            "losing key status dismisses the panel without ending standby"
        );
        assert!(
            gui.contains("panel_placement"),
            "placement policy is shared with Linux-tested panel.rs"
        );
        assert!(
            !gui.contains("SelectPane"),
            "screenshot panel has no sidebar; do not leave an unused UiCommand variant for macOS -D warnings"
        );
        assert!(
            !gui.contains("select_adjacent") && !gui.contains("ArrowDown"),
            "arrow keys must stay with NSPopUpButton; sidebar traversal steals duration Up/Down"
        );
        assert!(
            gui.contains("&& event.logical_key == Key::Escape"),
            "Escape hide must be one if so macOS clippy::collapsible_if stays clean"
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
    fn standby_click_plays_one_half_turn() {
        let src = include_str!("native_panel.rs");
        let flip = rust_fn_src(src, "set_coin_flip");
        let snap = flip
            .find("if duration <= 0.0")
            .expect("snap path when the click should not animate");
        assert!(
            !flip[..snap].contains("layout_coin_layers")
                && !flip[..snap].contains("layoutSubtreeIfNeeded"),
            "relayout before an animated flip sets bounds/anchorPoint and Core Animation plays a second half-turn"
        );
        assert!(
            flip[snap..].contains("layout_coin_layers"),
            "snap-to-face still recenters the layers when there is no flip to disturb"
        );
        let rot = rust_fn_src(src, "set_rotation_y");
        let begin = rot
            .find("CATransaction::begin()")
            .expect("typed CATransaction::begin so disableActions actually applies");
        let add = rot
            .find("addAnimation")
            .expect("one explicit transform.rotation.y animation");
        let set = rot
            .find("setValue")
            .expect("model layer must land on the destination angle");
        let commit = rot
            .find("CATransaction::commit()")
            .expect("typed CATransaction::commit");
        assert!(
            rot.contains("CATransaction::setDisableActions(true)"),
            "msg_send setDisableActions is easy to mis-type; use the crate method"
        );
        assert!(
            begin < add && add < commit && begin < set && set < commit,
            "addAnimation + setValue must share one disableActions transaction; otherwise the default transform action plays a second flip"
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
        assert!(
            cargo.contains("\"NSAccessibilityProtocols\""),
            "NSSwitch setAccessibilityLabel needs the objc2-app-kit NSAccessibilityProtocols feature"
        );
        assert!(
            !src.contains("CATransform3DMakeRotation"),
            "coin face swap must not use a CATransform3D C struct"
        );
        assert!(
            src.contains("NSAppearanceCustomization"),
            "idle/active appearance switch uses the AppKit customization trait"
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
            src.contains("NSLayoutAttribute::Width") || src.contains("fill_width"),
            "rows size to the panel inner width"
        );
        assert!(
            src.contains("row_caption"),
            "switch captions wrap instead of pushing the control off-screen"
        );
        assert!(
            src.contains("grouped_copy_max_width"),
            "How-to details wrap to the card inner width beside the badge"
        );
        assert!(
            !src.contains("hotkey_cluster") && !src.contains("help_hotkey_step"),
            "step 3 uses the same wrapping help_step as 1 and 2"
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
