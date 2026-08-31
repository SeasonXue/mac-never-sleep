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
    fn popover_uses_solid_background_without_header_gear() {
        let html = include_str!("../ui/popover.html");
        assert!(
            !html.contains("backdrop-filter"),
            "popover must use a solid color, not frosted glass"
        );
        assert!(
            !html.contains("id=\"gearButton\""),
            "settings gear is not in the header"
        );
        assert!(
            html.contains("id=\"moreButton\""),
            "More Settings stays in the footer"
        );
        assert!(
            html.contains("id=\"helpView\""),
            "in-popover How to use view must survive style-only edits"
        );
        assert!(
            html.contains("--bg: #f5f5f7"),
            "idle popover uses an opaque light fill"
        );
        assert!(
            html.contains("--bg: #1c1c1e"),
            "active popover uses an opaque dark fill"
        );
    }

    #[test]
    fn popover_is_flush_without_arrow_shadow_or_app_title() {
        let html = include_str!("../ui/popover.html");
        assert!(
            !html.contains(".float::before"),
            "pointer arrow overlaps background text; flush the panel instead"
        );
        assert!(
            !html.contains("--arrow-x"),
            "arrow position is unused once the pointer is gone"
        );
        assert!(
            !html.contains("drop-shadow(0 10px 20px"),
            "outer drop-shadow around the panel must be removed"
        );
        assert!(
            !html.contains("id=\"appTitle\""),
            "Never Sleep / 熄屏待命 title chrome is redundant in the popover"
        );
        let float = css_rule(html, ".float");
        assert!(
            float.contains("inset: 0") || float.contains("inset:0"),
            "panel shell must sit flush in the window, got: {float}"
        );
        assert!(
            !float.contains("drop-shadow"),
            "panel shell must not cast an outer shadow, got: {float}"
        );
        let panel = css_rule(html, ".panel");
        assert!(
            panel.contains("inset: 0") || panel.contains("inset:0"),
            "panel must not reserve space for an arrow, got: {panel}"
        );
    }

    #[test]
    fn preview_lid_awake_matches_runtime_best_effort_copy() {
        let html = include_str!("../ui/popover.html");
        let en = Tr::new(Lang::En).lid_awake();
        let zh = Tr::new(Lang::Zh).lid_awake();
        assert!(
            en.contains("best effort"),
            "runtime English lid copy must keep the best-effort qualifier: {en}"
        );
        assert!(
            html.contains(en),
            "previewState must ship the same lid_awake string as Tr::lid_awake(), got runtime {en:?}"
        );
        assert!(
            html.contains(zh),
            "previewState must ship the same Chinese lid_awake string as Tr::lid_awake(), got runtime {zh:?}"
        );
    }

    #[test]
    fn help_body_keeps_a_pointer_scroll_affordance() {
        let html = include_str!("../ui/popover.html");
        let help_body = css_rule(html, ".help-body");
        assert!(
            !help_body.contains("scrollbar-width: none"),
            "hiding the help scrollbar leaves pointer users with no way to reach lower items: {help_body}"
        );
        assert!(
            help_body.contains("scrollbar-width: thin") || help_body.contains("overflow-y: scroll"),
            "help body needs a visible, draggable scrollbar, got: {help_body}"
        );
        assert!(
            !html.contains(".help-body::-webkit-scrollbar { width: 0"),
            "webkit scrollbar width 0 removes the only draggable thumb"
        );
    }

    fn css_rule(html: &str, selector: &str) -> String {
        let marker = format!("{selector} {{");
        let start = html
            .find(&marker)
            .unwrap_or_else(|| panic!("missing CSS rule {selector}"));
        let body = &html[start + marker.len()..];
        let end = body
            .find('}')
            .unwrap_or_else(|| panic!("unclosed CSS rule {selector}"));
        body[..end].to_string()
    }
}
