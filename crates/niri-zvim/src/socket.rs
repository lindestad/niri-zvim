use std::path::PathBuf;

pub const fn adapter_magic() -> u8 {
    0x7f
}

pub fn socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRI_ZVIM_SOCKET") {
        return path.into();
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    runtime.join("niri-zvim.sock")
}
