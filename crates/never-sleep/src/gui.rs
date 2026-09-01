use std::sync::mpsc;
use std::time::{Duration, Instant};

use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use never_sleep_core::{
    AppConfig, DurationPref, Engine, Input, Lang, StopReason, Tr, DEFAULT_BATTERY_FLOOR,
    DEFAULT_HOTKEY_LABEL, HEARTBEAT_MS,
};
use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{ElementState, Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy};
use tao::keyboard::Key;
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS, WindowBuilderExtMacOS};
use tao::window::{Window, WindowBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{
    MouseButton, MouseButtonState, Rect as TrayRect, TrayIcon, TrayIconBuilder, TrayIconEvent,
};

use crate::apply::{dispatch, stop_for_quit};
use crate::icon::tray_icon;
use crate::ipc::{self, IpcIncoming};
use crate::panel::{panel_state, PanelState};
use crate::persist::{load_config, save_config};
use crate::platform::{default_platform, Platform};
use crate::protocol::{IpcRequest, IpcResponse};

mod native_panel;

enum UserEvent {
    Hotkey,
    Menu(tray_icon::menu::MenuId),
    Tray(TrayRect),
    Ui(UiCommand),
}

#[derive(Debug)]
enum UiCommand {
    Toggle,
    SetDuration { value: String },
    SetOption { key: String, enabled: bool },
    SetLanguage { language: String },
    Help,
    More,
    Back,
    Quit,
}

const POPOVER_WIDTH: f64 = 320.0;
const POPOVER_HEIGHT: f64 = 480.0;

struct Popover {
    window: Window,
    ui: native_panel::NativePanel,
    visible: bool,
    last: Option<PanelState>,
}

impl Popover {
    fn build(
        event_loop: &EventLoop<UserEvent>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> Result<Self, String> {
        let window = WindowBuilder::new()
            .with_title("Never Sleep")
            .with_inner_size(LogicalSize::new(POPOVER_WIDTH, POPOVER_HEIGHT))
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_visible(false)
            .with_always_on_top(true)
            .with_has_shadow(false)
            .with_movable_by_window_background(false)
            .build(event_loop)
            .map_err(|e| format!("popover window: {e}"))?;

        let ui = native_panel::NativePanel::attach(&window, proxy)?;

        Ok(Self {
            window,
            ui,
            visible: false,
            last: None,
        })
    }

    fn toggle_at(&mut self, rect: TrayRect) {
        if self.visible {
            self.hide();
            return;
        }

        let scale = self.window.scale_factor();
        let width = POPOVER_WIDTH * scale;
        let anchor_x = rect.position.x + f64::from(rect.size.width) / 2.0;
        let desired_x = anchor_x - width / 2.0;
        let x = self
            .window
            .available_monitors()
            .find(|monitor| {
                let position = monitor.position();
                let size = monitor.size();
                anchor_x >= f64::from(position.x)
                    && anchor_x <= f64::from(position.x) + f64::from(size.width)
                    && rect.position.y >= f64::from(position.y)
                    && rect.position.y <= f64::from(position.y) + f64::from(size.height)
            })
            .map(|monitor| {
                let position = monitor.position();
                let size = monitor.size();
                let margin = 8.0 * scale;
                let min_x = f64::from(position.x) + margin;
                let max_x = f64::from(position.x) + f64::from(size.width) - width - margin;
                desired_x.clamp(min_x, max_x.max(min_x))
            })
            .unwrap_or(desired_x);
        let y = rect.position.y + f64::from(rect.size.height) + 4.0 * scale;
        self.window
            .set_outer_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
        self.window.set_visible(true);
        self.window.set_focus();
        self.visible = true;
    }

    fn hide(&mut self) {
        self.window.set_visible(false);
        self.visible = false;
    }

    fn update(&mut self, state: PanelState) {
        if self.last.as_ref() == Some(&state) {
            return;
        }
        self.ui.apply(&state);
        self.last = Some(state);
    }
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
    lang_en: CheckMenuItem,
    lang_zh: CheckMenuItem,
    help: MenuItem,
    quit: MenuItem,
    dur_root: Submenu,
    lang_root: Submenu,
}

pub fn run() {
    let mut platform = default_platform();
    platform.cleanup_orphans();
    let mut engine = Engine::new(load_config());

    let (ipc_tx, ipc_rx) = mpsc::channel::<IpcIncoming>();
    match ipc::spawn_server(ipc_tx) {
        Err(e) if e == "already_running" => {
            eprintln!("{}", load_config().tr().already_running());
            return;
        }
        Err(e) => eprintln!("{}", load_config().tr().ipc_not_started(&e)),
        Ok(()) => {}
    }

    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);
    let proxy_menu = event_loop.create_proxy();
    let proxy_hk = proxy_menu.clone();
    let proxy_tray = proxy_menu.clone();

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = proxy_menu.send_event(UserEvent::Menu(event.id));
    }));
    GlobalHotKeyEvent::set_event_handler(Some(move |event: GlobalHotKeyEvent| {
        if event.state() == HotKeyState::Pressed {
            let _ = proxy_hk.send_event(UserEvent::Hotkey);
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            rect,
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            let _ = proxy_tray.send_event(UserEvent::Tray(rect));
        }
    }));

    let hotkeys = GlobalHotKeyManager::new().ok();
    if let Some(ref mgr) = hotkeys {
        let hk = HotKey::new(Some(Modifiers::ALT | Modifiers::SUPER), Code::KeyP);
        if mgr.register(hk).is_err() {
            eprintln!("{}", load_config().tr().hotkey_failed(DEFAULT_HOTKEY_LABEL));
        }
    }
    let _hotkeys = hotkeys;

    let menu = Menu::new();
    let handles = build_menu(&menu, &engine.config);
    let mut popover = match Popover::build(&event_loop, event_loop.create_proxy()) {
        Ok(popover) => Some(popover),
        Err(error) => {
            eprintln!("{error}");
            None
        }
    };
    let mut tray: Option<TrayIcon> = None;
    let mut tray_active: Option<bool> = None;
    let mut last_tick = Instant::now();
    let mut shown_onboarding = engine.config.onboarding_done;

    event_loop.run(move |event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(HEARTBEAT_MS));

        while let Ok(incoming) = ipc_rx.try_recv() {
            handle_ipc(&mut engine, platform.as_mut(), incoming);
            refresh_ui(
                &handles,
                &mut tray,
                &mut tray_active,
                &mut popover,
                &engine,
                platform.as_mut(),
            );
        }

        match event {
            Event::NewEvents(StartCause::Init) => {
                let icon = tray_icon(false);
                tray = TrayIconBuilder::new()
                    .with_menu(Box::new(menu.clone()))
                    .with_menu_on_left_click(popover.is_none())
                    .with_tooltip(engine.config.tr().app_display_name())
                    .with_icon(icon)
                    .with_icon_as_template(true)
                    .build()
                    .ok();
                if !shown_onboarding {
                    shown_onboarding = true;
                    engine.config.onboarding_done = true;
                    save_config(&engine.config);
                    let t = engine.config.tr();
                    show_dialog(&t, t.welcome_title(), t.onboarding());
                }
                refresh_ui(
                    &handles,
                    &mut tray,
                    &mut tray_active,
                    &mut popover,
                    &engine,
                    platform.as_mut(),
                );
            }
            Event::NewEvents(StartCause::ResumeTimeReached { .. }) => {
                if last_tick.elapsed() >= Duration::from_millis(400) {
                    last_tick = Instant::now();
                    dispatch(&mut engine, platform.as_mut(), Input::Tick);
                    refresh_ui(
                        &handles,
                        &mut tray,
                        &mut tray_active,
                        &mut popover,
                        &engine,
                        platform.as_mut(),
                    );
                }
            }
            Event::UserEvent(UserEvent::Menu(id)) => {
                handle_menu_event(
                    &mut engine,
                    platform.as_mut(),
                    &handles,
                    control_flow,
                    id,
                    popover.as_mut(),
                );
                refresh_ui(
                    &handles,
                    &mut tray,
                    &mut tray_active,
                    &mut popover,
                    &engine,
                    platform.as_mut(),
                );
            }
            Event::UserEvent(UserEvent::Hotkey) => {
                dispatch(&mut engine, platform.as_mut(), Input::Toggle);
                refresh_ui(
                    &handles,
                    &mut tray,
                    &mut tray_active,
                    &mut popover,
                    &engine,
                    platform.as_mut(),
                );
            }
            Event::UserEvent(UserEvent::Tray(rect)) => {
                refresh_ui(
                    &handles,
                    &mut tray,
                    &mut tray_active,
                    &mut popover,
                    &engine,
                    platform.as_mut(),
                );
                if let Some(panel) = popover.as_mut() {
                    panel.toggle_at(rect);
                }
            }
            Event::UserEvent(UserEvent::Ui(command)) => {
                handle_ui_command(
                    command,
                    &mut engine,
                    platform.as_mut(),
                    &handles,
                    popover.as_mut(),
                    control_flow,
                );
                refresh_ui(
                    &handles,
                    &mut tray,
                    &mut tray_active,
                    &mut popover,
                    &engine,
                    platform.as_mut(),
                );
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::Focused(false) | WindowEvent::CloseRequested,
                ..
            } => {
                if let Some(popover) = popover.as_mut() {
                    if popover.window.id() == window_id {
                        popover.hide();
                    }
                }
            }
            Event::WindowEvent {
                window_id,
                event: WindowEvent::KeyboardInput { event, .. },
                ..
            } => {
                if event.state == ElementState::Pressed && event.logical_key == Key::Escape {
                    if let Some(popover) = popover.as_mut() {
                        if popover.window.id() == window_id {
                            popover.hide();
                        }
                    }
                }
            }
            Event::LoopDestroyed => {
                stop_for_quit(&mut engine, platform.as_mut());
            }
            _ => {}
        }
    });
}

fn build_menu(menu: &Menu, cfg: &AppConfig) -> MenuHandles {
    let t = cfg.tr();
    let status = MenuItem::new(t.idle_status(), false, None);
    let detail = MenuItem::new(t.will_sleep_display(), false, None);
    let warn = MenuItem::new(" ", false, None);
    let toggle = MenuItem::new(t.start_standby(), true, None);

    let dur_inf = CheckMenuItem::new(
        t.indefinite(),
        true,
        matches!(cfg.duration, DurationPref::Indefinite),
        None,
    );
    let dur_1h = CheckMenuItem::new(
        t.hours(1),
        true,
        matches!(cfg.duration, DurationPref::Hours { hours: 1 }),
        None,
    );
    let dur_3h = CheckMenuItem::new(
        t.hours(3),
        true,
        matches!(cfg.duration, DurationPref::Hours { hours: 3 }),
        None,
    );
    let dur_8h = CheckMenuItem::new(
        t.hours(8),
        true,
        matches!(cfg.duration, DurationPref::Hours { hours: 8 }),
        None,
    );
    let dur_until = CheckMenuItem::new(
        t.until_clock(8, 0),
        true,
        matches!(
            cfg.duration,
            DurationPref::UntilLocal { hour: 8, minute: 0 }
        ),
        None,
    );
    let dur_root = Submenu::with_items(
        t.duration_menu(),
        true,
        &[&dur_inf, &dur_1h, &dur_3h, &dur_8h, &dur_until],
    )
    .expect("duration submenu");

    let screen_off = CheckMenuItem::new(t.screen_off_now(), true, cfg.screen_off, None);
    let lid_awake = CheckMenuItem::new(t.lid_awake(), true, cfg.keep_awake_on_lid_close, None);
    let resleep = CheckMenuItem::new(t.resleep_display(), true, cfg.resleep_display, None);
    let lock_screen = CheckMenuItem::new(t.lock_screen(), true, cfg.lock_screen, None);
    let battery_floor = CheckMenuItem::new(
        t.battery_floor_on(DEFAULT_BATTERY_FLOOR),
        true,
        cfg.battery_floor_percent.is_some(),
        None,
    );
    let login = CheckMenuItem::new(t.launch_at_login(), true, cfg.launch_at_login, None);
    let lang_en = CheckMenuItem::new(t.language_english(), true, cfg.lang() == Lang::En, None);
    let lang_zh = CheckMenuItem::new(t.language_chinese(), true, cfg.lang() == Lang::Zh, None);
    let lang_root = Submenu::with_items(t.language_menu(), true, &[&lang_en, &lang_zh])
        .expect("language submenu");
    let help = MenuItem::new(t.help_title(), true, None);
    let quit = MenuItem::new(t.quit(), true, None);

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
        &lang_root,
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
        lang_en,
        lang_zh,
        help,
        quit,
        dur_root,
        lang_root,
    }
}

fn apply_static_labels(handles: &MenuHandles, lang: Lang) {
    let t = Tr::new(lang);
    handles.dur_inf.set_text(t.indefinite());
    handles.dur_1h.set_text(t.hours(1));
    handles.dur_3h.set_text(t.hours(3));
    handles.dur_8h.set_text(t.hours(8));
    handles.dur_until.set_text(t.until_clock(8, 0));
    handles.dur_root.set_text(t.duration_menu());
    handles.screen_off.set_text(t.screen_off_now());
    handles.lid_awake.set_text(t.lid_awake());
    handles.resleep.set_text(t.resleep_display());
    handles.lock_screen.set_text(t.lock_screen());
    handles
        .battery_floor
        .set_text(t.battery_floor_on(DEFAULT_BATTERY_FLOOR));
    handles.login.set_text(t.launch_at_login());
    handles.lang_root.set_text(t.language_menu());
    handles.help.set_text(t.help_title());
    handles.quit.set_text(t.quit());
}

fn refresh_ui(
    handles: &MenuHandles,
    tray: &mut Option<TrayIcon>,
    tray_active: &mut Option<bool>,
    popover: &mut Option<Popover>,
    engine: &Engine,
    platform: &mut dyn Platform,
) {
    let host = platform.snapshot();
    let vm = engine.view(&host);
    let next = popover.as_ref().map(|_| panel_state(&engine.config, &vm));
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
        .lang_en
        .set_checked(engine.config.lang() == Lang::En);
    handles
        .lang_zh
        .set_checked(engine.config.lang() == Lang::Zh);
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
        // set_icon() creates a fresh NSImage without isTemplate. Re-apply the
        // template flag so macOS can tint the glyph white on a dark menu bar.
        if *tray_active != Some(vm.active) {
            t.set_icon(Some(tray_icon(vm.active))).ok();
            t.set_icon_as_template(true);
            *tray_active = Some(vm.active);
        }
    }
    if let (Some(panel), Some(state)) = (popover.as_mut(), next) {
        panel.update(state);
    }
}

fn handle_menu_event(
    engine: &mut Engine,
    platform: &mut dyn Platform,
    handles: &MenuHandles,
    control_flow: &mut ControlFlow,
    id: tray_icon::menu::MenuId,
    popover: Option<&mut Popover>,
) {
    if id == handles.toggle.id() {
        dispatch(engine, platform, Input::Toggle);
    } else if id == handles.quit.id() {
        stop_for_quit(engine, platform);
        *control_flow = ControlFlow::Exit;
    } else if id == handles.help.id() {
        show_help(engine, popover);
    } else if id == handles.lang_en.id() {
        engine.config.language = Some(Lang::En);
        save_config(&engine.config);
        apply_static_labels(handles, Lang::En);
    } else if id == handles.lang_zh.id() {
        engine.config.language = Some(Lang::Zh);
        save_config(&engine.config);
        apply_static_labels(handles, Lang::Zh);
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
            platform.notify(engine.config.tr().login_item_title(), &e);
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

fn handle_ui_command(
    command: UiCommand,
    engine: &mut Engine,
    platform: &mut dyn Platform,
    handles: &MenuHandles,
    popover: Option<&mut Popover>,
    control_flow: &mut ControlFlow,
) {
    match command {
        UiCommand::Toggle => {
            dispatch(engine, platform, Input::Toggle);
        }
        UiCommand::SetDuration { value } => {
            let pref = match value.as_str() {
                "indefinite" => Some(DurationPref::Indefinite),
                "1h" => Some(DurationPref::Hours { hours: 1 }),
                "3h" => Some(DurationPref::Hours { hours: 3 }),
                "8h" => Some(DurationPref::Hours { hours: 8 }),
                "until_0800" => Some(DurationPref::UntilLocal { hour: 8, minute: 0 }),
                _ => None,
            };
            if let Some(pref) = pref {
                set_duration(engine, platform, pref);
            }
        }
        UiCommand::SetOption { key, enabled } => {
            match key.as_str() {
                "screen_off" => engine.config.screen_off = enabled,
                "lid_awake" => engine.config.keep_awake_on_lid_close = enabled,
                "resleep_display" => engine.config.resleep_display = enabled,
                "lock_screen" => engine.config.lock_screen = enabled,
                "battery_floor" => {
                    engine.config.battery_floor_percent = enabled.then_some(DEFAULT_BATTERY_FLOOR)
                }
                "launch_at_login" => {
                    if let Err(error) = platform.set_launch_at_login(enabled) {
                        platform.notify(engine.config.tr().login_item_title(), &error);
                    } else {
                        engine.config.launch_at_login = enabled;
                    }
                }
                _ => return,
            }
            save_config(&engine.config);
            if key == "lid_awake" && engine.is_active() {
                dispatch(engine, platform, Input::Tick);
            }
        }
        UiCommand::SetLanguage { language } => {
            let lang = match language.as_str() {
                "en" => Some(Lang::En),
                "zh" => Some(Lang::Zh),
                _ => None,
            };
            if let Some(lang) = lang {
                engine.config.language = Some(lang);
                save_config(&engine.config);
                apply_static_labels(handles, lang);
            }
        }
        UiCommand::Help => {
            show_help(engine, popover);
        }
        UiCommand::More => {
            if let Some(panel) = popover {
                panel.ui.show_settings();
            }
        }
        UiCommand::Back => {
            if let Some(panel) = popover {
                panel.ui.go_back();
            }
        }
        UiCommand::Quit => {
            stop_for_quit(engine, platform);
            *control_flow = ControlFlow::Exit;
        }
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
            let input = match crate::protocol::parse_on_duration_in(
                duration.as_deref(),
                engine.config.lang(),
            ) {
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

fn show_help(engine: &Engine, popover: Option<&mut Popover>) {
    if let Some(panel) = popover {
        if panel.visible {
            panel.ui.show_help();
            return;
        }
    }
    let t = engine.config.tr();
    show_dialog(&t, t.help_title(), t.onboarding());
}

fn show_dialog(t: &Tr, title: &str, body: &str) {
    let ok = t.dialog_ok().replace('"', "\\\"");
    let script = format!(
        "display dialog \"{}\" with title \"{}\" with icon note buttons {{\"{ok}\"}} default button 1",
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
