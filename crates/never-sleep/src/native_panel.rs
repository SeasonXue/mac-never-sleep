//! Native AppKit utility panel. Sidebar uses Liquid Glass / Sidebar vibrancy;
//! the detail column is opaque so copy stays readable (Notes-like).

use std::ops::Deref;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSAutoresizingMaskOptions, NSBezelStyle, NSBorderType, NSButton, NSCellImagePosition, NSColor,
    NSControlStateValueOff, NSControlStateValueOn, NSFont, NSGlassEffectView,
    NSGlassEffectViewStyle, NSImage, NSImageScaling, NSImageView, NSLayoutAttribute,
    NSLayoutConstraintOrientation, NSPopUpButton, NSScrollView, NSSegmentSwitchTracking,
    NSSegmentedControl, NSStackView, NSStackViewDistribution, NSSwitch, NSTextAlignment,
    NSTextField, NSUserInterfaceLayoutOrientation, NSView, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
};
use objc2_foundation::{
    MainThreadMarker, NSData, NSEdgeInsets, NSObject, NSObjectProtocol, NSString,
};
use tao::event_loop::EventLoopProxy;
use tao::platform::macos::WindowExtMacOS;
use tao::window::Window;

use crate::gui::{UiCommand, UserEvent};
use crate::panel::{
    preferred_glass, DurationKey, GlassKind, PanelState, PanelView, SidebarItem, DETAIL_INSET,
    DETAIL_MAX_WIDTH, SIDEBAR_WIDTH,
};

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

        #[unsafe(method(selectPane:))]
        fn select_pane(&self, sender: Option<&AnyObject>) {
            let Some(sender) = sender else { return };
            let tag: isize = unsafe { msg_send![sender, tag] };
            self.emit(UiCommand::SelectPane { index: tag });
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
    sidebar_buttons: Vec<Retained<NSButton>>,
    group_standby: Retained<NSTextField>,
    group_options: Retained<NSTextField>,
    group_guide: Retained<NSTextField>,
    pane_standby: Retained<NSView>,
    pane_display: Retained<NSView>,
    pane_lid: Retained<NSView>,
    pane_safeguards: Retained<NSView>,
    pane_general: Retained<NSView>,
    pane_help: Retained<NSView>,
    glyph: Retained<NSImageView>,
    sun: Retained<NSImage>,
    moon: Retained<NSImage>,
    status_title: Retained<NSTextField>,
    summary: Retained<NSTextField>,
    warning: Retained<NSTextField>,
    primary: Retained<NSButton>,
    duration: Retained<NSPopUpButton>,
    duration_label: Retained<NSTextField>,
    hotkey_hint: Retained<NSTextField>,
    display_title: Retained<NSTextField>,
    display_lead: Retained<NSTextField>,
    screen_off_label: Retained<NSTextField>,
    screen_off: Retained<NSSwitch>,
    resleep_label: Retained<NSTextField>,
    resleep: Retained<NSSwitch>,
    lid_title: Retained<NSTextField>,
    lid_lead: Retained<NSTextField>,
    lid_label: Retained<NSTextField>,
    lid: Retained<NSSwitch>,
    safeguards_title: Retained<NSTextField>,
    safeguards_lead: Retained<NSTextField>,
    lock_label: Retained<NSTextField>,
    lock: Retained<NSSwitch>,
    battery_label: Retained<NSTextField>,
    battery: Retained<NSSwitch>,
    general_title: Retained<NSTextField>,
    general_lead: Retained<NSTextField>,
    login_label: Retained<NSTextField>,
    login: Retained<NSSwitch>,
    language_label: Retained<NSTextField>,
    language: Retained<NSSegmentedControl>,
    help_title: Retained<NSTextField>,
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
    current: SidebarItem,
}

impl NativePanel {
    pub fn attach(window: &Window, proxy: EventLoopProxy<UserEvent>) -> Result<Self, String> {
        let mtm = MainThreadMarker::new().ok_or("native panel: not on the main thread")?;
        let target = PanelTarget::new(mtm, proxy);
        let sun =
            sf_symbol("sun.max", 28.0).unwrap_or(load_png(include_bytes!("../ui/assets/sun.png"))?);
        let moon = sf_symbol("moon.zzz", 28.0)
            .unwrap_or(load_png(include_bytes!("../ui/assets/moon.png"))?);

        let ns_window = unsafe { &*window.ns_window().cast::<NSWindow>() };
        ns_window.setOpaque(true);
        ns_window.setBackgroundColor(Some(&NSColor::windowBackgroundColor()));

        let host = unsafe { &*window.ns_view().cast::<NSView>() };
        host.setWantsLayer(true);

        let glass_ok = AnyClass::get(c"NSGlassEffectView").is_some();
        let kind = preferred_glass(glass_ok);

        let (sidebar_root, sidebar_body) = sidebar_chrome(mtm, kind);
        let detail = NSView::new(mtm);
        pin_split(host, nv(&*sidebar_root), &detail);

        let group_standby = section_header(mtm);
        let group_options = section_header(mtm);
        let group_guide = section_header(mtm);
        let mut sidebar_buttons = Vec::new();
        let nav = column(mtm, 2.0, 12.0);
        nav.setAlignment(NSLayoutAttribute::Leading);
        arrange(&nav, &group_standby);
        for item in [
            SidebarItem::Standby,
            SidebarItem::Display,
            SidebarItem::Lid,
            SidebarItem::Safeguards,
            SidebarItem::General,
            SidebarItem::Help,
        ] {
            if item == SidebarItem::Display {
                arrange(&nav, &group_options);
            }
            if item == SidebarItem::Help {
                arrange(&nav, &group_guide);
            }
            let button = sidebar_button(&target, item, mtm);
            arrange(&nav, &button);
            sidebar_buttons.push(button);
        }
        fill(&sidebar_body, nv(&*nav));

        let pane_standby = NSView::new(mtm);
        let pane_display = NSView::new(mtm);
        let pane_lid = NSView::new(mtm);
        let pane_safeguards = NSView::new(mtm);
        let pane_general = NSView::new(mtm);
        let pane_help = NSView::new(mtm);
        for pane in [
            &pane_standby,
            &pane_display,
            &pane_lid,
            &pane_safeguards,
            &pane_general,
            &pane_help,
        ] {
            fill(&detail, pane);
        }

        let glyph = NSImageView::new(mtm);
        glyph.setImage(Some(&sun));
        glyph.setEditable(false);
        glyph.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        nv(&*glyph)
            .heightAnchor()
            .constraintEqualToConstant(28.0)
            .setActive(true);
        nv(&*glyph)
            .widthAnchor()
            .constraintEqualToConstant(28.0)
            .setActive(true);

        let status_title = heading(mtm, 22.0);
        let summary = wrap(mtm);
        let warning = wrap(mtm);
        warning.setTextColor(Some(&NSColor::systemOrangeColor()));
        let primary = push_button(&target, sel!(toggle:), NSBezelStyle::Push, mtm);
        let duration_label = label(mtm);
        let duration = NSPopUpButton::new(mtm);
        unsafe {
            duration.setTarget(Some(as_any(&target)));
            duration.setAction(Some(sel!(durationChanged:)));
        }
        let hotkey_hint = footnote(mtm);

        let identity = column(mtm, 2.0, 0.0);
        identity.setAlignment(NSLayoutAttribute::Leading);
        arrange(&identity, &status_title);
        arrange(&identity, &summary);
        let header = NSStackView::new(mtm);
        header.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
        header.setAlignment(NSLayoutAttribute::CenterY);
        header.setSpacing(12.0);
        arrange(&header, &glyph);
        arrange(&header, &identity);

        let standby_stack = detail_column(mtm);
        arrange(&standby_stack, &header);
        arrange(&standby_stack, &warning);
        arrange(&standby_stack, &primary);
        arrange(
            &standby_stack,
            &duration_row(&duration_label, duration.as_ref(), mtm),
        );
        arrange(&standby_stack, &hotkey_hint);
        pin_detail_content(&pane_standby, nv(&*standby_stack));

        let display_title = heading(mtm, 22.0);
        let display_lead = wrap(mtm);
        let (screen_off_label, screen_off, screen_off_row) =
            labeled_switch(&target, TAG_SCREEN_OFF, mtm);
        let (resleep_label, resleep, resleep_row) = labeled_switch(&target, TAG_RESLEEP, mtm);
        let display_stack = detail_column(mtm);
        arrange(&display_stack, &display_title);
        arrange(&display_stack, &display_lead);
        arrange(&display_stack, &screen_off_row);
        arrange(&display_stack, &resleep_row);
        pin_detail_content(&pane_display, nv(&*display_stack));

        let lid_title = heading(mtm, 22.0);
        let lid_lead = wrap(mtm);
        let (lid_label, lid, lid_row) = labeled_switch(&target, TAG_LID, mtm);
        let lid_stack = detail_column(mtm);
        arrange(&lid_stack, &lid_title);
        arrange(&lid_stack, &lid_lead);
        arrange(&lid_stack, &lid_row);
        pin_detail_content(&pane_lid, nv(&*lid_stack));

        let safeguards_title = heading(mtm, 22.0);
        let safeguards_lead = wrap(mtm);
        let (lock_label, lock, lock_row) = labeled_switch(&target, TAG_LOCK, mtm);
        let (battery_label, battery, battery_row) = labeled_switch(&target, TAG_BATTERY, mtm);
        let safeguards_stack = detail_column(mtm);
        arrange(&safeguards_stack, &safeguards_title);
        arrange(&safeguards_stack, &safeguards_lead);
        arrange(&safeguards_stack, &lock_row);
        arrange(&safeguards_stack, &battery_row);
        pin_detail_content(&pane_safeguards, nv(&*safeguards_stack));

        let general_title = heading(mtm, 22.0);
        let general_lead = wrap(mtm);
        let (login_label, login, login_row) = labeled_switch(&target, TAG_LOGIN, mtm);
        let language_label = label(mtm);
        let language = NSSegmentedControl::new(mtm);
        language.setSegmentCount(2);
        language.setTrackingMode(NSSegmentSwitchTracking::SelectOne);
        language.setLabel_forSegment(&ns("English"), 0);
        language.setLabel_forSegment(&ns("简体中文"), 1);
        unsafe {
            language.setTarget(Some(as_any(&target)));
            language.setAction(Some(sel!(languageChanged:)));
        }
        nv(&*language).setContentHuggingPriority_forOrientation(
            750.0_f32,
            NSLayoutConstraintOrientation::Horizontal,
        );
        let general_stack = detail_column(mtm);
        arrange(&general_stack, &general_title);
        arrange(&general_stack, &general_lead);
        arrange(&general_stack, &login_row);
        arrange(
            &general_stack,
            &trailing_control_row(&language_label, nv(&*language), mtm),
        );
        pin_detail_content(&pane_general, nv(&*general_stack));

        let help_title = heading(mtm, 22.0);
        let help_kicker = heading(mtm, 13.0);
        let help_lead = wrap(mtm);
        let help_how = heading(mtm, 13.0);
        let help_step1_title = heading(mtm, 13.0);
        let help_step1_detail = wrap(mtm);
        let help_step2_title = heading(mtm, 13.0);
        let help_step2_detail = wrap(mtm);
        let help_step3_title = heading(mtm, 13.0);
        let help_step3 = wrap(mtm);
        let help_notes = heading(mtm, 13.0);
        let help_note_lid = wrap(mtm);
        let help_note_battery = wrap(mtm);
        let help_note_quit = wrap(mtm);
        let help_body = detail_column(mtm);
        help_body.setEdgeInsets(NSEdgeInsets {
            top: 0.0,
            left: 0.0,
            bottom: 24.0,
            right: 0.0,
        });
        arrange(&help_body, &help_title);
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
        pin_document_width(&scroll, nv(&*help_body));
        stretch(nv(&*scroll));
        let help_stack = column(mtm, 0.0, DETAIL_INSET);
        help_stack.setAlignment(NSLayoutAttribute::Width);
        arrange(&help_stack, &scroll);
        fill(&pane_help, nv(&*help_stack));

        let mut panel = Self {
            _target: target,
            sidebar_buttons,
            group_standby,
            group_options,
            group_guide,
            pane_standby,
            pane_display,
            pane_lid,
            pane_safeguards,
            pane_general,
            pane_help,
            glyph,
            sun,
            moon,
            status_title,
            summary,
            warning,
            primary,
            duration,
            duration_label,
            hotkey_hint,
            display_title,
            display_lead,
            screen_off_label,
            screen_off,
            resleep_label,
            resleep,
            lid_title,
            lid_lead,
            lid_label,
            lid,
            safeguards_title,
            safeguards_lead,
            lock_label,
            lock,
            battery_label,
            battery,
            general_title,
            general_lead,
            login_label,
            login,
            language_label,
            language,
            help_title,
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
            current: SidebarItem::Standby,
        };
        panel.apply_view();
        Ok(panel)
    }

    pub fn apply(&mut self, state: &PanelState) {
        self.glyph
            .setImage(Some(if state.active { &self.moon } else { &self.sun }));
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
        set_text(&self.hotkey_hint, &state.hotkey_hint);

        set_text(&self.group_standby, &state.section_session);
        set_text(&self.group_options, &state.sidebar_options);
        set_text(&self.group_guide, &state.sidebar_guide);
        let titles = [
            &state.section_session,
            &state.section_display,
            &state.section_lid,
            &state.section_safeguards,
            &state.section_general,
            &state.help,
        ];
        for (button, title) in self.sidebar_buttons.iter().zip(titles) {
            button.setTitle(&ns(title));
        }

        set_text(&self.display_title, &state.section_display);
        set_text(&self.display_lead, &state.pane_display_lead);
        set_text(&self.screen_off_label, &state.screen_off_label);
        set_switch(&self.screen_off, state.screen_off);
        set_text(&self.resleep_label, &state.resleep);
        set_switch(&self.resleep, state.resleep_display);

        set_text(&self.lid_title, &state.section_lid);
        set_text(&self.lid_lead, &state.pane_lid_lead);
        set_text(&self.lid_label, &state.lid_awake_label);
        set_switch(&self.lid, state.lid_awake);

        set_text(&self.safeguards_title, &state.section_safeguards);
        set_text(&self.safeguards_lead, &state.pane_safeguards_lead);
        set_text(&self.lock_label, &state.lock_screen_label);
        set_switch(&self.lock, state.lock_screen);
        set_text(&self.battery_label, &state.battery);
        set_switch(&self.battery, state.battery_floor);

        set_text(&self.general_title, &state.section_general);
        set_text(&self.general_lead, &state.pane_general_lead);
        set_text(&self.login_label, &state.launch_at_login_label);
        set_switch(&self.login, state.launch_at_login);
        set_text(&self.language_label, &state.language_label);
        self.language
            .setSelectedSegment(if state.lang == never_sleep_core::Lang::Zh {
                1
            } else {
                0
            });

        set_text(&self.help_title, &state.help);
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
        self.apply_view();
    }

    pub fn show_help(&mut self) {
        self.show_pane(SidebarItem::Help);
    }

    pub fn show_settings(&mut self) {
        self.show_pane(SidebarItem::Display);
    }

    pub fn go_back(&mut self) {
        self.show_pane(SidebarItem::Standby);
    }

    pub fn show_pane(&mut self, item: SidebarItem) {
        self.current = item;
        match item.as_panel_view() {
            PanelView::Main | PanelView::Settings | PanelView::Help => {}
        }
        self.apply_view();
    }

    pub fn select_adjacent(&mut self, delta: isize) {
        let last = SidebarItem::ALL.len() as isize - 1;
        let next = (self.current.index() + delta).clamp(0, last);
        if let Some(item) = SidebarItem::from_index(next) {
            self.show_pane(item);
        }
    }

    fn apply_view(&self) {
        self.pane_standby
            .setHidden(self.current != SidebarItem::Standby);
        self.pane_display
            .setHidden(self.current != SidebarItem::Display);
        self.pane_lid.setHidden(self.current != SidebarItem::Lid);
        self.pane_safeguards
            .setHidden(self.current != SidebarItem::Safeguards);
        self.pane_general
            .setHidden(self.current != SidebarItem::General);
        self.pane_help.setHidden(self.current != SidebarItem::Help);
        for (index, button) in self.sidebar_buttons.iter().enumerate() {
            let on = self.current.index() == index as isize;
            button.setBordered(on);
        }
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

fn sf_symbol(name: &str, point: f64) -> Option<Retained<NSImage>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(&ns(name), None)?;
    image.setSize(objc2_foundation::NSSize::new(point, point));
    image.setTemplate(true);
    Some(image)
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

fn pin_document_width(scroll: &NSScrollView, document: &NSView) {
    document.setTranslatesAutoresizingMaskIntoConstraints(false);
    let clip = scroll.contentView();
    document
        .leadingAnchor()
        .constraintEqualToAnchor(&clip.leadingAnchor())
        .setActive(true);
    document
        .topAnchor()
        .constraintEqualToAnchor(&clip.topAnchor())
        .setActive(true);
    document
        .widthAnchor()
        .constraintEqualToAnchor(&clip.widthAnchor())
        .setActive(true);
}

fn pin_detail_content(pane: &NSView, content: &NSView) {
    content.setTranslatesAutoresizingMaskIntoConstraints(false);
    pane.addSubview(content);
    content
        .leadingAnchor()
        .constraintEqualToAnchor(&pane.leadingAnchor())
        .setActive(true);
    content
        .topAnchor()
        .constraintEqualToAnchor(&pane.topAnchor())
        .setActive(true);
    content
        .trailingAnchor()
        .constraintLessThanOrEqualToAnchor(&pane.trailingAnchor())
        .setActive(true);
    content
        .bottomAnchor()
        .constraintLessThanOrEqualToAnchor(&pane.bottomAnchor())
        .setActive(true);
}

fn pin_split(host: &NSView, sidebar: &NSView, detail: &NSView) {
    sidebar.setTranslatesAutoresizingMaskIntoConstraints(false);
    detail.setTranslatesAutoresizingMaskIntoConstraints(false);
    host.addSubview(sidebar);
    host.addSubview(detail);
    sidebar
        .leadingAnchor()
        .constraintEqualToAnchor(&host.leadingAnchor())
        .setActive(true);
    sidebar
        .topAnchor()
        .constraintEqualToAnchor(&host.topAnchor())
        .setActive(true);
    sidebar
        .bottomAnchor()
        .constraintEqualToAnchor(&host.bottomAnchor())
        .setActive(true);
    sidebar
        .widthAnchor()
        .constraintEqualToConstant(SIDEBAR_WIDTH)
        .setActive(true);
    detail
        .leadingAnchor()
        .constraintEqualToAnchor(&sidebar.trailingAnchor())
        .setActive(true);
    detail
        .topAnchor()
        .constraintEqualToAnchor(&host.topAnchor())
        .setActive(true);
    detail
        .bottomAnchor()
        .constraintEqualToAnchor(&host.bottomAnchor())
        .setActive(true);
    detail
        .trailingAnchor()
        .constraintEqualToAnchor(&host.trailingAnchor())
        .setActive(true);
}

fn sidebar_chrome(mtm: MainThreadMarker, kind: GlassKind) -> (Retained<NSView>, Retained<NSView>) {
    match kind {
        GlassKind::LiquidGlass => {
            let glass = NSGlassEffectView::new(mtm);
            glass.setStyle(NSGlassEffectViewStyle::Regular);
            glass.setCornerRadius(0.0);
            let content = NSView::new(mtm);
            content.setAutoresizingMask(
                NSAutoresizingMaskOptions::ViewWidthSizable
                    | NSAutoresizingMaskOptions::ViewHeightSizable,
            );
            glass.setContentView(Some(&content));
            (Retained::into_super(glass), content)
        }
        GlassKind::Vibrancy => {
            let visual = NSVisualEffectView::new(mtm);
            visual.setMaterial(NSVisualEffectMaterial::Sidebar);
            visual.setBlendingMode(NSVisualEffectBlendingMode::BehindWindow);
            visual.setState(NSVisualEffectState::Active);
            let content = NSView::new(mtm);
            fill(nv(&*visual), &content);
            (Retained::into_super(visual), content)
        }
    }
}

fn sidebar_button(
    target: &PanelTarget,
    item: SidebarItem,
    mtm: MainThreadMarker,
) -> Retained<NSButton> {
    let button = unsafe {
        NSButton::buttonWithTitle_target_action(
            &ns(""),
            Some(as_any(target)),
            Some(sel!(selectPane:)),
            mtm,
        )
    };
    button.setBezelStyle(NSBezelStyle::AccessoryBarAction);
    button.setBordered(false);
    button.setAlignment(NSTextAlignment::Left);
    button.setImagePosition(NSCellImagePosition::ImageLeading);
    if let Some(symbol) = sf_symbol(item.symbol(), 14.0) {
        button.setImage(Some(&symbol));
    }
    button.setTag(item.index());
    button.setContentHuggingPriority_forOrientation(
        1.0_f32,
        NSLayoutConstraintOrientation::Horizontal,
    );
    button
}

fn detail_column(mtm: MainThreadMarker) -> Retained<NSStackView> {
    let stack = column(mtm, 14.0, DETAIL_INSET);
    stack.setAlignment(NSLayoutAttribute::Leading);
    nv(&*stack)
        .widthAnchor()
        .constraintLessThanOrEqualToConstant(DETAIL_MAX_WIDTH)
        .setActive(true);
    stack
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
    field.setAlignment(NSTextAlignment::Left);
    field
}

fn section_header(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&ns(""), mtm);
    field.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    field.setAlignment(NSTextAlignment::Left);
    field
}

fn wrap(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&ns(""), mtm);
    field.setSelectable(false);
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    field.setTextColor(Some(&NSColor::secondaryLabelColor()));
    field.setAlignment(NSTextAlignment::Left);
    field.setPreferredMaxLayoutWidth(DETAIL_MAX_WIDTH - DETAIL_INSET);
    field
}

fn footnote(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&ns(""), mtm);
    field.setSelectable(false);
    field.setFont(Some(&NSFont::systemFontOfSize(11.0)));
    field.setTextColor(Some(&NSColor::tertiaryLabelColor()));
    field.setAlignment(NSTextAlignment::Left);
    field.setPreferredMaxLayoutWidth(DETAIL_MAX_WIDTH - DETAIL_INSET);
    field
}

fn row_caption(mtm: MainThreadMarker) -> Retained<NSTextField> {
    let field = NSTextField::wrappingLabelWithString(&ns(""), mtm);
    field.setSelectable(false);
    field.setFont(Some(&NSFont::systemFontOfSize(13.0)));
    field.setTextColor(Some(&NSColor::labelColor()));
    field.setAlignment(NSTextAlignment::Left);
    field.setPreferredMaxLayoutWidth(DETAIL_MAX_WIDTH - 80.0);
    field.setContentHuggingPriority_forOrientation(
        1.0_f32,
        NSLayoutConstraintOrientation::Horizontal,
    );
    field.setContentCompressionResistancePriority_forOrientation(
        250.0_f32,
        NSLayoutConstraintOrientation::Horizontal,
    );
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

fn labeled_switch(
    target: &PanelTarget,
    tag: isize,
    mtm: MainThreadMarker,
) -> (
    Retained<NSTextField>,
    Retained<NSSwitch>,
    Retained<NSStackView>,
) {
    let caption = row_caption(mtm);
    let toggle = NSSwitch::new(mtm);
    bind_switch(&toggle, target, tag);
    nv(&*toggle).setContentHuggingPriority_forOrientation(
        750.0_f32,
        NSLayoutConstraintOrientation::Horizontal,
    );
    nv(&*toggle).setContentCompressionResistancePriority_forOrientation(
        1000.0_f32,
        NSLayoutConstraintOrientation::Horizontal,
    );
    let row = NSStackView::new(mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setDistribution(NSStackViewDistribution::Fill);
    row.setAlignment(NSLayoutAttribute::CenterY);
    row.setSpacing(8.0);
    arrange(&row, &caption);
    arrange(&row, &toggle);
    nv(&*row)
        .widthAnchor()
        .constraintEqualToConstant(DETAIL_MAX_WIDTH - DETAIL_INSET)
        .setActive(true);
    (caption, toggle, row)
}

fn duration_row(
    caption: &NSTextField,
    popup: &NSPopUpButton,
    mtm: MainThreadMarker,
) -> Retained<NSStackView> {
    trailing_control_row(caption, nv(popup), mtm)
}

fn trailing_control_row(
    caption: &NSTextField,
    control: &NSView,
    mtm: MainThreadMarker,
) -> Retained<NSStackView> {
    caption.setAlignment(NSTextAlignment::Left);
    control.setContentHuggingPriority_forOrientation(
        750.0_f32,
        NSLayoutConstraintOrientation::Horizontal,
    );
    let row = NSStackView::new(mtm);
    row.setOrientation(NSUserInterfaceLayoutOrientation::Horizontal);
    row.setDistribution(NSStackViewDistribution::Fill);
    row.setAlignment(NSLayoutAttribute::CenterY);
    row.setSpacing(8.0);
    arrange(&row, caption);
    row.addArrangedSubview(control);
    nv(&*row)
        .widthAnchor()
        .constraintEqualToConstant(DETAIL_MAX_WIDTH - DETAIL_INSET)
        .setActive(true);
    row
}
