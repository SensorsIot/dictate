//! A listen-only Quartz event tap watching one key, reporting press and release
//! to the main thread.
//!
//! The tap runs on its own thread and run loop, so a busy main thread never
//! delays an edge. Listen-only means the key is never swallowed: the default
//! hotkey, Right Command, does nothing on its own, so there is nothing to
//! suppress, and a listen-only tap needs Input Monitoring rather than control of
//! the whole keyboard.
//!
//! **A press is not guaranteed to produce a release.** The system stops
//! delivering keyboard events to taps while a password field has secure input,
//! and disables a tap it considers slow. So a watchdog compares the tap's idea
//! of the key with the hardware state and supplies the release that never came.

use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::ptr::null_mut;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU16, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use dictate_core::StopReason;

use crate::app;
use crate::ffi::*;
use crate::injector::INJECTION_SIGNATURE;

static KEYCODE: AtomicU16 = AtomicU16::new(0);
/// The device-dependent flag bit of a modifier hotkey; 0 for an ordinary key.
static FLAG: AtomicU64 = AtomicU64::new(0);
static DOWN: AtomicBool = AtomicBool::new(false);
static TAP: AtomicPtr<c_void> = AtomicPtr::new(null_mut());

const WATCHDOG_PERIOD: Duration = Duration::from_millis(100);
/// Consecutive watchdog reads of "up" before a release is supplied.
const WATCHDOG_READS: u32 = 3;
const RETRY_PERIOD: Duration = Duration::from_secs(2);

/// A macOS key code, and the device flag bit when the key is a modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacKey {
    pub keycode: u16,
    pub flag: u64,
}

/// The configuration names keys by Windows virtual-key code, so one config.json
/// means the same physical key on both systems. A Windows keyboard's Windows
/// keys are Command on a Mac, and Alt is Option.
const KEYS: &[(i32, u16, u64)] = &[
    (0x5C, 54, 0x10),   // Right Windows -> Right Command
    (0x5B, 55, 0x08),   // Left Windows -> Left Command
    (0xA3, 62, 0x2000), // Right Ctrl
    (0xA2, 59, 0x01),   // Left Ctrl
    (0xA5, 61, 0x40),   // Right Alt -> Right Option
    (0xA4, 58, 0x20),   // Left Alt -> Left Option
    (0xA1, 60, 0x04),   // Right Shift
    (0xA0, 56, 0x02),   // Left Shift
    (0x70, 122, 0),     // F1
    (0x71, 120, 0),
    (0x72, 99, 0),
    (0x73, 118, 0),
    (0x74, 96, 0),
    (0x75, 97, 0),
    (0x76, 98, 0),
    (0x77, 100, 0),
    (0x78, 101, 0),
    (0x79, 109, 0),
    (0x7A, 103, 0),
    (0x7B, 111, 0), // F12
    (0x7C, 105, 0), // F13
    (0x7D, 107, 0),
    (0x7E, 113, 0),
    (0x7F, 106, 0),
    (0x80, 64, 0),
    (0x81, 79, 0),
    (0x82, 80, 0),
    (0x83, 90, 0), // F20
];

pub fn mac_key(virtual_key: i32) -> Option<MacKey> {
    KEYS.iter().find(|(vk, _, _)| *vk == virtual_key).map(|&(_, keycode, flag)| MacKey { keycode, flag })
}

/// Starts the tap thread and the watchdog. Returns at once; if Input
/// Monitoring has not been granted yet, the tap thread says so and keeps trying.
pub fn start(key: MacKey) {
    KEYCODE.store(key.keycode, Ordering::SeqCst);
    FLAG.store(key.flag, Ordering::SeqCst);

    thread::Builder::new().name("hotkey".into()).spawn(run_tap).ok();
    thread::Builder::new().name("hotkey-watchdog".into()).spawn(watchdog).ok();
}

fn run_tap() {
    let mask: CGEventMask = (1 << kCGEventKeyDown) | (1 << kCGEventKeyUp) | (1 << kCGEventFlagsChanged);
    let mut told = false;

    loop {
        let tap = unsafe {
            CGEventTapCreate(kCGSessionEventTap, kCGHeadInsertEventTap, kCGEventTapOptionListenOnly, mask, on_event, null_mut())
        };

        if !tap.is_null() {
            TAP.store(tap as *mut c_void, Ordering::SeqCst);
            app::post(|| app::log_event("hotkey.tap", true));
            unsafe {
                let source = CFMachPortCreateRunLoopSource(std::ptr::null(), tap, 0);
                CFRunLoopAddSource(CFRunLoopGetCurrent(), source, kCFRunLoopCommonModes);
                CGEventTapEnable(tap, true);
                CFRunLoopRun();
            }
            return;
        }

        if !told {
            told = true;
            unsafe { CGRequestListenEventAccess() };
            app::post(|| {
                app::log_event("hotkey.tap", false);
                app::notify(
                    "Allow Input Monitoring",
                    "System Settings → Privacy & Security → Input Monitoring: switch on dictate. dictate starts listening as soon as it is allowed.",
                );
            });
        }
        thread::sleep(RETRY_PERIOD);
    }
}

extern "C" fn on_event(_proxy: CGEventTapProxy, kind: CGEventType, event: CGEventRef, _info: *mut c_void) -> CGEventRef {
    // A panic must not unwind into CoreGraphics.
    let _ = panic::catch_unwind(AssertUnwindSafe(|| handle(kind, event)));
    event
}

fn handle(kind: CGEventType, event: CGEventRef) {
    if kind == kCGEventTapDisabledByTimeout || kind == kCGEventTapDisabledByUserInput {
        let tap = TAP.load(Ordering::SeqCst);
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, true) };
        }
        return;
    }

    let (keycode, user_data, source_pid, flags) = unsafe {
        (
            CGEventGetIntegerValueField(event, kCGKeyboardEventKeycode) as u16,
            CGEventGetIntegerValueField(event, kCGEventSourceUserData),
            CGEventGetIntegerValueField(event, kCGEventSourceUnixProcessID),
            CGEventGetFlags(event),
        )
    };

    if keycode != KEYCODE.load(Ordering::SeqCst) || user_data == INJECTION_SIGNATURE {
        return;
    }

    #[allow(non_upper_case_globals)] // Apple's constant names, matched as patterns
    let pressed = match kind {
        kCGEventFlagsChanged => flags & FLAG.load(Ordering::SeqCst) != 0,
        // Auto-repeat arrives as further key-downs; DOWN is already set, so
        // they produce no edge (FR-8.2).
        kCGEventKeyDown => true,
        kCGEventKeyUp => false,
        _ => return,
    };

    let was_down = DOWN.swap(pressed, Ordering::SeqCst);
    if pressed && !was_down {
        let pressed_at = Instant::now();
        let injected = source_pid != 0;
        app::post(move || app::on_press(pressed_at, injected));
    } else if !pressed && was_down {
        app::post(|| app::on_release(StopReason::Released));
    }
}

/// FR-8.7: a press whose release never arrives is ended once the hardware has
/// read the key as up several times running. A key still held is never
/// released, because the hardware state is read, not inferred.
fn watchdog() {
    let mut up_reads = 0;

    loop {
        thread::sleep(WATCHDOG_PERIOD);

        if !DOWN.load(Ordering::SeqCst) {
            up_reads = 0;
            continue;
        }

        up_reads = if held() { 0 } else { up_reads + 1 };

        if up_reads >= WATCHDOG_READS && DOWN.swap(false, Ordering::SeqCst) {
            up_reads = 0;
            app::post(|| app::on_release(StopReason::Reconciled));
        }
    }
}

/// The hardware's view of the hotkey. The key state reports both sides of a
/// modifier under the left key's code, so a right-side modifier would always
/// read as up; its device flag is the only reading that tells the sides apart.
fn held() -> bool {
    let flag = FLAG.load(Ordering::SeqCst);
    if flag != 0 {
        unsafe { CGEventSourceFlagsState(kCGEventSourceStateHIDSystemState) & flag != 0 }
    } else {
        unsafe { CGEventSourceKeyState(kCGEventSourceStateHIDSystemState, KEYCODE.load(Ordering::SeqCst)) }
    }
}
