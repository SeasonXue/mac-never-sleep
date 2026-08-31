//! Native macOS glass behind the HTML popover.
//!
//! The WKWebView is transparent. This inserts a rounded `NSVisualEffectView`
//! (or `NSGlassEffectView` on macOS 26+) *behind* it, matching the CSS `.shell`
//! inset, so the desktop shows through as frosted glass instead of a flat wash.

use std::ffi::c_char;

use cocoa::base::{id, nil};
use cocoa::foundation::{NSPoint, NSRect, NSSize};
use objc::runtime::{Class, BOOL, NO, YES};
use objc::{class, msg_send, sel, sel_impl};
use tao::platform::macos::WindowExtMacOS;
use tao::window::Window;

/// Must stay in sync with `.shell { inset: 10px 8px 8px; border-radius: 19px }`.
const INSET_TOP: f64 = 10.0;
const INSET_X: f64 = 8.0;
const INSET_BOTTOM: f64 = 8.0;
const CORNER_RADIUS: f64 = 19.0;

const MATERIAL_POPOVER: i64 = 6;
const BLENDING_BEHIND_WINDOW: i64 = 0;
const STATE_FOLLOWS_WINDOW: i64 = 1;
const GLASS_STYLE_CLEAR: i64 = 1;
const VIEW_WIDTH_SIZABLE: u64 = 2;
const VIEW_HEIGHT_SIZABLE: u64 = 16;

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

    let clear: id = msg_send![class!(NSColor), clearColor];
    let _: () = msg_send![ns_window, setOpaque: NO];
    let _: () = msg_send![ns_window, setBackgroundColor: clear];
    let _: () = msg_send![content, setWantsLayer: YES];

    let bounds: NSRect = msg_send![content, bounds];
    let width = (bounds.size.width - INSET_X * 2.0).max(1.0);
    let height = (bounds.size.height - INSET_TOP - INSET_BOTTOM).max(1.0);
    let frame = NSRect::new(
        NSPoint::new(INSET_X, INSET_BOTTOM),
        NSSize::new(width, height),
    );

    let clip_alloc: id = msg_send![class!(NSView), alloc];
    let clip: id = msg_send![clip_alloc, initWithFrame: frame];
    if clip == nil {
        return false;
    }
    let _: () = msg_send![clip, setWantsLayer: YES];
    let clip_layer: id = msg_send![clip, layer];
    if clip_layer != nil {
        let _: () = msg_send![clip_layer, setCornerRadius: CORNER_RADIUS];
        let _: () = msg_send![clip_layer, setMasksToBounds: YES];
    }

    let inner = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, height));
    let glass = create_glass_view(inner);
    if glass == nil {
        return false;
    }
    let _: () = msg_send![glass, setAutoresizingMask: VIEW_WIDTH_SIZABLE | VIEW_HEIGHT_SIZABLE];
    let _: () = msg_send![clip, addSubview: glass];
    let _: () = msg_send![content, addSubview: clip];
    let _: () = msg_send![content, sendSubviewToBack: clip];
    true
}

unsafe fn create_glass_view(frame: NSRect) -> id {
    let liquid = objc_getClass(c"NSGlassEffectView".as_ptr()) as id;
    if liquid != nil {
        let alloc: id = msg_send![liquid, alloc];
        let view: id = msg_send![alloc, initWithFrame: frame];
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

    let alloc: id = msg_send![class!(NSVisualEffectView), alloc];
    let view: id = msg_send![alloc, initWithFrame: frame];
    if view == nil {
        return nil;
    }
    let _: () = msg_send![view, setMaterial: MATERIAL_POPOVER];
    let _: () = msg_send![view, setBlendingMode: BLENDING_BEHIND_WINDOW];
    let _: () = msg_send![view, setState: STATE_FOLLOWS_WINDOW];
    view
}

unsafe fn responds(object: id, selector: objc::runtime::Sel) -> bool {
    let ok: BOOL = msg_send![object, respondsToSelector: selector];
    ok == YES
}
