//! macOS 电源与主机状态。
//!
//! 不改写用户的 `pmset` 节能偏好。断言随进程释放；合盖标志在 Drop / panic / 孤儿锁里还原。
#![allow(dead_code)]

use std::ffi::{c_char, c_void, CString};
use std::fs;
use std::io::Read;
use std::process::Command;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use never_sleep_core::{HostSnapshot, PowerPlan, Thermal};

use crate::clock::{base_snapshot, monotonic_ms};
use crate::paths::{current_exe, ensure_data_dir, launch_agent_path, session_lock_path};
use crate::platform::Platform;

const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
const K_IOPM_ASSERTION_LEVEL_ON: u32 = 255;
const K_IO_RETURN_SUCCESS: i32 = 0;
const K_CF_NUMBER_SINT64_TYPE: i32 = 4;
const K_PM_SET_CLAMSHELL_SLEEP_STATE: u32 = 12;
const K_CF_RUN_LOOP_DEFAULT_MODE: &str = "kCFRunLoopDefaultMode";

const MSG_CAN_SYSTEM_SLEEP: u32 = 0xE000_0270;
const MSG_SYSTEM_WILL_SLEEP: u32 = 0xE000_0280;
const MSG_SYSTEM_HAS_POWERED_ON: u32 = 0xE000_0300;

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static ROOT_POWER_PORT: AtomicU32 = AtomicU32::new(0);
static CLAMSHELL_CONNECT: AtomicU32 = AtomicU32::new(0);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    fn CFGetTypeID(cf: *const c_void) -> usize;
    fn CFStringGetTypeID() -> usize;
    fn CFNumberGetTypeID() -> usize;
    fn CFBooleanGetTypeID() -> usize;
    fn CFBooleanGetValue(b: *const c_void) -> u8;
    fn CFNumberGetValue(n: *const c_void, the_type: i32, value_ptr: *mut c_void) -> u8;
    fn CFStringGetCString(
        the_string: *const c_void,
        buffer: *mut c_char,
        buffer_size: i64,
        encoding: u32,
    ) -> u8;
    fn CFDictionaryGetValue(dict: *const c_void, key: *const c_void) -> *const c_void;
    fn CFArrayGetCount(arr: *const c_void) -> i64;
    fn CFArrayGetValueAtIndex(arr: *const c_void, idx: i64) -> *const c_void;
    fn CFRunLoopGetCurrent() -> *mut c_void;
    fn CFRunLoopAddSource(rl: *mut c_void, source: *mut c_void, mode: *const c_void);
    fn CFRunLoopRun();
    static kCFRunLoopCommonModes: *const c_void;
    static kCFBooleanTrue: *const c_void;
}

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOPMAssertionCreateWithName(
        assertion_type: *const c_void,
        assertion_level: u32,
        assertion_name: *const c_void,
        assertion_id: *mut u32,
    ) -> i32;
    fn IOPMAssertionRelease(assertion_id: u32) -> i32;
    fn IOServiceMatching(name: *const c_char) -> *mut c_void;
    fn IOServiceGetMatchingService(main_port: u32, matching: *mut c_void) -> u32;
    fn IOServiceOpen(service: u32, owning_task: u32, type_: u32, connect: *mut u32) -> i32;
    fn IOServiceClose(connect: u32) -> i32;
    fn IOObjectRelease(obj: u32) -> i32;
    fn IOConnectCallScalarMethod(
        connect: u32,
        selector: u32,
        input: *const u64,
        input_cnt: u32,
        output: *mut u64,
        output_cnt: *mut u32,
    ) -> i32;
    fn IORegistryEntryCreateCFProperty(
        entry: u32,
        key: *const c_void,
        allocator: *const c_void,
        options: u32,
    ) -> *const c_void;
    fn IORegistryEntryCreateCFProperties(
        entry: u32,
        properties: *mut *mut c_void,
        allocator: *const c_void,
        options: u32,
    ) -> i32;
    fn IOPSCopyPowerSourcesInfo() -> *const c_void;
    fn IOPSCopyPowerSourcesList(blob: *const c_void) -> *const c_void;
    fn IOPSGetPowerSourceDescription(blob: *const c_void, ps: *const c_void) -> *const c_void;
    fn IOPSGetProvidingPowerSourceType(blob: *const c_void) -> *const c_void;
    fn IORegisterForSystemPower(
        refcon: *mut c_void,
        the_port_ref: *mut *mut c_void,
        callback: extern "C" fn(*mut c_void, u32, u32, *mut c_void),
        notifier: *mut u32,
    ) -> u32;
    fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> *mut c_void;
    fn IOAllowPowerChange(kernel_port: u32, notification_id: i64) -> i32;
    fn IOCancelPowerChange(kernel_port: u32, notification_id: i64) -> i32;
    fn IORegistryEntrySetCFProperty(entry: u32, name: *const c_void, value: *const c_void) -> i32;
}

fn cf_string(s: &str) -> *const c_void {
    let c = CString::new(s).unwrap_or_else(|_| CString::new("never-sleep").unwrap());
    unsafe { CFStringCreateWithCString(ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8) }
}

fn cf_release(p: *const c_void) {
    if !p.is_null() {
        unsafe { CFRelease(p) }
    }
}

fn cf_string_to_rust(cf: *const c_void) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    let mut buf = [0u8; 512];
    let ok = unsafe {
        CFStringGetCString(
            cf,
            buf.as_mut_ptr() as *mut c_char,
            buf.len() as i64,
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    String::from_utf8(buf[..nul].to_vec()).ok()
}

fn cf_i64(cf: *const c_void) -> Option<i64> {
    if cf.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(cf) != CFNumberGetTypeID() {
            return None;
        }
        let mut v: i64 = 0;
        if CFNumberGetValue(
            cf,
            K_CF_NUMBER_SINT64_TYPE,
            &mut v as *mut i64 as *mut c_void,
        ) == 0
        {
            return None;
        }
        Some(v)
    }
}

fn cf_bool(cf: *const c_void) -> Option<bool> {
    if cf.is_null() {
        return None;
    }
    unsafe {
        if CFGetTypeID(cf) != CFBooleanGetTypeID() {
            return None;
        }
        Some(CFBooleanGetValue(cf) != 0)
    }
}

fn create_assertion(kind: &str, name: &str) -> Option<u32> {
    let ty = cf_string(kind);
    let nm = cf_string(name);
    if ty.is_null() || nm.is_null() {
        cf_release(ty);
        cf_release(nm);
        return None;
    }
    let mut id = 0u32;
    let ret = unsafe { IOPMAssertionCreateWithName(ty, K_IOPM_ASSERTION_LEVEL_ON, nm, &mut id) };
    cf_release(ty);
    cf_release(nm);
    if ret == K_IO_RETURN_SUCCESS {
        Some(id)
    } else {
        None
    }
}

fn release_assertion(id: &mut Option<u32>) {
    if let Some(v) = id.take() {
        unsafe {
            let _ = IOPMAssertionRelease(v);
        }
    }
}

extern "C" fn power_callback(
    _refcon: *mut c_void,
    _service: u32,
    message_type: u32,
    message_argument: *mut c_void,
) {
    let arg = message_argument as usize as i64;
    let port = ROOT_POWER_PORT.load(Ordering::SeqCst);
    if port == 0 {
        return;
    }
    match message_type {
        MSG_CAN_SYSTEM_SLEEP => {
            if SESSION_ACTIVE.load(Ordering::SeqCst) {
                unsafe {
                    let _ = IOCancelPowerChange(port, arg);
                }
            } else {
                unsafe {
                    let _ = IOAllowPowerChange(port, arg);
                }
            }
        }
        MSG_SYSTEM_WILL_SLEEP => unsafe {
            let _ = IOAllowPowerChange(port, arg);
        },
        MSG_SYSTEM_HAS_POWERED_ON => {
            if SESSION_ACTIVE.load(Ordering::SeqCst) {
                set_clamshell_sleep_disabled(true);
            }
        }
        _ => {}
    }
}

fn matching_service(class: &str) -> u32 {
    let c = CString::new(class).unwrap();
    unsafe {
        let matching = IOServiceMatching(c.as_ptr());
        if matching.is_null() {
            return 0;
        }
        IOServiceGetMatchingService(0, matching)
    }
}

fn open_root_domain() -> Option<u32> {
    let service = matching_service("IOPMrootDomain");
    if service == 0 {
        return None;
    }
    let mut conn = 0u32;
    let task = unsafe { libc::mach_task_self() };
    let ret = unsafe { IOServiceOpen(service, task, 0, &mut conn) };
    unsafe {
        let _ = IOObjectRelease(service);
    }
    if ret == K_IO_RETURN_SUCCESS && conn != 0 {
        Some(conn)
    } else {
        None
    }
}

fn set_clamshell_sleep_disabled(disabled: bool) -> bool {
    let conn = CLAMSHELL_CONNECT.load(Ordering::SeqCst);
    if conn == 0 {
        return false;
    }
    let input: [u64; 1] = [u64::from(disabled)];
    let mut out_cnt: u32 = 0;
    let ret = unsafe {
        IOConnectCallScalarMethod(
            conn,
            K_PM_SET_CLAMSHELL_SLEEP_STATE,
            input.as_ptr(),
            1,
            ptr::null_mut(),
            &mut out_cnt,
        )
    };
    ret == K_IO_RETURN_SUCCESS
}

fn property_on_service(class: &str, key: &str) -> *const c_void {
    let service = matching_service(class);
    if service == 0 {
        return ptr::null();
    }
    let k = cf_string(key);
    let prop = unsafe { IORegistryEntryCreateCFProperty(service, k, ptr::null(), 0) };
    cf_release(k);
    unsafe {
        let _ = IOObjectRelease(service);
    }
    prop
}

fn hid_idle_ms() -> u64 {
    let prop = property_on_service("IOHIDSystem", "HIDIdleTime");
    let ns = cf_i64(prop).unwrap_or(0) as u64;
    cf_release(prop);
    ns / 1_000_000
}

fn lid_closed() -> bool {
    let prop = property_on_service("IOPMrootDomain", "AppleClamshellState");
    let closed = cf_bool(prop).unwrap_or(false);
    cf_release(prop);
    closed
}

fn display_asleep() -> Option<bool> {
    let service = matching_service("IODisplayWrangler");
    if service == 0 {
        return None;
    }
    let mut props: *mut c_void = ptr::null_mut();
    let ret = unsafe { IORegistryEntryCreateCFProperties(service, &mut props, ptr::null(), 0) };
    unsafe {
        let _ = IOObjectRelease(service);
    }
    if ret != K_IO_RETURN_SUCCESS || props.is_null() {
        return None;
    }
    let key = cf_string("IOPowerManagement");
    let pm = unsafe { CFDictionaryGetValue(props, key) };
    cf_release(key);
    let state_key = cf_string("CurrentPowerState");
    let state = if pm.is_null() {
        None
    } else {
        cf_i64(unsafe { CFDictionaryGetValue(pm, state_key) })
    };
    cf_release(state_key);
    cf_release(props);
    // 0/1 通常是休眠，>=2 为点亮（机型之间略有差异）
    state.map(|s| s <= 1)
}

fn power_source() -> (bool, Option<u8>) {
    unsafe {
        let info = IOPSCopyPowerSourcesInfo();
        if info.is_null() {
            return (true, None);
        }
        let kind = IOPSGetProvidingPowerSourceType(info);
        let on_ac = cf_string_to_rust(kind)
            .map(|s| s.contains("AC") || s.contains("UPS"))
            .unwrap_or(true);

        let mut battery = None;
        let list = IOPSCopyPowerSourcesList(info);
        if !list.is_null() {
            let n = CFArrayGetCount(list);
            for i in 0..n {
                let ps = CFArrayGetValueAtIndex(list, i);
                let desc = IOPSGetPowerSourceDescription(info, ps);
                if desc.is_null() {
                    continue;
                }
                let cur_k = cf_string("Current Capacity");
                let max_k = cf_string("Max Capacity");
                let cur = cf_i64(CFDictionaryGetValue(desc, cur_k));
                let max = cf_i64(CFDictionaryGetValue(desc, max_k));
                cf_release(cur_k);
                cf_release(max_k);
                if let (Some(c), Some(m)) = (cur, max) {
                    if m > 0 {
                        battery = Some(((c * 100) / m).clamp(0, 100) as u8);
                        break;
                    }
                }
            }
            CFRelease(list);
        }
        CFRelease(info);
        (on_ac, battery)
    }
}

fn thermal_state() -> Thermal {
    // NSProcessInfo 在 CLI 线程里也可用；失败则视为正常。
    match Command::new("sysctl")
        .args(["-n", "machdep.xcpm.cpu_thermal_level"])
        .output()
    {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout);
            match s.trim().parse::<i32>().unwrap_or(0) {
                0 => Thermal::Nominal,
                1 => Thermal::Fair,
                2 => Thermal::Serious,
                _ => Thermal::Critical,
            }
        }
        _ => Thermal::Nominal,
    }
}

fn escape_as(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn write_lock(clamshell: bool) {
    let _ = ensure_data_dir();
    let body = format!(
        "pid={}\nclamshell={}\n",
        std::process::id(),
        u8::from(clamshell)
    );
    let _ = fs::write(session_lock_path(), body);
}

fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

fn parse_lock() -> Option<(u32, bool)> {
    let mut s = String::new();
    fs::File::open(session_lock_path())
        .ok()?
        .read_to_string(&mut s)
        .ok()?;
    let mut pid = 0u32;
    let mut clamshell = false;
    for line in s.lines() {
        if let Some(v) = line.strip_prefix("pid=") {
            pid = v.trim().parse().unwrap_or(0);
        }
        if let Some(v) = line.strip_prefix("clamshell=") {
            clamshell = v.trim() == "1";
        }
    }
    Some((pid, clamshell))
}

pub struct MacPlatform {
    idle_id: Option<u32>,
    system_id: Option<u32>,
    disk_id: Option<u32>,
    network_id: Option<u32>,
    clamshell_on: bool,
    power_thread_started: bool,
}

impl MacPlatform {
    pub fn new() -> Self {
        let mut me = Self {
            idle_id: None,
            system_id: None,
            disk_id: None,
            network_id: None,
            clamshell_on: false,
            power_thread_started: false,
        };
        me.ensure_clamshell_conn();
        me.cleanup_orphans();
        me.start_power_thread();
        install_panic_cleanup();
        me
    }

    fn ensure_clamshell_conn(&self) {
        if CLAMSHELL_CONNECT.load(Ordering::SeqCst) != 0 {
            return;
        }
        if let Some(conn) = open_root_domain() {
            CLAMSHELL_CONNECT.store(conn, Ordering::SeqCst);
        }
    }

    fn start_power_thread(&mut self) {
        if self.power_thread_started {
            return;
        }
        self.power_thread_started = true;
        thread::Builder::new()
            .name("never-sleep-pm".into())
            .spawn(|| unsafe {
                let mut port: *mut c_void = ptr::null_mut();
                let mut notifier: u32 = 0;
                let root = IORegisterForSystemPower(
                    ptr::null_mut(),
                    &mut port,
                    power_callback,
                    &mut notifier,
                );
                if root == 0 || port.is_null() {
                    return;
                }
                ROOT_POWER_PORT.store(root, Ordering::SeqCst);
                let src = IONotificationPortGetRunLoopSource(port);
                CFRunLoopAddSource(CFRunLoopGetCurrent(), src, kCFRunLoopCommonModes);
                let _ = K_CF_RUN_LOOP_DEFAULT_MODE;
                CFRunLoopRun();
            })
            .ok();
    }
}

fn install_panic_cleanup() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            SESSION_ACTIVE.store(false, Ordering::SeqCst);
            set_clamshell_sleep_disabled(false);
            let _ = fs::remove_file(session_lock_path());
            prev(info);
        }));
    });
}

impl Drop for MacPlatform {
    fn drop(&mut self) {
        let _ = self.release_power();
    }
}

impl Platform for MacPlatform {
    fn snapshot(&self) -> HostSnapshot {
        let mut snap = base_snapshot(monotonic_ms());
        let (on_ac, battery) = power_source();
        snap.on_ac = on_ac;
        snap.battery_percent = battery;
        snap.lid_closed = lid_closed();
        snap.display_asleep = display_asleep();
        snap.hid_idle_ms = hid_idle_ms();
        snap.thermal = thermal_state();
        snap
    }

    fn apply_power(&mut self, plan: PowerPlan) -> Result<(), String> {
        self.ensure_clamshell_conn();
        SESSION_ACTIVE.store(
            plan.prevent_idle_sleep || plan.disable_clamshell_sleep,
            Ordering::SeqCst,
        );
        let reason = "熄屏待命：保持系统运行供远程客户端连接";

        if plan.prevent_idle_sleep && self.idle_id.is_none() {
            self.idle_id = create_assertion("PreventUserIdleSystemSleep", reason);
        }
        if !plan.prevent_idle_sleep {
            release_assertion(&mut self.idle_id);
        }

        if plan.prevent_system_sleep && self.system_id.is_none() {
            self.system_id = create_assertion("PreventSystemSleep", reason);
        }
        if !plan.prevent_system_sleep {
            release_assertion(&mut self.system_id);
        }

        if plan.prevent_disk_idle && self.disk_id.is_none() {
            self.disk_id = create_assertion("PreventDiskIdle", reason);
        }
        if !plan.prevent_disk_idle {
            release_assertion(&mut self.disk_id);
        }

        if plan.network_client && self.network_id.is_none() {
            self.network_id = create_assertion("NetworkClientActive", reason);
        }
        if !plan.network_client {
            release_assertion(&mut self.network_id);
        }

        if plan.disable_clamshell_sleep {
            set_clamshell_sleep_disabled(true);
            self.clamshell_on = true;
        } else if self.clamshell_on {
            set_clamshell_sleep_disabled(false);
            self.clamshell_on = false;
        }

        if plan.prevent_idle_sleep || plan.disable_clamshell_sleep {
            write_lock(self.clamshell_on);
        } else {
            let _ = fs::remove_file(session_lock_path());
        }

        if plan.prevent_idle_sleep && self.idle_id.is_none() {
            return Err("无法创建 PreventUserIdleSystemSleep 断言".into());
        }
        Ok(())
    }

    fn release_power(&mut self) -> Result<(), String> {
        SESSION_ACTIVE.store(false, Ordering::SeqCst);
        release_assertion(&mut self.idle_id);
        release_assertion(&mut self.system_id);
        release_assertion(&mut self.disk_id);
        release_assertion(&mut self.network_id);
        if self.clamshell_on || CLAMSHELL_CONNECT.load(Ordering::SeqCst) != 0 {
            set_clamshell_sleep_disabled(false);
            self.clamshell_on = false;
        }
        let _ = fs::remove_file(session_lock_path());
        Ok(())
    }

    fn sleep_display(&self) -> Result<(), String> {
        let status = Command::new("pmset")
            .arg("displaysleepnow")
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            let service = matching_service("IODisplayWrangler");
            if service == 0 {
                return Err("pmset displaysleepnow 失败，且找不到 IODisplayWrangler".into());
            }
            let key = cf_string("IORequestIdle");
            unsafe {
                let _ = IORegistryEntrySetCFProperty(service, key, kCFBooleanTrue);
                let _ = IOObjectRelease(service);
            }
            cf_release(key);
        }
        Ok(())
    }

    fn lock_session(&self) {
        // Ctrl+Cmd+Q。可能需要「系统设置 → 隐私 → 自动化」授权给本应用。
        let _ = Command::new("osascript")
            .arg("-e")
            .arg("tell application \"System Events\" to keystroke \"q\" using {command down, control down}")
            .status();
    }

    fn notify(&self, title: &str, body: &str) {
        let script = format!(
            "display notification \"{}\" with title \"{}\"",
            escape_as(body),
            escape_as(title)
        );
        let _ = Command::new("osascript").arg("-e").arg(script).status();
    }

    fn set_launch_at_login(&self, enabled: bool) -> Result<(), String> {
        let plist_path = launch_agent_path();
        if !enabled {
            let _ = Command::new("launchctl")
                .args(["unload", &plist_path.to_string_lossy()])
                .status();
            let _ = fs::remove_file(&plist_path);
            return Ok(());
        }
        if let Some(parent) = plist_path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let exe = current_exe();
        let plist = if let Some(app) = app_bundle_from_exe(&exe) {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.seasonxue.never-sleep</string>
  <key>RunAtLoad</key><true/>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
  <key>ProgramArguments</key>
  <array>
    <string>/usr/bin/open</string>
    <string>-ga</string>
    <string>{}</string>
    <string>--args</string>
    <string>--menubar</string>
  </array>
</dict></plist>
"#,
                app.display()
            )
        } else {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>com.seasonxue.never-sleep</string>
  <key>RunAtLoad</key><true/>
  <key>LimitLoadToSessionType</key><string>Aqua</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>--menubar</string>
  </array>
</dict></plist>
"#,
                exe.display()
            )
        };
        fs::write(&plist_path, plist).map_err(|e| e.to_string())?;
        let _ = Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .status();
        let st = Command::new("launchctl")
            .args(["load", &plist_path.to_string_lossy()])
            .status()
            .map_err(|e| e.to_string())?;
        if !st.success() {
            return Err("launchctl load 失败".into());
        }
        Ok(())
    }

    fn cleanup_orphans(&self) {
        self.ensure_clamshell_conn();
        if let Some((pid, clamshell)) = parse_lock() {
            if pid != std::process::id() && !pid_alive(pid) {
                if clamshell {
                    set_clamshell_sleep_disabled(false);
                }
                let _ = fs::remove_file(session_lock_path());
            }
        }
    }

    fn doctor(&self) -> String {
        let snap = self.snapshot();
        let mut out = String::new();
        out.push_str("熄屏待命诊断\n");
        out.push_str(&format!(
            "电源: {}\n电量: {:?}\n合盖: {}\n屏幕休眠: {:?}\nHID空闲: {} ms\n过热: {:?}\n",
            if snap.on_ac { "AC" } else { "电池" },
            snap.battery_percent,
            snap.lid_closed,
            snap.display_asleep,
            snap.hid_idle_ms,
            snap.thermal
        ));
        if let Ok(o) = Command::new("pmset").args(["-g", "assertions"]).output() {
            out.push_str("\n--- pmset -g assertions ---\n");
            out.push_str(&String::from_utf8_lossy(&o.stdout));
        }
        if let Ok(o) = Command::new("pmset").args(["-g", "batt"]).output() {
            out.push_str("\n--- pmset -g batt ---\n");
            out.push_str(&String::from_utf8_lossy(&o.stdout));
        }
        out
    }
}

fn app_bundle_from_exe(exe: &std::path::Path) -> Option<std::path::PathBuf> {
    // Foo.app/Contents/MacOS/never-sleep
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let app = contents.parent()?;
    if app.extension()?.to_str()? == "app" {
        Some(app.to_path_buf())
    } else {
        None
    }
}
