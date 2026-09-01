//! Native AppKit menu-bar panel. macOS 26 uses Liquid Glass (`NSGlassEffectView`);
//! older releases fall back to `NSVisualEffectView` vibrancy.

use std::ops::Deref;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezelStyle, NSBorderType, NSButton, NSCellImagePosition, NSColor,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSGlassEffectView,
    NSGlassEffectViewStyle, NSImage, NSImageScaling, NSLayoutAttribute,
    NSLayoutConstraintOrientation, NSPopUpButton, NSScrollView, NSSegmentSwitchTracking,
    NSSegmentedControl, NSStackView, NSSwitch, NSTextAlignment, NSTextField,
    NSUserInterfaceLayoutOrientation, NSView, NSVisualEffectBlendingMode, NSVisualEffectMaterial,
    NSVisualEffectState, NSVisualEffectView, NSWindow,
};
use objc2_foundation::{
    MainThreadMarker, NSData, NSEdgeInsets, NSObject, NSObjectProtocol, NSString,
};
use tao::event_loop::EventLoopProxy;
use tao::platform::macos::WindowExtMacOS;
use tao::window::Window;

use crate::gui::{UiCommand, UserEvent};
use crate::panel::{preferred_glass, DurationKey, GlassKind, PanelState, PanelView};

const TAG_RESLEEP: isize = 1;
const TAG_BATTERY: isize = 2;
const TAG_SCREEN_OFF: isize = 3;
const TAG_LID: isize = 4;
const TAG_LOCK: isize = 5;
const TAG_LOGIN: isize = 6;

struct PanelIvars {
    proxy: EventLoopProxy<UserEvent>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[name = "NeverSleepPanelTarget"]
    #[ivars = PanelIvars]
    struct PanelTarget;

    unsafe impl NSObjectProtocol for PanelTarget {}

    impl PanelTarget {
        #[unsafe(method(toggle:))]
        fn toggle(&self, _sender: Option<&AnyObject>) {
            self.emit(UiCommand::Toggle);
        }

        #[unsafe(method(more:))]
        fn more(&self, _sender: Option<&AnyObject>) {
            self.emit(UiCommand::More);
        }

        #[unsafe(method(back:))]
        fn back(&self, _sender: Option<&AnyObject>) {
            self.emit(UiCommand::Back);
        }

        #[unsafe(method(help:))]
        fn help(&self, _sender: Option<&AnyObject>) {
            self.emit(UiCommand::Help);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _sender: Option<&AnyObject>) {
            self.emit(UiCommand::Quit);
        }

        #[unsafe(method(durationChanged:))]
        fn duration_changed(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, indexOfSelectedItem] };
            if let Some(key) = DurationKey::from_index(index) {
                self.emit(UiCommand::SetDuration {
                    value: key.as_ipc().into(),
                });
            }
        }

        #[unsafe(method(optionChanged:))]
        fn option_changed(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let tag: isize = unsafe { msg_send![sender, tag] };
            let state: isize = unsafe { msg_send![sender, state] };
            let key = match tag {
                TAG_RESLEEP => "resleep_display",
                TAG_BATTERY => "battery_floor",
                TAG_SCREEN_OFF => "screen_off",
                TAG_LID => "lid_awake",
                TAG_LOCK => "lock_screen",
                TAG_LOGIN => "launch_at_login",
                _ => return,
            };
            self.emit(UiCommand::SetOption {
                key: key.into(),
                enabled: state != 0,
            });
        }

        #[unsafe(method(languageChanged:))]
        fn language_changed(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let index: isize = unsafe { msg_send![sender, selectedSegment] };
            let language = if index == 1 { "zh" } else { "en" };
            self.emit(UiCommand::SetLanguage {
                language: language.into(),
            });
        }
    }
);

impl PanelTarget {
    fn new(mtm: MainThreadMarker, proxy: EventLoopProxy<UserEvent>) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(PanelIvars { proxy });
        unsafe { msg_send![super(this), init] }
    }

    fn emit(&self, command: UiCommand) {
        let _ = self.ivars().proxy.send_event(UserEvent::Ui(command));
    }
}

pub struct NativePanel {
    _target: Retained<PanelTarget>,
    glass: Option<Retained<NSGlassEffectView>>,
    vibrancy: Option<Retained<NSVisualEffectView>>,
    main_view: Retained<NSView>,
    settings_view: Retained<NSView>,
    help_view: Retained<NSView>,
    hero: Retained<NSButton>,
    sun: Retained<NSImage>,
    moon: Retained<NSImage>,
    status_title: Retained<NSTextField>,
    summary: Retained<NSTextField>,
    warning: Retained<NSTextField>,
    primary: Retained<NSButton>,
    duration: Retained<NSPopUpButton>,
    duration_label: Retained<NSTextField>,
    resleep_label: Retained<NSTextField>,
    resleep: Retained<NSSwitch>,
    battery_label: Retained<NSTextField>,
    battery: Retained<NSSwitch>,
    more: Retained<NSButton>,
    quit_main: Retained<NSButton>,
    settings_title: Retained<NSTextField>,
    back: Retained<NSButton>,
    screen_off_label: Retained<NSTextField>,
    screen_off: Retained<NSSwitch>,
    lid_label: Retained<NSTextField>,
    lid: Retained<NSSwitch>,
    resleep_settings_label: Retained<NSTextField>,
    resleep_settings: Retained<NSSwitch>,
    lock_label: Retained<NSTextField>,
    lock: Retained<NSSwitch>,
    battery_settings_label: Retained<NSTextField>,
    battery_settings: Retained<NSSwitch>,
    login_label: Retained<NSTextField>,
    login: Retained<NSSwitch>,
    language: Retained<NSSegmentedControl>,
    help_btn: Retained<NSButton>,
    quit_settings: Retained<NSButton>,
    help_title: Retained<NSTextField>,
    help_back: Retained<NSButton>,
    help_kicker: Retained<NSTextField>,
    help_lead: Retained<NSTextField>,
    help_how: Retained<NSTextField>,
    help_step1_title: Retained<NSTextField>,
    help_step1_detail: Retained<NSTextField>,
    help_step2_title: Retained<NSTextField>,
    help_step2_detail: Retained<NSTextField>,
    help_step3_title: Retained<NSTextField>,
    help_step3: Retained<NSTextField>,
    help_notes: Retained<NSTextField>,
    help_note_lid: Retained<NSTextField>,
    help_note_battery: Retained<NSTextField>,
    help_note_quit: Retained<NSTextField>,
    current_view: PanelView,
    help_return: PanelView,
}

impl NativePanel {
    pub fn attach(window: &Window, proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("native panel: not on the main thread")?;
        let target = PanelTarget::new(mtm, proxy);
        let sun = load_png(include_bytes!("../ui/assets/sun.png"))?;
        let moon = load_png(include_bytes!("../ui/assets/moon.png"))?;
        sun.setSize(objc2_foundation::NSSize::new(88.0, 88.0));
        moon.setSize(objc2_foundation::NSSize::new(88.0, 88.0));

        let ns_window = unsafe { &*window.ns_window().cast::<NSWindow>() };
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&NSColor::clearColor()));

        let host = unsafe { &*window.ns_view().cast::<NSView>() };
        host.setWantsLayer(true);

        let glass_ok = AnyClass::get(c"NSGlassEffectView").is_some();
        let kind = preferred_glass(glass_ok);

        let (root, glass, vibrancy) = match kind {
            GlassKind::LiquidGlass => {
                let glass = NSGlassEffectView::new(mtm);
                glass.setStyle(NSGlassEffectViewStyle::Regular);
                glass.setCornerRadius(12.0);
                let content = NSView::new(mtm);
                content.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                glass.setContentView(Some(&content));
                fill(host, nv(&*glass));
                (content, Some(glass), None)
            }
            GlassKind::Vibrancy => {
                let visual = NSVisualEffectView::new(mtm);
                visual.setMaterial(NSVisualEffectMaterial::Popover);
                visual.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
                visual.setState(NSVisualEffectState::Active);
                visual.setWantsLayer(true);
                fill(host, nv(&*visual));
                let content = NSView::new(mtm);
                fill(nv(&*visual), &content);
                (content, None, Some(visual))
            }
        };

        let pages = NSView::new(mtm);
        fill(&root, &pages);

        let main_view = NSView::new(mtm);
        let settings_view = NSView::new(mtm);
        let help_view = NSView::new(mtm);
        fill(&pages, &main_view);
        fill(&pages, &settings_view);
        fill(&pages, &help_view);
        settings_view.setHidden(true);
        help_view.setHidden(true);

        let hero = unsafe {
            NSButton::buttonWithImage_target_action(
                &sun,
                Some(as_any(&target)),
                Some(sel!(toggle:)),
                mtm,
            )
        };
        hero.setBordered(false);
        hero.setImagePosition(NSCellImagePosition::ImageOnly);
        hero.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        hero.setFrameSize(objc2_foundation::NSSize::new(88.0, 88.0));
        hero.heightAnchor()
            .constraintEqualToConstant(88.0)
            .setActive(true);
        hero.widthAnchor()
            .constraintEqualToConstant(88.0)
            .setActive(true);

        let status_title = heading(mtm, 17.0);
        let summary = wrap(mtm);
        let warning = wrap(mtm);
        warning.setTextColor(Some(&NSColor::systemOrangeColor()));

        let primary_style = if glass_ok {
            NSBezelStyle::Glass
        } else {
            NSBezelStyle::Push
        };
        let primary = push_button(&target, sel!(toggle:), primary_style, mtm);
        let duration_label = label(mtm);
        let duration = NSPopUpButton::new(mtm);
        unsafe {
            duration.setTarget(Some(as_any(&target)));
            duration.setAction(Some(sel!(durationChanged:)));
        }
        let (resleep_label, resleep, resleep_row) = labeled_switch(&target, TAG_RESLEEP, mtm);
        let (battery_label, battery, battery_row) = labeled_switch(&target, TAG_BATTERY, mtm);
        let more = text_button(&target, sel!(more:), mtm);
        let quit_main = text_button(&target, sel!(quit:), mtm);

        let main_stack = column(mtm, 12.0, 16.0);
        main_stack.setAlignment(NSLayoutAttribute::CenterX);
        arrange(&main_stack, &hero);
        arrange(&main_stack, &status_title);
        arrange(&main_stack, &summary);
        arrange(&main_stack, &warning);
        arrange(&main_stack, &primary);
        stretch(nv(&*primary));
        let card = column(mtm, 8.0, 0.0);
        card.setAlignment(NSLayoutAttribute::Leading);
        arrange(
            &card,
            &duration_row(&duration_label, duration.as_ref(), mtm),
        );
        arrange(&card, &resleep_row);
        arrange(&card, &battery_row);
        stretch(nv(&*card));
        arrange(&main_stack, &card);
        arrange(&main_stack, &footer(&more, &quit_main, mtm));
        fill(&main_view, &main_stack);

        let back = text_button(&target, sel!(back:), mtm);
        let settings_title = heading(mtm, 15.0);
        let (screen_off_label, screen_off, screen_off_row) =
            labeled_switch(&target, TAG_SCREEN_OFF, mtm);
        let (lid_label, lid, lid_row) = labeled_switch(&target, TAG_LID, mtm);
        let (resleep_settings_label, resleep_settings, resleep_settings_row) =
            labeled_switch(&target, TAG_RESLEEP, mtm);
        let (lock_label, lock, lock_row) = labeled_switch(&target, TAG_LOCK, mtm);
        let (battery_settings_label, battery_settings, battery_settings_row) =
            labeled_switch(&target, TAG_BATTERY, mtm);
        let (login_label, login, login_row) = labeled_switch(&target, TAG_LOGIN, mtm);
        let language = NSSegmentedControl::new(mtm);
        language.setSegmentCount(2);
        language.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
        language.setLabel_forSegment(&ns("English"), 0);
        language.setLabel_forSegment(&ns("简体中文"), 1);
        unsafe {
            language.setTarget(Some(as_any(&target)));
            language.setAction(Some(sel!(languageChanged:)));
        }
        stretch(nv(&*language));
        let help_btn = text_button(&target, sel!(help:), mtm);
        let quit_settings = text_button(&target, sel!(quit:), mtm);

        let settings_stack = column(mtm, 10.0, 16.0);
        settings_stack.setAlignment(NSLayoutAttribute::Leading);
        arrange(&settings_stack, &header_row(&back, &settings_title, mtm));
        arrange(&settings_stack, &screen_off_row);
        arrange(&settings_stack, &lid_row);
        arrange(&settings_stack, &resleep_settings_row);
        arrange(&settings_stack, &lock_row);
        arrange(&settings_stack, &battery_settings_row);
        arrange(&settings_stack, &login_row);
        arrange(&settings_stack, &language);
        arrange(&settings_stack, &footer(&help_btn, &quit_settings, mtm));
        fill(&settings_view, &settings_stack);

        let help_back = text_button(&target, sel!(back:), mtm);
        let help_title = heading(mtm, 15.0);
        let help_kicker = heading(mtm, 12.0);
        let help_lead = wrap(mtm);
        let help_how = heading(mtm, 12.0);
        let help_step1_title = heading(mtm, 13.0);
        let help_step1_detail = wrap(mtm);
        let help_step2_title = heading(mtm, 13.0);
        let help_step2_detail = wrap(mtm);
        let help_step3_title = heading(mtm, 13.0);
        let help_step3 = wrap(mtm);
        let help_notes = heading(mtm, 12.0);
        let help_note_lid = wrap(mtm);
        let help_note_battery = wrap(mtm);
        let help_note_quit = wrap(mtm);

        let help_body = column(mtm, 8.0, 0.0);
        help_body.setAlignment(NSLayoutAttribute::Leading);
        arrange(&help_body, &help_kicker);
        arrange(&help_body, &help_lead);
        arrange(&help_body, &help_how);
        arrange(&help_body, &help_step1_title);
        arrange(&help_body, &help_step1_detail);
        arrange(&help_body, &help_step2_title);
        arrange(&help_body, &help_step2_detail);
        arrange(&help_body, &help_step3_title);
        arrange(&help_body, &help_step3);
        arrange(&help_body, &help_notes);
        arrange(&help_body, &help_note_lid);
        arrange(&help_body, &help_note_battery);
        arrange(&help_body, &help_note_quit);

        let scroll = NSScrollView::new(mtm);
        scroll.setHasVerticalScroller(true);
        scroll.setAutohidesScrollers(true);
        scroll.setDrawsBackground(false);
        scroll.setBorderType(NSBorderType::NoBorder);
        scroll.setDocumentView(Some(&help_body));
        stretch(nv(&*scroll));

        let help_stack = column(mtm, 10.0, 16.0);
        help_stack.setAlignment(NSLayoutAttribute::Leading);
        arrange(&help_stack, &header_row(&help_back, &help_title, mtm));
        arrange(&help_stack, &scroll);
        fill(&help_view, &help_stack);

        Ok(Self {
            _target: target,
            glass,
            vibrancy,
            main_view,
            settings_view,
            help_view,
            hero,
            sun,
            moon,
            status_title,
            summary,
            warning,
            primary,
            duration,
            duration_label,
            resleep_label,
            resleep,
            battery_label,
            battery,
            more,
            quit_main,
            settings_title,
            back,
            screen_off_label,
            screen_off,
            lid_label,
            lid,
            resleep_settings_label,
            resleep_settings,
            lock_label,
            lock,
            battery_settings_label,
            battery_settings,
            login_label,
            login,
            language,
            help_btn,
            quit_settings,
            help_title,
            help_back,
            help_kicker,
            help_lead,
            help_how,
            help_step1_title,
            help_step1_detail,
            help_step2_title,
            help_step2_detail,
            help_step3_title,
            help_step3,
            help_notes,
            help_note_lid,
            help_note_battery,
            help_note_quit,
            current_view: PanelView::Main,
            help_return: PanelView::Main,
        })
    }

    pub fn apply(&mut self, state: &PanelState) {
        self.hero
            .setImage(Some(if state.active { &self.moon } else { &self.sun }));
        self.hero.setToolTip(Some(&ns(&state.primary_action)));
        set_text(&self.status_title, &state.status_title);
        set_text(&self.summary, &state.summary);
        set_text(&self.warning, &state.warning);
        self.warning.setHidden(state.warning.is_empty());
        self.primary.setTitle(&ns(&state.primary_action));
        set_text(&self.duration_label, &state.duration_label);
        self.duration.removeAllItems();
        self.duration
            .addItemWithTitle(&ns(&state.duration_indefinite));
        self.duration.addItemWithTitle(&ns(&state.duration_1h));
        self.duration.addItemWithTitle(&ns(&state.duration_3h));
        self.duration.addItemWithTitle(&ns(&state.duration_8h));
        self.duration.addItemWithTitle(&ns(&state.duration_until));
        self.duration.selectItemAtIndex(state.duration.index());
        set_text(&self.resleep_label, &state.resleep);
        set_switch(&self.resleep, state.resleep_display);
        set_text(&self.battery_label, &state.battery);
        set_switch(&self.battery, state.battery_floor);
        self.more.setTitle(&ns(&state.more_settings));
        self.quit_main.setTitle(&ns(&state.quit));

        set_text(&self.settings_title, &state.settings);
        self.back.setTitle(&ns(&state.back));
        self.back.setToolTip(Some(&ns(&state.back)));
        set_text(&self.screen_off_label, &state.screen_off_label);
        set_switch(&self.screen_off, state.screen_off);
        set_text(&self.lid_label, &state.lid_awake_label);
        set_switch(&self.lid, state.lid_awake);
        set_text(&self.resleep_settings_label, &state.resleep);
        set_switch(&self.resleep_settings, state.resleep_display);
        set_text(&self.lock_label, &state.lock_screen_label);
        set_switch(&self.lock, state.lock_screen);
        set_text(&self.battery_settings_label, &state.battery);
        set_switch(&self.battery_settings, state.battery_floor);
        set_text(&self.login_label, &state.launch_at_login_label);
        set_switch(&self.login, state.launch_at_login);
        self.language
            .setSelectedSegment(if state.lang == never_sleep_core::Lang::Zh {
                1
            } else {
                0
            });
        self.help_btn.setTitle(&ns(&state.help));
        self.quit_settings.setTitle(&ns(&state.quit));

        set_text(&self.help_title, &state.help);
        self.help_back.setTitle(&ns(&state.back));
        self.help_back.setToolTip(Some(&ns(&state.back)));
        set_text(&self.help_kicker, &state.help_kicker);
        set_text(&self.help_lead, &state.help_lead);
        set_text(&self.help_how, &state.help_how);
        set_text(&self.help_step1_title, &state.help_step1_title);
        set_text(&self.help_step1_detail, &state.help_step1_detail);
        set_text(&self.help_step2_title, &state.help_step2_title);
        set_text(&self.help_step2_detail, &state.help_step2_detail);
        set_text(&self.help_step3_title, &state.help_step3_title);
        set_text(&self.help_step3, &state.help_step3);
        set_text(&self.help_notes, &state.help_notes);
        set_text(&self.help_note_lid, &state.help_note_lid);
        set_text(&self.help_note_battery, &state.help_note_battery);
        set_text(&self.help_note_quit, &state.help_note_quit);

        if let Some(glass) = &self.glass {
            if state.active {
                glass.setTintColor(Some(&NSColor::blackColor().colorWithAlphaComponent(0.28)));
            } else {
                glass.setTintColor(None);
            }
        }
        if let Some(visual) = &self.vibrancy {
            visual.setMaterial(if state.active {
                NSVisualEffectMaterial::HUDWindow
            } else {
                NSVisualEffectMaterial::Popover
            });
        }
        self.apply_view();
    }

    pub fn show_help(&mut self) {
        if self.current_view != PanelView::Help {
            self.help_return = self.current_view;
        }
        self.current_view = PanelView::Help;
        self.apply_view();
    }

    pub fn show_settings(&mut self) {
        self.current_view = PanelView::Settings;
        self.apply_view();
    }

    pub fn go_back(&mut self) {
        self.current_view = match self.current_view {
            PanelView::Help => self.help_return,
            PanelView::Settings => PanelView::Main,
            PanelView::Main => PanelView::Main,
        };
        self.apply_view();
    }

    fn apply_view(&self) {
        self.main_view
            .setHidden(self.current_view != PanelView::Main);
        self.settings_view
            .setHidden(self.current_view != PanelView::Settings);
        self.help_view
            .setHidden(self.current_view != PanelView::Help);
    }
}

fn as_any(obj: &PanelTarget) -> &AnyObject {
    // SAFETY: PanelTarget is an Objective-C class instance.
    unsafe { &*(core::ptr::from_ref(obj).cast::<AnyObject>()) }
}

fn ns(text: &str) -> Retained<NSString> {
    NSString::from_str(text)
}

fn load_png(bytes: &[u8]) -> Result<Retained<NSImage>, String> {
    let data = NSData::with_bytes(bytes);
    NSImage::initWithData(NSImage::alloc(), &data).ok_or_else(|| "panel image".into())
}

fn nv<T: AsRef<NSView>>(obj: &T) -> &NSView {
    obj.as_ref()
}

fn arrange<T>(stack: &NSStackView, child: &T)
where
    T: Deref,
    T::Target: AsRef<NSView>,
{
    stack.addArrangedSubview(child.deref().as_ref());
}

fn fill(parent: &NSView, child: &NSView) {
    child.setFrame(parent.bounds());
    child.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    parent.addSubview(child);
}

fn stretch(view: &NSView) {
    view.setContentHuggingPriority_forOrientation(1.0_f32, NSLayoutConstraintOrientation::Vertical);
}

fn column(mtm: MainThreadMarker, spacing: f64, inset: f64) -> Retained<NSStackView> {
    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setSpacing(spacing);
    stack.setEdgeInsets(NSEdgeInsets {
        top: inset,
        left: inset,
        bottom: inset,
        right: inset,
    });
    stack
}

fn label(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&ns(""), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    field.setTextColor(Some(&NSColor::labelColor()));
    field
}

fn heading(mtm: MainThreadMarker, size: f64) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&ns(""), mtm);
    field.setFont(Some(&NSFont::boldSystemFontOfSize(size)));
    field.setTextColor(Some(&NSColor::labelColor()));
    field.setAlignment(NSTextAlignment::Center);
    field
}

fn wrap(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&ns(""), mtm);
    field.setSelectable(false);
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    field.setAlignment(NSTextAlignment::Center);
    field
}

fn set_text(field: &NSTextField, value: &str) {
    field.setStringValue(&ns(value));
}

fn set_switch(control: &NSSwitch, on: bool) {
    control.setState(if on {
        NSControlStateValueOn
    } else {
        NSControlStateValueOff
    });
}

fn bind_switch(toggle: &NSSwitch, target: &PanelTarget, tag: isize) {
    unsafe {
        toggle.setTarget(Some(as_any(target)));
        toggle.setAction(Some(sel!(optionChanged:)));
    }
    toggle.setTag(tag);
}

fn push_button(
    target: &PanelTarget,
    action: Sel,
    bezel: NSBezelStyle,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&ns(""), Some(as_any(target)), Some(action), mtm)
    };
    button.setBezelStyle(bezel);
    button
}

fn text_button(target: &PanelTarget, action: Sel, mtm: MainThreadMarker) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(&ns(""), Some(as_any(target)), Some(action), mtm)
    };
    button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    button.setBordered(false);
    button
}

fn labeled_switch(
    target: &PanelTarget,
    tag: isize,
    mtm: MainThreadMarker,
) -> (
    Retained<NSTextField>,
    Retained<NSSwitch>,
    Retained<NSStackView>,
) {
    let caption = label(mtm);
    caption.setAlignment(NSTextAlignment::Left);
    let toggle = NSSwitch::new(mtm);
    bind_switch(&toggle, target, tag);
    let row = NSStackView::new(mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setAlignment(NSLayoutAttribute::CenterY);
    row.setSpacing(8.0);
    arrange(&row, &caption);
    arrange(&row, &toggle);
    (caption, toggle, row)
}

fn duration_row(
    caption: &NSTextField,
    popup: &NSPopUpButton,
    mtm: MainThreadMarker,
) -> Retained<NSStackView> {
    caption.setAlignment(NSTextAlignment::Left);
    let row = NSStackView::new(mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setAlignment(NSLayoutAttribute::CenterY);
    row.setSpacing(8.0);
    arrange(&row, caption);
    arrange(&row, popup);
    row
}

fn header_row(
    back: &NSButton,
    title: &NSTextField,
    mtm: MainThreadMarker,
) -> Retained<NSStackView> {
    let row = NSStackView::new(mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setAlignment(NSLayoutAttribute::CenterY);
    row.setSpacing(8.0);
    arrange(&row, back);
    arrange(&row, title);
    row
}

fn footer(left: &NSButton, right: &NSButton, mtm: MainThreadMarker) -> Retained<NSStackView> {
    let row = NSStackView::new(mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setAlignment(NSLayoutAttribute::CenterY);
    row.setSpacing(12.0);
    arrange(&row, left);
    arrange(&row, right);
    row
}
