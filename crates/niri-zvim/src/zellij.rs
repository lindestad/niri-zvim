use std::{
    collections::BTreeSet,
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use anyhow::Context;
use niri_zvim_core::{
    AdapterMessage, DaemonMessage, NeighborMap, NiriWindow, Rect, ZellijClient, ZellijClientState,
    directional_neighbors,
};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
    time::{Duration, sleep},
};
use tracing::{debug, info, warn};

use crate::daemon::DaemonEvent;

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

async fn run_bridge(
    session: &str,
    window_id: u64,
    events: mpsc::Sender<DaemonEvent>,
) -> anyhow::Result<()> {
    let plugin = plugin_path();
    anyhow::ensure!(plugin.is_file(), "plugin not found at {}", plugin.display());
    let plugin_url = format!("file:{}", plugin.display());
    let status = Command::new("zellij")
        .args([
            "--session",
            session,
            "action",
            "start-or-reload-plugin",
            &plugin_url,
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .with_context(|| format!("could not bootstrap plugin in Zellij session {session}"))?;
    anyhow::ensure!(
        status.success(),
        "could not bootstrap plugin in Zellij session {session}"
    );
    sleep(Duration::from_millis(50)).await;

    let mut child = Command::new("zellij")
        .args(["--session", session, "pipe", "--name", "niri-zvim"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("could not connect to Zellij session {session}"))?;
    let mut stdin = child.stdin.take().context("Zellij pipe has no stdin")?;
    let stdout = child.stdout.take().context("Zellij pipe has no stdout")?;
    let (sink, mut actions) = mpsc::unbounded_channel();
    let revisions = Arc::new(AtomicU64::new(0));

    if let Ok(state) = query_snapshot(session, window_id, 0).await {
        events
            .send(DaemonEvent::Adapter {
                message: AdapterMessage::ZellijSnapshot { state },
                sink: sink.clone(),
            })
            .await?;
    }

    let metadata_watcher = tokio::spawn(watch_session_metadata(
        session.to_owned(),
        window_id,
        events.clone(),
        sink.clone(),
        revisions.clone(),
    ));

    let bind = serde_json::to_vec(&DaemonMessage::BindNiriWindow {
        window_id,
        session: session.to_owned(),
    })?;
    stdin.write_all(&bind).await?;
    stdin.write_u8(b'\n').await?;
    stdin.flush().await?;
    info!(%session, window_id, "connected Zellij bridge");

    let refresh_session = session.to_owned();
    let refresh_events = events.clone();
    let refresh_sink = sink.clone();
    let action_revisions = revisions.clone();
    tokio::spawn(async move {
        while let Some(message) = actions.recv().await {
            let should_refresh = match &message {
                DaemonMessage::Navigate { .. } => true,
                DaemonMessage::BindNiriWindow { .. } => false,
            };
            let Ok(mut encoded) = serde_json::to_vec(&message) else {
                continue;
            };
            encoded.push(b'\n');
            if stdin.write_all(&encoded).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
            if should_refresh {
                let revision = action_revisions.fetch_add(1, Ordering::Relaxed) + 1;
                let session = refresh_session.clone();
                let events = refresh_events.clone();
                let sink = refresh_sink.clone();
                tokio::spawn(async move {
                    sleep(Duration::from_millis(20)).await;
                    if let Ok(mut state) = query_snapshot(&session, window_id, 0).await {
                        state.revision = revision;
                        let _ = events
                            .send(DaemonEvent::Adapter {
                                message: AdapterMessage::ZellijSnapshot { state },
                                sink,
                            })
                            .await;
                    }
                });
            }
        }
    });

    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        match serde_json::from_str::<AdapterMessage>(&line) {
            Ok(AdapterMessage::ZellijSnapshot { mut state }) => {
                state.client.client_id = 0;
                state.niri_window_id = window_id;
                state.revision = revisions.fetch_add(1, Ordering::Relaxed) + 1;
                events
                    .send(DaemonEvent::Adapter {
                        message: AdapterMessage::ZellijSnapshot { state },
                        sink: sink.clone(),
                    })
                    .await?;
            }
            Ok(message) => {
                events
                    .send(DaemonEvent::Adapter {
                        message,
                        sink: sink.clone(),
                    })
                    .await?;
            }
            Err(error) => debug!(%error, %line, "ignored invalid Zellij bridge output"),
        }
    }
    let status = child.wait().await?;
    metadata_watcher.abort();
    anyhow::bail!("Zellij pipe exited with {status}")
}

async fn watch_session_metadata(
    session: String,
    window_id: u64,
    events: mpsc::Sender<DaemonEvent>,
    sink: mpsc::UnboundedSender<DaemonMessage>,
    revisions: Arc<AtomicU64>,
) {
    let metadata = session_metadata_path(&session);
    let mut last_topology = metadata_topology_stamp(&metadata);
    loop {
        sleep(Duration::from_millis(50)).await;
        let topology = metadata_topology_stamp(&metadata);
        if topology == last_topology {
            continue;
        }
        sleep(Duration::from_millis(20)).await;
        last_topology = metadata_topology_stamp(&metadata);
        let Ok(mut state) = query_snapshot(&session, window_id, 0).await else {
            continue;
        };
        state.revision = revisions.fetch_add(1, Ordering::Relaxed) + 1;
        if events
            .send(DaemonEvent::Adapter {
                message: AdapterMessage::ZellijSnapshot { state },
                sink: sink.clone(),
            })
            .await
            .is_err()
        {
            break;
        }
    }
}

fn session_metadata_path(session: &str) -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    cache
        .join("zellij/contract_version_1/session_info")
        .join(session)
        .join("session-metadata.kdl")
}

fn metadata_topology_stamp(path: &Path) -> Option<u64> {
    let contents = fs::read_to_string(path).ok()?;
    Some(metadata_topology_fingerprint(&contents))
}

fn metadata_topology_fingerprint(contents: &str) -> u64 {
    const STATE_FIELDS: &[&str] = &[
        "position ",
        "active ",
        "are_floating_panes_visible ",
        "id ",
        "is_plugin ",
        "is_focused ",
        "is_floating ",
        "is_suppressed ",
        "pane_x ",
        "pane_y ",
        "pane_rows ",
        "pane_columns ",
        "is_selectable ",
        "tab_position ",
    ];
    let mut hasher = DefaultHasher::new();
    for line in contents.lines().map(str::trim) {
        if STATE_FIELDS.iter().any(|field| line.starts_with(field)) {
            line.hash(&mut hasher);
        }
    }
    hasher.finish()
}

#[derive(Debug, Deserialize)]
struct ListedPane {
    id: u32,
    is_plugin: bool,
    is_floating: bool,
    is_suppressed: bool,
    is_selectable: bool,
    pane_x: usize,
    pane_y: usize,
    pane_rows: usize,
    pane_columns: usize,
    tab_position: usize,
}

async fn query_snapshot(
    session: &str,
    window_id: u64,
    client_id: u16,
) -> anyhow::Result<ZellijClientState> {
    let panes = Command::new("zellij")
        .args([
            "--session",
            session,
            "action",
            "list-panes",
            "--all",
            "--json",
        ])
        .output()
        .await
        .with_context(|| format!("could not list panes in Zellij session {session}"))?;
    anyhow::ensure!(
        panes.status.success(),
        "could not list panes in Zellij session {session}"
    );
    let clients = Command::new("zellij")
        .args(["--session", session, "action", "list-clients"])
        .output()
        .await
        .with_context(|| format!("could not list clients in Zellij session {session}"))?;
    anyhow::ensure!(
        clients.status.success(),
        "could not list clients in Zellij session {session}"
    );
    let focused_pane = focused_pane_from_clients(&clients.stdout)
        .context("Zellij does not have exactly one connected terminal client")?;
    let panes: Vec<ListedPane> = serde_json::from_slice(&panes.stdout)?;
    snapshot_from_panes(session, window_id, client_id, focused_pane, &panes)
        .context("Zellij has no focused terminal pane")
}

fn focused_pane_from_clients(output: &[u8]) -> Option<u32> {
    let output = std::str::from_utf8(output).ok()?;
    let mut panes = output.lines().skip(1).filter_map(|line| {
        line.split_whitespace()
            .nth(1)?
            .strip_prefix("terminal_")?
            .parse::<u32>()
            .ok()
    });
    let focused = panes.next()?;
    panes.next().is_none().then_some(focused)
}

fn snapshot_from_panes(
    session: &str,
    window_id: u64,
    client_id: u16,
    focused_pane: u32,
    panes: &[ListedPane],
) -> Option<ZellijClientState> {
    let focused = panes
        .iter()
        .find(|pane| !pane.is_plugin && pane.id == focused_pane)?;
    let rectangles: Vec<_> = panes
        .iter()
        .filter(|pane| {
            !pane.is_plugin
                && pane.is_selectable
                && !pane.is_suppressed
                && pane.tab_position == focused.tab_position
                && pane.is_floating == focused.is_floating
        })
        .map(|pane| {
            (
                u64::from(pane.id),
                Rect {
                    x: pane.pane_x as f64,
                    y: pane.pane_y as f64,
                    width: pane.pane_columns as f64,
                    height: pane.pane_rows as f64,
                },
            )
        })
        .collect();
    let pane_neighbors = directional_neighbors(rectangles)
        .into_iter()
        .map(|(id, neighbors)| {
            (
                (id as u32).to_string(),
                NeighborMap {
                    left: neighbors.left.map(|id| id as u32),
                    down: neighbors.down.map(|id| id as u32),
                    up: neighbors.up.map(|id| id as u32),
                    right: neighbors.right.map(|id| id as u32),
                },
            )
        })
        .collect();
    Some(ZellijClientState {
        client: ZellijClient {
            session: session.to_owned(),
            client_id,
        },
        niri_window_id: window_id,
        revision: 0,
        acknowledged_sequence: None,
        focused_pane,
        pane_neighbors,
    })
}

fn plugin_path() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRI_ZVIM_ZELLIJ_PLUGIN") {
        return path.into();
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    config.join("zellij/plugins/niri-zvim.wasm")
}

fn session_socket_exists(session: &str) -> bool {
    let mut components = Path::new(session).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return false;
    }
    let root = std::env::var_os("ZELLIJ_SOCKET_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR").map(|runtime| PathBuf::from(runtime).join("zellij"))
        })
        .unwrap_or_else(|| PathBuf::from("/tmp/zellij"));
    Path::new(&root)
        .join("contract_version_1")
        .join(session)
        .exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ghostty_titles_that_are_not_session_names() {
        for title in ["", ".", "..", "/home/dl", "project/src"] {
            assert!(!session_socket_exists(title), "accepted {title:?}");
        }
    }

    #[test]
    fn extracts_session_from_zellij_terminal_title() {
        assert_eq!(
            session_from_title("dev-session | nvim src/main.rs"),
            "dev-session"
        );
        assert_eq!(session_from_title("dev-session"), "dev-session");
    }

    #[test]
    fn builds_initial_snapshot_from_listed_panes() {
        let pane = |id, x| ListedPane {
            id,
            is_plugin: false,
            is_floating: false,
            is_suppressed: false,
            is_selectable: true,
            pane_x: x,
            pane_y: 0,
            pane_rows: 20,
            pane_columns: 40,
            tab_position: 0,
        };
        let state = snapshot_from_panes("dev", 42, 0, 1, &[pane(1, 0), pane(2, 40)]).unwrap();

        assert_eq!(state.focused_pane, 1);
        assert_eq!(state.pane_neighbors["1"].right, Some(2));
        assert_eq!(state.pane_neighbors["2"].left, Some(1));
    }

    #[test]
    fn extracts_the_only_connected_terminal_client_focus() {
        let one = b"CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND\n1 terminal_37 N/A\n";
        let two =
            b"CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND\n1 terminal_37 N/A\n2 terminal_9 N/A\n";

        assert_eq!(focused_pane_from_clients(one), Some(37));
        assert_eq!(focused_pane_from_clients(two), None);
    }

    #[test]
    fn metadata_fingerprint_ignores_cursor_but_tracks_topology() {
        let first = "id 1\npane_columns 40\ncursor_coordinates_in_pane 2 3\n";
        let moved_cursor = "id 1\npane_columns 40\ncursor_coordinates_in_pane 9 8\n";
        let resized = "id 1\npane_columns 80\ncursor_coordinates_in_pane 9 8\n";

        assert_eq!(
            metadata_topology_fingerprint(first),
            metadata_topology_fingerprint(moved_cursor)
        );
        assert_ne!(
            metadata_topology_fingerprint(first),
            metadata_topology_fingerprint(resized)
        );
    }
}
