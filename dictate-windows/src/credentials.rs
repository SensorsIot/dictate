//! API keys in Windows Credential Manager, DPAPI-encrypted under the logged-in
//! user. Chosen over a .env file so there is no plaintext key sitting on a
//! daily-driver machine. The target names are part of the stored data:
//! changing them orphans every key a user has already saved.

use std::ptr::{self, null_mut};

use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Security::Credentials::{
    CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
};

use crate::win::wide;

pub const ELEVENLABS: &str = "dictate:elevenlabs";
pub const ANTHROPIC: &str = "dictate:anthropic";

pub fn read(target: &str) -> Option<String> {
    let target = wide(target);
    let mut credential: *mut CREDENTIALW = null_mut();

    unsafe {
        if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) == 0 || credential.is_null() {
            return None;
        }

        let stored = &*credential;
        let value = if stored.CredentialBlobSize == 0 || stored.CredentialBlob.is_null() {
            None
        } else {
            let bytes = std::slice::from_raw_parts(stored.CredentialBlob, stored.CredentialBlobSize as usize);
            Some(String::from_utf8_lossy(bytes).into_owned())
        };

        CredFree(credential as *const _);
        value.filter(|v| !v.trim().is_empty())
    }
}

pub fn write(target: &str, secret: &str) -> Result<(), String> {
    let mut target_name = wide(target);
    let mut user_name = wide(&std::env::var("USERNAME").unwrap_or_default());
    let mut blob = secret.as_bytes().to_vec();

    let (stored, error) = unsafe {
        let mut credential: CREDENTIALW = std::mem::zeroed();
        credential.Type = CRED_TYPE_GENERIC;
        credential.TargetName = target_name.as_mut_ptr();
        credential.CredentialBlobSize = blob.len() as u32;
        credential.CredentialBlob = blob.as_mut_ptr();
        credential.Persist = CRED_PERSIST_LOCAL_MACHINE;
        credential.UserName = user_name.as_mut_ptr();

        let stored = CredWriteW(&credential, 0) != 0;
        (stored, GetLastError())
    };

    // Wipe our copy of the key. Volatile so the optimiser cannot decide a
    // buffer about to be freed does not need writing.
    for byte in blob.iter_mut() {
        unsafe { ptr::write_volatile(byte, 0) };
    }

    if stored {
        Ok(())
    } else {
        Err(format!("Could not store '{target}' in Credential Manager (Win32 error {error})."))
    }
}

pub fn delete(target: &str) {
    let target = wide(target);
    unsafe {
        CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0);
    }
}
