mod apply;
mod cli;
mod clock;
mod foreground;
mod icon;
mod ipc;
mod paths;
mod persist;
mod platform;
mod protocol;

#[cfg(target_os = "macos")]
mod gui;

use clap::Parser;
use never_sleep_core::ONBOARDING;

use crate::cli::{Cli, Command};
use crate::ipc::try_send;
use crate::platform::default_platform;
use crate::protocol::{IpcRequest, IpcResponse};

fn main() {
    let cli = Cli::parse();

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
            eprintln!("菜单栏仅支持 macOS。");
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
            println!("已尝试还原合盖睡眠标志并清除残留锁。");
        }
        Command::Explain => {
            println!("{ONBOARDING}");
        }
    }
}

fn print_resp(resp: &IpcResponse, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(resp).unwrap());
        return;
    }
    if !resp.ok {
        eprintln!("{}", resp.error.as_deref().unwrap_or("失败"));
        std::process::exit(1);
    }
    if let Some(st) = &resp.status {
        if st.active {
            println!(
                "待命中 · 屏幕 {} · {} · {}{}",
                st.display,
                if st.lid == "closed" {
                    "合盖"
                } else {
                    "开盖"
                },
                if st.on_ac {
                    "电源适配器"
                } else {
                    "电池"
                },
                st.battery
                    .map(|b| format!(" · 电量 {b}%"))
                    .unwrap_or_default()
            );
        } else {
            println!("未待命。");
            if let Some(r) = &st.stop_reason {
                println!("{r}");
            }
        }
    }
}

fn cmd_on(for_raw: Option<String>, json: bool) {
    let req = IpcRequest::On {
        duration: for_raw.clone(),
    };
    if let Some(resp) = try_send(&req) {
        print_resp(&resp, json);
        return;
    }
    if json {
        eprintln!("菜单栏未运行，以前台模式启动（JSON 状态请另开终端查询）。");
    }
    let mut platform = default_platform();
    let duration = match crate::foreground::parse_optional_duration(for_raw.as_deref()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
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
        eprintln!("菜单栏未运行。请先打开「熄屏待命」，或使用 never-sleep on 以前台方式启动。");
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
    use never_sleep_core::DurationPref;

    #[test]
    fn moon_icon_has_pixels() {
        let (px, w, h) = icon::moon_icon(true);
        assert_eq!((w, h), (32, 32));
        assert_eq!(px.len(), 32 * 32 * 4);
        assert!(px.iter().any(|&b| b != 0));
        let (idle, _, _) = icon::moon_icon(false);
        assert_eq!(idle.len(), px.len());
    }

    #[test]
    fn parse_on_duration_ok() {
        assert!(protocol::parse_on_duration(None).unwrap().is_none());
        assert!(matches!(
            protocol::parse_on_duration(Some("3h")).unwrap(),
            Some(DurationPref::Hours { hours: 3 })
        ));
    }
}
