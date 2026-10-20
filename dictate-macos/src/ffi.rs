//! The C surface of CoreGraphics, CoreFoundation and the Accessibility API that
//! dictate uses, declared from Apple's headers.

#![allow(non_upper_case_globals, dead_code)]

use std::ffi::c_void;

pub type CFTypeRef = *const c_void;
pub type CFStringRef = *const c_void;
pub type CFArrayRef = *const c_void;
pub type CFDictionaryRef = *const c_void;
pub type CFNumberRef = *const c_void;
pub type CFMachPortRef = *const c_void;
pub type CFRunLoopRef = *const c_void;
pub type CFRunLoopSourceRef = *const c_void;
pub type CFIndex = isize;
pub type CGEventRef = *const c_void;
pub type CGEventTapProxy = *const c_void;
pub type CGEventMask = u64;
pub type CGEventType = u32;
pub type CGEventFlags = u64;
pub type CGKeyCode = u16;
pub type AXUIElementRef = *const c_void;

pub type CGEventTapCallBack =
    extern "C" fn(proxy: CGEventTapProxy, kind: CGEventType, event: CGEventRef, user_info: *mut c_void) -> CGEventRef;

// CGEventType
pub const kCGEventKeyDown: CGEventType = 10;
pub const kCGEventKeyUp: CGEventType = 11;
pub const kCGEventFlagsChanged: CGEventType = 12;
pub const kCGEventTapDisabledByTimeout: CGEventType = 0xFFFF_FFFE;
pub const kCGEventTapDisabledByUserInput: CGEventType = 0xFFFF_FFFF;

// CGEventField
pub const kCGKeyboardEventAutorepeat: u32 = 8;
pub const kCGKeyboardEventKeycode: u32 = 9;
pub const kCGEventSourceUnixProcessID: u32 = 41;
pub const kCGEventSourceUserData: u32 = 42;

// CGEventTapLocation, CGEventTapPlacement, CGEventTapOptions
pub const kCGHIDEventTap: u32 = 0;
pub const kCGSessionEventTap: u32 = 1;
pub const kCGHeadInsertEventTap: u32 = 0;
pub const kCGEventTapOptionListenOnly: u32 = 1;

// CGEventSourceStateID
pub const kCGEventSourceStateHIDSystemState: i32 = 1;

// CGWindowListOption
pub const kCGWindowListOptionOnScreenOnly: u32 = 1 << 0;
pub const kCGWindowListExcludeDesktopElements: u32 = 1 << 4;

pub const kCFNumberSInt64Type: CFIndex = 4;
pub const kCFStringEncodingUTF8: u32 = 0x0800_0100;
pub const kAXErrorSuccess: i32 = 0;

#[repr(C)]
pub struct CFDictionaryKeyCallBacks {
    _opaque: [u8; 0],
}

#[repr(C)]
pub struct CFDictionaryValueCallBacks {
    _opaque: [u8; 0],
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    pub static kCFRunLoopCommonModes: CFStringRef;
    pub static kCFBooleanTrue: CFTypeRef;
    pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
    pub static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;

    pub fn CFRelease(value: CFTypeRef);
    pub fn CFGetTypeID(value: CFTypeRef) -> usize;
    pub fn CFStringGetTypeID() -> usize;
    pub fn CFStringCreateWithBytes(
        allocator: CFTypeRef,
        bytes: *const u8,
        length: CFIndex,
        encoding: u32,
        is_external_representation: u8,
    ) -> CFStringRef;
    pub fn CFStringGetLength(string: CFStringRef) -> CFIndex;
    pub fn CFStringGetMaximumSizeForEncoding(length: CFIndex, encoding: u32) -> CFIndex;
    pub fn CFStringGetCString(string: CFStringRef, buffer: *mut u8, size: CFIndex, encoding: u32) -> u8;
    pub fn CFArrayGetCount(array: CFArrayRef) -> CFIndex;
    pub fn CFArrayGetValueAtIndex(array: CFArrayRef, index: CFIndex) -> CFTypeRef;
    pub fn CFDictionaryGetValue(dictionary: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    pub fn CFDictionaryCreate(
        allocator: CFTypeRef,
        keys: *const CFTypeRef,
        values: *const CFTypeRef,
        count: CFIndex,
        key_callbacks: *const CFDictionaryKeyCallBacks,
        value_callbacks: *const CFDictionaryValueCallBacks,
    ) -> CFDictionaryRef;
    pub fn CFNumberGetValue(number: CFNumberRef, kind: CFIndex, value: *mut c_void) -> u8;
    pub fn CFMachPortCreateRunLoopSource(allocator: CFTypeRef, port: CFMachPortRef, order: CFIndex)
        -> CFRunLoopSourceRef;
    pub fn CFRunLoopGetCurrent() -> CFRunLoopRef;
    pub fn CFRunLoopAddSource(run_loop: CFRunLoopRef, source: CFRunLoopSourceRef, mode: CFStringRef);
    pub fn CFRunLoopRun();
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    pub static kCGWindowOwnerPID: CFStringRef;
    pub static kCGWindowOwnerName: CFStringRef;
    pub static kCGWindowNumber: CFStringRef;
    pub static kCGWindowLayer: CFStringRef;

    pub fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: CGEventMask,
        callback: CGEventTapCallBack,
        user_info: *mut c_void,
    ) -> CFMachPortRef;
    pub fn CGEventTapEnable(tap: CFMachPortRef, enable: bool);
    pub fn CGEventGetIntegerValueField(event: CGEventRef, field: u32) -> i64;
    pub fn CGEventSetIntegerValueField(event: CGEventRef, field: u32, value: i64);
    pub fn CGEventGetFlags(event: CGEventRef) -> CGEventFlags;
    pub fn CGEventSetFlags(event: CGEventRef, flags: CGEventFlags);
    pub fn CGEventCreateKeyboardEvent(source: CFTypeRef, key: CGKeyCode, key_down: bool) -> CGEventRef;
    pub fn CGEventKeyboardSetUnicodeString(event: CGEventRef, length: usize, string: *const u16);
    pub fn CGEventPost(tap: u32, event: CGEventRef);
    pub fn CGEventSourceKeyState(state: i32, key: CGKeyCode) -> bool;
    pub fn CGEventSourceFlagsState(state: i32) -> CGEventFlags;
    pub fn CGPreflightListenEventAccess() -> bool;
    pub fn CGRequestListenEventAccess() -> bool;
    pub fn CGWindowListCopyWindowInfo(option: u32, relative_to_window: u32) -> CFArrayRef;
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    pub static kAXTrustedCheckOptionPrompt: CFStringRef;

    pub fn AXIsProcessTrusted() -> bool;
    pub fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    pub fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    pub fn AXUIElementCopyAttributeValue(element: AXUIElementRef, attribute: CFStringRef, value: *mut CFTypeRef)
        -> i32;
}

/// A CFString made from a Rust string. Released by the caller.
pub fn cf_string(text: &str) -> CFStringRef {
    unsafe { CFStringCreateWithBytes(std::ptr::null(), text.as_ptr(), text.len() as CFIndex, kCFStringEncodingUTF8, 0) }
}

/// The text of a CFString, or None for anything that is not one.
pub fn rust_string(value: CFTypeRef) -> Option<String> {
    if value.is_null() {
        return None;
    }

    unsafe {
        if CFGetTypeID(value) != CFStringGetTypeID() {
            return None;
        }

        let size = CFStringGetMaximumSizeForEncoding(CFStringGetLength(value), kCFStringEncodingUTF8) + 1;
        let mut buffer = vec![0u8; size.max(1) as usize];
        if CFStringGetCString(value, buffer.as_mut_ptr(), buffer.len() as CFIndex, kCFStringEncodingUTF8) == 0 {
            return None;
        }

        let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
        Some(String::from_utf8_lossy(&buffer[..end]).into_owned())
    }
}

/// A CFNumber as an i64, or None for anything that is not one.
pub fn number(value: CFTypeRef) -> Option<i64> {
    if value.is_null() {
        return None;
    }

    let mut out: i64 = 0;
    let ok = unsafe { CFNumberGetValue(value, kCFNumberSInt64Type, &mut out as *mut i64 as *mut c_void) };
    (ok != 0).then_some(out)
}
