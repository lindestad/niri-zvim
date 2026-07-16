use std::collections::BTreeSet;

use niri_zvim_core::NiriWindow;
use tokio::{
    sync::mpsc,
    time::{Duration, sleep},
};
use tracing::warn;

use crate::daemon::DaemonEvent;

use self::{bridge::run_bridge, metadata::session_socket_exists};

mod bridge;
mod metadata;
mod snapshot;

#[derive(Default)]
pub struct BridgeManager {
    sessions: BTreeSet<String>,
}

impl BridgeManager {
    pub fn observe(&mut self, windows: &[NiriWindow], events: &mpsc::Sender<DaemonEvent>) {
        for window in windows {
            if window.app_id.as_deref() != Some("com.mitchellh.ghostty") {
                continue;
            }
            let Some(title) = window.title.as_deref() else {
                continue;
            };
            let session = session_from_title(title);
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

fn session_from_title(title: &str) -> &str {
    title
        .split_once(" | ")
        .map_or(title, |(session, _command)| session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_session_from_zellij_terminal_title() {
        assert_eq!(
            session_from_title("dev-session | nvim src/main.rs"),
            "dev-session"
        );
        assert_eq!(session_from_title("dev-session"), "dev-session");
    }
}
