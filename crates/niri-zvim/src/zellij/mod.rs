use std::collections::{BTreeMap, BTreeSet};

use niri_zvim_core::NiriWindow;
use tokio::{
    sync::{mpsc, watch},
    time::{Duration, sleep},
};
use tracing::warn;

use crate::{config::ZellijDiscovery, daemon::DaemonEvent};

use self::{bridge::run_bridge, metadata::session_socket_exists};

mod bridge;
mod forward;
mod metadata;
mod snapshot;

pub use forward::run_forwarded_bridge;

pub(crate) fn configured_plugin_path() -> std::path::PathBuf {
    metadata::plugin_path()
}

pub struct BridgeManager {
    discovery: ZellijDiscovery,
    sessions: BTreeMap<String, watch::Sender<Vec<u64>>>,
    retiring: BTreeSet<String>,
}

impl BridgeManager {
    pub fn new(discovery: ZellijDiscovery) -> Self {
        Self {
            discovery,
            sessions: BTreeMap::new(),
            retiring: BTreeSet::new(),
        }
    }

    pub fn configure(&mut self, discovery: ZellijDiscovery) {
        self.discovery = discovery;
    }

    pub fn observe(&mut self, windows: &[NiriWindow], events: &mpsc::Sender<DaemonEvent>) {
        let mut discovered = BTreeMap::<String, Vec<u64>>::new();
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
            if !session_socket_exists(session) {
                continue;
            }
            discovered
                .entry(session.to_owned())
                .or_default()
                .push(window.id);
        }

        for window_ids in discovered.values_mut() {
            window_ids.sort_unstable();
            window_ids.dedup();
        }

        let stopped: Vec<_> = self
            .sessions
            .keys()
            .filter(|session| !discovered.contains_key(*session))
            .cloned()
            .collect();
        for session in stopped {
            if self.sessions.remove(&session).is_some() {
                self.retiring.insert(session);
            }
        }

        for (session, window_ids) in discovered {
            if self.retiring.contains(&session) {
                continue;
            }
            if let Some(windows) = self.sessions.get(&session) {
                windows.send_if_modified(|current| {
                    if *current == window_ids {
                        false
                    } else {
                        *current = window_ids.clone();
                        true
                    }
                });
                continue;
            }

            let (window_updates, windows) = watch::channel(window_ids);
            self.sessions.insert(session.clone(), window_updates);
            let events = events.clone();
            tokio::spawn(async move {
                loop {
                    if let Err(error) = run_bridge(&session, windows.clone(), events.clone()).await
                    {
                        warn!(%session, %error, "Zellij bridge stopped");
                    }
                    if windows.has_changed().is_err() || !session_socket_exists(&session) {
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

    pub fn bridge_stopped(&mut self, session: &str) -> bool {
        self.retiring.remove(session) || self.sessions.remove(session).is_some()
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

    #[test]
    fn retiring_session_blocks_replacement_until_old_bridge_stops() {
        let mut manager = BridgeManager::new(ZellijDiscovery::default());
        let (windows, _receiver) = watch::channel(vec![7]);
        manager.sessions.insert("dev-session".into(), windows);
        let (events, _received) = mpsc::channel(1);

        manager.observe(&[], &events);

        assert!(!manager.sessions.contains_key("dev-session"));
        assert!(manager.retiring.contains("dev-session"));
        assert!(manager.bridge_stopped("dev-session"));
        assert!(!manager.retiring.contains("dev-session"));
        assert!(!manager.bridge_stopped("dev-session"));
    }
}
