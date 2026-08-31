//! Native macOS glass behind the HTML popover.
//!
//! The WKWebView is transparent. This inserts a rounded `NSVisualEffectView`
//! (or `NSGlassEffectView` on macOS 26+) *behind* it, matching the CSS `.shell`
//! inset, so the desktop shows through as frosted glass instead of a flat wash.

#![allow(deprecated)]

use std::ffi::c_char;

use cocoa::appkit::{
    NSColor, NSView, NSViewHeightSizable, NSViewWidthSizable, NSVisualEffectBlendingMode,
    NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView, NSWindow,
    NSWindowOrderingMode,
};
use cocoa::base::{id, nil, YES};
use cocoa::foundation::{NSPoint, NSRect, NSSize};
use objc::runtime::{Class, BOOL};
use objc::{msg_send, sel, sel_impl};
use tao::platform::macos::WindowExtMacOS;
use tao::window::Window;

/// Must stay in sync with `.shell { inset: 10px 8px 8px; border-radius: 19px }`.
const INSET_TOP: f64 = 10.0;
const INSET_X: f64 = 8.0;
const INSET_BOTTOM: f64 = 8.0;
const CORNER_RADIUS: f64 = 19.0;
const GLASS_STYLE_CLEAR: i64 = 1;

extern "C" {
    fn objc_getClass(name: *const c_char) -> *const Class;
}

pub fn apply_popover_glass(window: &Window) -> bool {
    unsafe { apply_popover_glass_inner(window) }
}

unsafe fn apply_popover_glass_inner(window: &Window) -> bool {
    let ns_window = window.ns_window() as id;
    let content = window.ns_view() as id;
    if ns_window == nil || content == nil {
        return false;
    }

    ns_window.setOpaque_(cocoa::base::NO);
    ns_window.setBackgroundColor_(NSColor::clearColor(nil));
    NSView::setWantsLayer(content, YES);

    let bounds = NSView::bounds(content);
    let width = (bounds.size.width - INSET_X * 2.0).max(1.0);
    let height = (bounds.size.height - INSET_TOP - INSET_BOTTOM).max(1.0);
    let frame = NSRect::new(
        NSPoint::new(INSET_X, INSET_BOTTOM),
        NSSize::new(width, height),
    );

    let clip = NSView::initWithFrame_(NSView::alloc(nil), frame);
    if clip == nil {
        return false;
    }
    NSView::setWantsLayer(clip, YES);
    round_layer(NSView::layer(clip), CORNER_RADIUS);

    let inner = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
    let glass = create_glass_view(inner);
    if glass == nil {
        return false;
    }
    NSView::setAutoresizingMask_(glass, NSViewWidthSizable | NSViewHeightSizable);
    NSView::addSubview_(clip, glass);

    let _: () = msg_send![
        content,
        addSubview: clip
        positioned: NSWindowOrderingMode::NSWindowBelow
        relativeTo: 0
    ];
    true
}

unsafe fn create_glass_view(frame: NSRect) -> id {
    let liquid = objc_getClass(c"NSGlassEffectView".as_ptr()) as id;
    if liquid != nil {
        let view: id = msg_send![liquid, alloc];
        let view: id = msg_send![view, initWithFrame: frame];
        if view != nil {
            if responds(view, sel!(setStyle:)) {
                let _: () = msg_send![view, setStyle: GLASS_STYLE_CLEAR];
            }
            if responds(view, sel!(setCornerRadius:)) {
                let _: () = msg_send![view, setCornerRadius: CORNER_RADIUS];
            }
            return view;
        }
    }

    let view = NSVisualEffectView::initWithFrame_(NSVisualEffectView::alloc(nil), frame);
    if view == nil {
        return nil;
    }
    view.setMaterial_(NSVisualEffectMaterial::Popover);
    view.setBlendingMode_(NSVisualEffectBlendingMode::BehindWindow);
    view.setState_(NSVisualEffectState::Active);
    view.setEmphasized_(YES);
    view
}

unsafe fn round_layer(layer: id, radius: f64) {
    if layer == nil {
        return;
    }
    let _: () = msg_send![layer, setCornerRadius: radius];
    let _: () = msg_send![layer, setMasksToBounds: YES];
}

unsafe fn responds(object: id, selector: objc::runtime::Sel) -> bool {
    let ok: BOOL = msg_send![object, respondsToSelector: selector];
    ok == YES
}
