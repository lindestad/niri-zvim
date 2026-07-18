use std::collections::BTreeSet;

use niri_zvim_core::NiriWindow;
use tokio::{
    sync::mpsc,
    time::{Duration, sleep},
};
use tracing::warn;

use crate::{config::ZellijDiscovery, daemon::DaemonEvent};

use self::{bridge::run_bridge, metadata::session_socket_exists};

mod bridge;
mod metadata;
mod snapshot;

pub(crate) fn configured_plugin_path() -> std::path::PathBuf {
    metadata::plugin_path()
}

pub struct BridgeManager {
    discovery: ZellijDiscovery,
    sessions: BTreeSet<String>,
}

impl BridgeManager {
    pub fn new(discovery: ZellijDiscovery) -> Self {
        Self {
            discovery,
            sessions: BTreeSet::new(),
        }
    }

    pub fn observe(&mut self, windows: &[NiriWindow], events: &mpsc::Sender<DaemonEvent>) {
        for window in windows {
            let Some(app_id) = window.app_id.as_deref() else {
                continue;
            };
            if !self.discovery.matches_app_id(app_id) {
                continue;
            }
            let Some(title) = window.title.as_deref() else {
                continue;
            };
            let session = session_from_title(title, &self.discovery.session_title_separator);
            if !session_socket_exists(session) || !self.sessions.insert(session.to_owned()) {
                continue;
            }
            let events = events.clone();
            let session = session.to_owned();
            let window_id = window.id;
            tokio::spawn(async move {
                loop {
                    if let Err(error) = run_bridge(&session, window_id, events.clone()).await {
                        warn!(%session, %error, "Zellij bridge stopped");
                    }
                    if !session_socket_exists(&session) {
                        break;
                    }
                    sleep(Duration::from_millis(250)).await;
                }
                let _ = events
                    .send(DaemonEvent::ZellijBridgeStopped { session })
                    .await;
            });
        }
    }

    pub fn bridge_stopped(&mut self, session: &str) {
        self.sessions.remove(session);
    }
}

fn session_from_title<'a>(title: &'a str, separator: &str) -> &'a str {
    title
        .split_once(separator)
        .map_or(title, |(session, _command)| session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_session_from_zellij_terminal_title() {
        assert_eq!(
            session_from_title("dev-session | nvim src/main.rs", " | "),
            "dev-session"
        );
        assert_eq!(session_from_title("dev-session", " | "), "dev-session");
        assert_eq!(
            session_from_title("dev-session :: nvim src/main.rs", " :: "),
            "dev-session"
        );
    }

    #[test]
    fn terminal_app_ids_are_exact_and_configurable() {
        let discovery = ZellijDiscovery {
            terminal_app_ids: vec!["org.wezfurlong.wezterm".into()],
            session_title_separator: " :: ".into(),
        };
        assert!(discovery.matches_app_id("org.wezfurlong.wezterm"));
        assert!(!discovery.matches_app_id("com.mitchellh.ghostty"));
    }
}
