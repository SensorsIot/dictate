use std::path::PathBuf;

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_folder() -> PathBuf {
    home().join("Library/Application Support/dictate")
}

pub fn config_file() -> PathBuf {
    config_folder().join("config.json")
}

pub fn log_file() -> PathBuf {
    home().join("Library/Logs/dictate/dictate.log")
}

/// Held locked while dictate runs (FR-5.4).
pub fn instance_lock() -> PathBuf {
    config_folder().join("dictate.lock")
}
