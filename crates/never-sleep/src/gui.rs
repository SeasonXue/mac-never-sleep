use std::sync::mpsc;
use std::time::{Duration, Instant};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use never_sleep_core::{
    AppConfig, DurationPref, Engine, Input, StopReason, APP_DISPLAY_NAME, DEFAULT_BATTERY_FLOOR,
    DEFAULT_HOTKEY_LABEL, HEARTBEAT_MS, ONBOARDING,
};
use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::apply::{dispatch, stop_for_quit};
use crate::icon::tray_icon;
use crate::ipc::{self, IpcIncoming};
use crate::persist::{load_config, save_config};
use crate::platform::{default_platform, Platform};
use crate::protocol::{IpcRequest, IpcResponse};

enum UserEvent {
    Tick,
    Hotkey,
    Menu(tray_icon::menu::MenuId),
}

struct MenuHandles {
    status: MenuItem,
    detail: MenuItem,
    warn: MenuItem,
    toggle: MenuItem,
    dur_inf: CheckMenuItem,
    dur_1h: CheckMenuItem,
    dur_3h: CheckMenuItem,
    dur_8h: CheckMenuItem,
    dur_until: CheckMenuItem,
    screen_off: CheckMenuItem,
    lid_awake: CheckMenuItem,
    resleep: CheckMenuItem,
    lock_screen: CheckMenuItem,
    battery_floor: CheckMenuItem,
    login: CheckMenuItem,
    help: MenuItem,
    quit: MenuItem,
}

pub fn run() {
    let mut platform = default_platform();
    platform.cleanup_orphans();
    let mut engine = Engine::new(load_config());

    let (ipc_tx, ipc_rx) = mpsc::channel::<IpcIncoming>();
    match ipc::spawn_server(ipc_tx) {
        Err(e) if e == "already_running" => {
            eprintln!("熄屏待命已在菜单栏运行。");
            return;
        }
        Err(e) => eprintln!("IPC 未启动：{e}（命令行将以前台模式工作）"),
        Ok(()) => {}
    }

    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy_menu = event_loop.create_proxy();
    let proxy_hk = proxy_menu.clone();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = proxy_menu.send_event(UserEvent::Menu(event.id));
    }));
    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        if event.state() == HotKeyState::Pressed {
            let _ = proxy_hk.send_event(UserEvent::Hotkey);
        }
    }));

    let hotkeys = GlobalHotKeyManager::new().ok();
    if let Some(ref mgr) = hotkeys {
        let hk = HotKey::new(Some(Modifiers::ALT | Modifiers::SUPER), Code::KeyP);
        if mgr.register(hk).is_err() {
            eprintln!("快捷键 {DEFAULT_HOTKEY_LABEL} 注册失败，仍可通过菜单操作。");
        }
    }
    let _hotkeys = hotkeys;

    let menu = Menu::new();
    let handles = build_menu(&menu, &engine.config);
    let mut tray: Option<TrayIcon> = None;
    let mut last_tick = Instant::now();
    let mut shown_onboarding = engine.config.onboarding_done;

    event_loop.run(move |event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(HEARTBEAT_MS));

        while let Ok(incoming) = ipc_rx.try_recv() {
            handle_ipc(&mut engine, platform.as_mut(), incoming);
            refresh_ui(&handles, &mut tray, &engine, platform.as_mut());
        }

        match event {
            Event::NewEvents(StartCause::Init) => {
                let icon = tray_icon(false);
                tray = TrayIconBuilder::new()
                    .with_menu(Box::new(menu.clone()))
                    .with_tooltip(APP_DISPLAY_NAME)
                    .with_icon(icon)
                    .with_icon_as_template(true)
                    .build()
                    .ok();
                if !shown_onboarding {
                    shown_onboarding = true;
                    engine.config.onboarding_done = true;
                    save_config(&engine.config);
                    show_dialog("欢迎使用熄屏待命", ONBOARDING);
                }
                refresh_ui(&handles, &mut tray, &engine, platform.as_mut());
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. })
            | Event::UserEvent(UserEvent::Tick) => {
                if last_tick.elapsed() >= Duration::from_millis(400) {
                    last_tick = Instant::now();
                    dispatch(&mut engine, platform.as_mut(), Input::Tick);
                    refresh_ui(&handles, &mut tray, &engine, platform.as_mut());
                }
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                handle_menu_event(&mut engine, platform.as_mut(), &handles, control_flow, id);
                refresh_ui(&handles, &mut tray, &engine, platform.as_mut());
            }
            Event::UserEvent(UserEvent::Hotkey) => {
                dispatch(&mut engine, platform.as_mut(), Input::Toggle);
                refresh_ui(&handles, &mut tray, &engine, platform.as_mut());
            }
            Event::LoopDestroyed => {
                stop_for_quit(&mut engine, platform.as_mut());
            }
            _ => {}
        }
    });
}

fn build_menu(menu: &Menu, cfg: &AppConfig) -> MenuHandles {
    let status = MenuItem::new("未待命 · 点击开始", false, None);
    let detail = MenuItem::new("将关闭屏幕、保持系统运行", false, None);
    let warn = MenuItem::new(" ", false, None);
    let toggle = MenuItem::new("开始熄屏待命", true, None);

    let dur_inf = CheckMenuItem::new(
        "无限期",
        true,
        matches!(cfg.duration, DurationPref::Indefinite),
        None,
    );
    let dur_1h = CheckMenuItem::new(
        "1 小时",
        true,
        matches!(cfg.duration, DurationPref::Hours { hours: 1 }),
        None,
    );
    let dur_3h = CheckMenuItem::new(
        "3 小时",
        true,
        matches!(cfg.duration, DurationPref::Hours { hours: 3 }),
        None,
    );
    let dur_8h = CheckMenuItem::new(
        "8 小时",
        true,
        matches!(cfg.duration, DurationPref::Hours { hours: 8 }),
        None,
    );
    let dur_until = CheckMenuItem::new(
        "到 08:00",
        true,
        matches!(
            cfg.duration,
            DurationPref::UntilLocal { hour: 8, minute: 0 }
        ),
        None,
    );
    let dur_root = Submenu::with_items(
        "时长",
        true,
        &[&dur_inf, &dur_1h, &dur_3h, &dur_8h, &dur_until],
    )
    .expect("duration submenu");

    let screen_off = CheckMenuItem::new("立即关闭屏幕", true, cfg.screen_off, None);
    let lid_awake = CheckMenuItem::new("合盖尽量保持运行", true, cfg.keep_awake_on_lid_close, None);
    let resleep = CheckMenuItem::new("人离开后自动再关屏", true, cfg.resleep_display, None);
    let lock_screen = CheckMenuItem::new(
        "关屏时锁定登录（远程 GUI 会受影响）",
        true,
        cfg.lock_screen,
        None,
    );
    let battery_floor = CheckMenuItem::new(
        &format!("电量低于 {DEFAULT_BATTERY_FLOOR}% 时结束"),
        true,
        cfg.battery_floor_percent.is_some(),
        None,
    );
    let login = CheckMenuItem::new("登录时启动", true, cfg.launch_at_login, None);
    let help = MenuItem::new("使用说明", true, None);
    let quit = MenuItem::new("退出", true, None);

    let _ = menu.append_items(&[
        &status,
        &detail,
        &warn,
        &PredefinedMenuItem::separator(),
        &toggle,
        &PredefinedMenuItem::separator(),
        &dur_root,
        &PredefinedMenuItem::separator(),
        &screen_off,
        &lid_awake,
        &resleep,
        &lock_screen,
        &battery_floor,
        &PredefinedMenuItem::separator(),
        &login,
        &help,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    MenuHandles {
        status,
        detail,
        warn,
        toggle,
        dur_inf,
        dur_1h,
        dur_3h,
        dur_8h,
        dur_until,
        screen_off,
        lid_awake,
        resleep,
        lock_screen,
        battery_floor,
        login,
        help,
        quit,
    }
}

fn refresh_ui(
    handles: &MenuHandles,
    tray: &mut Option<TrayIcon>,
    engine: &Engine,
    platform: &mut dyn Platform,
) {
    let host = platform.snapshot();
    let vm = engine.view(&host);
    handles.status.set_text(vm.status_line);
    handles.detail.set_text(vm.detail_line);
    let warn = vm.warnings.first().cloned().unwrap_or_default();
    handles
        .warn
        .set_text(if warn.is_empty() { " " } else { &warn });
    handles.toggle.set_text(vm.primary_action);
    handles.screen_off.set_checked(vm.screen_off);
    handles.lid_awake.set_checked(vm.keep_awake_on_lid_close);
    handles.resleep.set_checked(vm.resleep_display);
    handles.lock_screen.set_checked(vm.lock_screen);
    handles.login.set_checked(vm.launch_at_login);
    handles
        .battery_floor
        .set_checked(engine.config.battery_floor_percent.is_some());
    handles
        .dur_inf
        .set_checked(matches!(vm.duration, DurationPref::Indefinite));
    handles
        .dur_1h
        .set_checked(matches!(vm.duration, DurationPref::Hours { hours: 1 }));
    handles
        .dur_3h
        .set_checked(matches!(vm.duration, DurationPref::Hours { hours: 3 }));
    handles
        .dur_8h
        .set_checked(matches!(vm.duration, DurationPref::Hours { hours: 8 }));
    handles.dur_until.set_checked(matches!(
        vm.duration,
        DurationPref::UntilLocal { hour: 8, minute: 0 }
    ));
    if let Some(t) = tray.as_mut() {
        t.set_tooltip(Some(vm.tooltip)).ok();
        t.set_icon(Some(tray_icon(vm.active))).ok();
    }
}

fn handle_menu_event(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    handles: &MenuHandles,
    control_flow: &mut ControlFlow,
    id: tray_icon::menu::MenuId,
) {
    if id == handles.toggle.id() {
        dispatch(engine, platform, Input::Toggle);
    } else if id == handles.quit.id() {
        stop_for_quit(engine, platform);
        *control_flow = ControlFlow::Exit;
    } else if id == handles.help.id() {
        show_dialog("使用说明", ONBOARDING);
    } else if id == handles.screen_off.id() {
        engine.config.screen_off = !engine.config.screen_off;
        save_config(&engine.config);
    } else if id == handles.lid_awake.id() {
        engine.config.keep_awake_on_lid_close = !engine.config.keep_awake_on_lid_close;
        save_config(&engine.config);
        if engine.is_active() {
            dispatch(engine, platform, Input::Tick);
        }
    } else if id == handles.resleep.id() {
        engine.config.resleep_display = !engine.config.resleep_display;
        save_config(&engine.config);
    } else if id == handles.lock_screen.id() {
        engine.config.lock_screen = !engine.config.lock_screen;
        save_config(&engine.config);
    } else if id == handles.battery_floor.id() {
        engine.config.battery_floor_percent = if engine.config.battery_floor_percent.is_some() {
            None
        } else {
            Some(DEFAULT_BATTERY_FLOOR)
        };
        save_config(&engine.config);
    } else if id == handles.login.id() {
        engine.config.launch_at_login = !engine.config.launch_at_login;
        if let Err(e) = platform.set_launch_at_login(engine.config.launch_at_login) {
            platform.notify("登录项", &e);
            engine.config.launch_at_login = !engine.config.launch_at_login;
        }
        save_config(&engine.config);
    } else if id == handles.dur_inf.id() {
        set_duration(engine, platform, DurationPref::Indefinite);
    } else if id == handles.dur_1h.id() {
        set_duration(engine, platform, DurationPref::Hours { hours: 1 });
    } else if id == handles.dur_3h.id() {
        set_duration(engine, platform, DurationPref::Hours { hours: 3 });
    } else if id == handles.dur_8h.id() {
        set_duration(engine, platform, DurationPref::Hours { hours: 8 });
    } else if id == handles.dur_until.id() {
        set_duration(
            engine,
            platform,
            DurationPref::UntilLocal { hour: 8, minute: 0 },
        );
    }
}

fn set_duration(engine: &mut Engine, platform: &mut dyn Platform, pref: DurationPref) {
    engine.config.duration = pref;
    save_config(&engine.config);
    if engine.is_active() {
        dispatch(
            engine,
            platform,
            Input::Stop {
                reason: StopReason::User,
            },
        );
        dispatch(engine, platform, Input::StartWith(pref));
    }
}

fn handle_ipc(engine: &mut Engine, platform: &mut dyn Platform, incoming: IpcIncoming) {
    let IpcIncoming::Request { req, reply } = incoming;
    let host_status = |engine: &Engine, platform: &mut dyn Platform| {
        let host = platform.snapshot();
        engine.json_status(&host)
    };
    let resp = match req {
        IpcRequest::Ping => IpcResponse::pong(),
        IpcRequest::Status => IpcResponse::ok_status(host_status(engine, platform)),
        IpcRequest::On { duration } => {
            let input = match crate::protocol::parse_on_duration(duration.as_deref()) {
                Ok(None) => Input::Start,
                Ok(Some(d)) => Input::StartWith(d),
                Err(e) => {
                    let _ = reply.send(IpcResponse::err(e));
                    return;
                }
            };
            if engine.is_active() && matches!(input, Input::Start) {
                // already on
            } else if engine.is_active() {
                dispatch(
                    engine,
                    platform,
                    Input::Stop {
                        reason: StopReason::User,
                    },
                );
                dispatch(engine, platform, input);
            } else {
                dispatch(engine, platform, input);
            }
            IpcResponse::ok_status(host_status(engine, platform))
        }
        IpcRequest::Off => {
            if engine.is_active() {
                dispatch(
                    engine,
                    platform,
                    Input::Stop {
                        reason: StopReason::User,
                    },
                );
            }
            IpcResponse::ok_status(host_status(engine, platform))
        }
        IpcRequest::Toggle => {
            dispatch(engine, platform, Input::Toggle);
            IpcResponse::ok_status(host_status(engine, platform))
        }
        IpcRequest::Quit => {
            stop_for_quit(engine, platform);
            std::process::exit(0);
        }
    };
    let _ = reply.send(resp);
}

fn show_dialog(title: &str, body: &str) {
    let script = format!(
        "display dialog \"{}\" with title \"{}\" buttons {{\"好\"}} default button 1",
        body.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n"),
        title.replace('"', "\\\"")
    );
    let _ = std::process::Command::new("osascript")
        .arg("-e")
        .arg(script)
        .status();
}
