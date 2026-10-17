use std::path::PathBuf;

fn known_folder(variable: &str) -> PathBuf {
    std::env::var_os(variable).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."))
}

pub fn config_folder() -> PathBuf {
    known_folder("APPDATA").join("dictate")
}

pub fn config_file() -> PathBuf {
    config_folder().join("config.json")
}

/// Under LocalAppData rather than beside the config: a log is machine-local and
/// must never follow a roaming profile onto other machines.
pub fn log_file() -> PathBuf {
    known_folder("LOCALAPPDATA").join("dictate").join("dictate.log")
}
