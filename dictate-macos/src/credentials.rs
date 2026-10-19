//! API keys in the login Keychain, encrypted under the user's login. Not a
//! file, so there is no plaintext key sitting on the machine. The service and
//! account names are part of the stored data: changing them orphans every key
//! a user has already saved.

use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};

const SERVICE: &str = "dictate";
pub const ELEVENLABS: &str = "elevenlabs";
pub const ANTHROPIC: &str = "anthropic";

pub fn read(account: &str) -> Option<String> {
    let bytes = get_generic_password(SERVICE, account).ok()?;
    let value = String::from_utf8_lossy(&bytes).into_owned();
    (!value.trim().is_empty()).then_some(value)
}

pub fn write(account: &str, secret: &str) -> Result<(), String> {
    set_generic_password(SERVICE, account, secret.as_bytes())
        .map_err(|error| format!("Could not store the {account} key in the Keychain: {error}"))
}

pub fn delete(account: &str) {
    let _ = delete_generic_password(SERVICE, account);
}
