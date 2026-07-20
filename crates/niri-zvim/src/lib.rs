mod config;
mod control;
mod daemon;
mod doctor;
mod niri;
mod niri_bindings;
mod socket;
mod zellij;

pub use config::{Config, config_path};
pub use control::request_status;
pub use daemon::run_daemon;
pub use doctor::{DoctorCheck, DoctorLevel, DoctorReport, doctor_report};
pub use niri_bindings::{
    BindingInstall, BindingRestore, binding_manifest_path, install_hjkl_bindings,
    restore_hjkl_bindings,
};
pub use socket::socket_path;
pub use zellij::run_forwarded_bridge;
