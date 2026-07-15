mod daemon;
mod niri;
mod socket;

pub use daemon::run_daemon;
pub use socket::{adapter_magic, socket_path};
