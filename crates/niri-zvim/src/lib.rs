mod config;
mod daemon;
mod niri;
mod socket;
mod zellij;

pub use daemon::run_daemon;
pub use socket::socket_path;
