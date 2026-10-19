//! "Start with Windows" (FR-16.3): dictate in the current user's Run key.
//!
//! Per-user, so no elevation is needed and dictate starts unelevated, which it
//! must (an elevated hook cannot see input bound for ordinary windows). Off
//! until the user ticks it: a program should not add itself to startup unasked.
//!
//! Two other things also decide whether dictate starts at login, and the
//! checkbox has to agree with both or it would show one thing and do another:
//!   * a shortcut in the Startup folder, which is how it was set up by hand
//!     before the checkbox existed;
//!   * Task Manager's "Disabled" switch, kept under StartupApproved.

use std::ffi::c_void;
use std::path::PathBuf;
use std::ptr::null_mut;

use windows_sys::Win32::System::Registry::{RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER};

use crate::win::wide;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const APPROVED_RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";
const APPROVED_FOLDER: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\StartupFolder";
const VALUE_NAME: &str = "dictate";

const REG_SZ: u32 = 1;
const RRF_RT_REG_SZ: u32 = 0x0000_0002;
const RRF_RT_REG_BINARY: u32 = 0x0000_0008;

/// Whether dictate will start at the next login, by any of the routes above.
pub fn is_enabled() -> bool {
    let by_run_key = run_entry().is_some() && !disabled_in_task_manager(APPROVED_RUN, VALUE_NAME);
    let by_shortcut = startup_shortcuts()
        .iter()
        .any(|(name, _)| !disabled_in_task_manager(APPROVED_FOLDER, name));

    by_run_key || by_shortcut
}

pub fn set_enabled(enabled: bool) -> Result<(), String> {
    if enabled {
        enable()
    } else {
        disable()
    }
}

fn enable() -> Result<(), String> {
    // An enabled Startup-folder shortcut already does the job; a Run entry as
    // well would start dictate twice.
    if startup_shortcuts().iter().any(|(name, _)| !disabled_in_task_manager(APPROVED_FOLDER, name)) {
        return Ok(());
    }

    // Always rewritten rather than left alone, so ticking the box after moving
    // the exe repairs the path.
    let exe = std::env::current_exe().map_err(|e| format!("Could not find dictate's own path: {e}"))?;
    let command = format!("\"{}\"", exe.display());
    set_string(RUN_KEY, VALUE_NAME, &command)?;

    // A "Disabled" left behind in Task Manager would otherwise keep the new
    // entry switched off while the menu shows it on.
    delete_value(APPROVED_RUN, VALUE_NAME);
    Ok(())
}

fn disable() -> Result<(), String> {
    delete_value(RUN_KEY, VALUE_NAME);
    delete_value(APPROVED_RUN, VALUE_NAME);

    for (name, path) in startup_shortcuts() {
        std::fs::remove_file(&path)
            .map_err(|e| format!("Could not remove the startup shortcut {}: {e}", path.display()))?;
        delete_value(APPROVED_FOLDER, &name);
    }

    Ok(())
}

fn run_entry() -> Option<String> {
    let key = wide(RUN_KEY);
    let value = wide(VALUE_NAME);
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;

    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_SZ,
            null_mut(),
            buffer.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };
    if status != 0 {
        return None;
    }

    let units = (size as usize / 2).saturating_sub(1); // size includes the terminator
    Some(String::from_utf16_lossy(&buffer[..units.min(buffer.len())]))
}

/// Task Manager stores a 12-byte value per startup item; an odd first byte
/// means the user disabled it there. No value means enabled.
fn disabled_in_task_manager(key: &str, value: &str) -> bool {
    let key = wide(key);
    let value = wide(value);
    let mut data = [0u8; 12];
    let mut size = data.len() as u32;

    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_BINARY,
            null_mut(),
            data.as_mut_ptr() as *mut c_void,
            &mut size,
        )
    };

    status == 0 && size > 0 && data[0] % 2 == 1
}

/// Shortcuts in the user's Startup folder whose file name mentions dictate.
fn startup_shortcuts() -> Vec<(String, PathBuf)> {
    let Some(appdata) = std::env::var_os("APPDATA") else { return Vec::new() };
    let folder = PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Startup");

    let Ok(entries) = std::fs::read_dir(&folder) else { return Vec::new() };
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            (lower.ends_with(".lnk") && lower.contains("dictate")).then(|| (name, entry.path()))
        })
        .collect()
}

fn set_string(key: &str, value: &str, data: &str) -> Result<(), String> {
    let key = wide(key);
    let value = wide(value);
    let data = wide(data);

    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            REG_SZ,
            data.as_ptr() as *const c_void,
            (data.len() * 2) as u32,
        )
    };

    if status == 0 {
        Ok(())
    } else {
        Err(format!("Could not register dictate to start with Windows (error {status})."))
    }
}

/// Missing is fine: the goal is that the value is not there.
fn delete_value(key: &str, value: &str) {
    let key = wide(key);
    let value = wide(value);
    unsafe {
        RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), value.as_ptr());
    }
}
