use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::ErrorKind,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use niri_zvim_core::{
    AdapterMessage, DaemonMessage, Direction, NavigationAction, NavigationGraph, NiriWindow,
    ZellijClient,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Semaphore, mpsc},
    time::timeout,
};
use tracing::{debug, info, warn};

use crate::{
    config::Config,
    niri::{NiriExecutor, start_event_thread},
    socket::{adapter_magic, socket_path},
    zellij::BridgeManager,
};

use self::reconcile::{
    PendingNavigation, PendingNiriNavigation, acknowledge_niri_observed, acknowledge_niri_pending,
    acknowledge_observed, acknowledge_pending,
};

mod reconcile;

const MAX_CONNECTIONS: usize = 128;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_ADAPTER_FRAME_BYTES: usize = 1024 * 1024;
pub(crate) const ADAPTER_OUTGOING_CAPACITY: usize = 64;

type Sink = mpsc::Sender<DaemonMessage>;

pub(crate) enum DaemonEvent {
    Navigate(Direction),
    NiriSnapshot {
        windows: Vec<NiriWindow>,
        focused: Option<u64>,
        acknowledged_sequence: Option<u64>,
    },
    Adapter {
        message: AdapterMessage,
        sink: Sink,
    },
    ZellijBridgeStopped {
        session: String,
    },
}

struct Daemon {
    graph: NavigationGraph,
    niri: NiriExecutor,
    nvim: BTreeMap<String, Sink>,
    zellij: BTreeMap<ZellijClient, Sink>,
    sequence: u64,
    pending_niri: VecDeque<PendingNiriNavigation>,
    pending_nvim: BTreeMap<String, VecDeque<PendingNavigation>>,
    pending_zellij: BTreeMap<ZellijClient, VecDeque<PendingNavigation>>,
}

impl Daemon {
    fn new(niri: NiriExecutor) -> Self {
        Self {
            graph: NavigationGraph::default(),
            niri,
            nvim: BTreeMap::new(),
            zellij: BTreeMap::new(),
            sequence: 0,
            pending_niri: VecDeque::new(),
            pending_nvim: BTreeMap::new(),
            pending_zellij: BTreeMap::new(),
        }
    }

    fn handle(&mut self, event: DaemonEvent) {
        match event {
            DaemonEvent::Navigate(direction) => self.navigate(direction),
            DaemonEvent::NiriSnapshot {
                windows,
                focused,
                acknowledged_sequence,
            } => {
                if let Some(sequence) = acknowledged_sequence {
                    acknowledge_niri_pending(&mut self.pending_niri, sequence);
                } else {
                    acknowledge_niri_observed(&mut self.pending_niri, focused);
                }
                self.graph.replace_niri_windows(windows, focused);
                for navigation in &self.pending_niri {
                    self.graph.predict_niri_focus(navigation.direction);
                }
            }
            DaemonEvent::Adapter { message, sink } => self.update_adapter(message, sink),
            DaemonEvent::ZellijBridgeStopped { session } => {
                self.pending_zellij
                    .retain(|client, _| client.session != session);
            }
        }
    }

    fn navigate(&mut self, direction: Direction) {
        self.sequence = self.sequence.wrapping_add(1);
        let sequence = self.sequence;
        let Ok(action) = self.graph.route_optimistically(direction) else {
            debug!(
                ?direction,
                pending = self.pending_niri.len(),
                "Niri focus unknown; routing directly to Niri"
            );
            self.niri.navigate(sequence, direction);
            return;
        };
        debug!(?direction, ?action, "routed navigation");

        let sent = match action {
            NavigationAction::Niri { direction } => {
                self.pending_niri.push_back(PendingNiriNavigation {
                    sequence,
                    direction,
                    expected: self.graph.niri_focus(),
                });
                self.niri.navigate(sequence, direction);
                true
            }
            NavigationAction::Nvim { id, direction } => {
                let sent = self.nvim.get(&id).is_some_and(|sink| {
                    sink.try_send(DaemonMessage::Navigate {
                        sequence,
                        direction,
                    })
                    .is_ok()
                });
                if sent {
                    let expected = self
                        .graph
                        .nvim_focus(&id)
                        .expect("routed Neovim instance remains in the graph");
                    self.pending_nvim
                        .entry(id)
                        .or_default()
                        .push_back(PendingNavigation {
                            sequence,
                            direction,
                            expected,
                        });
                }
                sent
            }
            NavigationAction::Zellij { client, direction } => {
                let sent = self.zellij.get(&client).is_some_and(|sink| {
                    sink.try_send(DaemonMessage::Navigate {
                        sequence,
                        direction,
                    })
                    .is_ok()
                });
                if sent {
                    let expected = u64::from(
                        self.graph
                            .zellij_focus(&client)
                            .expect("routed Zellij client remains in the graph"),
                    );
                    self.pending_zellij
                        .entry(client)
                        .or_default()
                        .push_back(PendingNavigation {
                            sequence,
                            direction,
                            expected,
                        });
                }
                sent
            }
        };

        if !sent {
            warn!(
                ?direction,
                "nested executor unavailable; falling back to niri"
            );
            self.graph.predict_niri_focus(direction);
            self.pending_niri.push_back(PendingNiriNavigation {
                sequence,
                direction,
                expected: self.graph.niri_focus(),
            });
            self.niri.navigate(sequence, direction);
        }
    }

    fn update_adapter(&mut self, message: AdapterMessage, sink: Sink) {
        match message {
            AdapterMessage::NvimSnapshot { state } => {
                let id = state.id.clone();
                self.nvim.insert(id.clone(), sink);
                let sequence_acknowledged = state.acknowledged_sequence.is_some_and(|sequence| {
                    acknowledge_pending(self.pending_nvim.get_mut(&id), sequence)
                });
                let observed_acknowledged =
                    acknowledge_observed(self.pending_nvim.get_mut(&id), state.focused_window);
                let acknowledged = sequence_acknowledged || observed_acknowledged;
                let has_pending = self
                    .pending_nvim
                    .get(&id)
                    .is_some_and(|pending| !pending.is_empty());
                if acknowledged || has_pending {
                    self.graph.acknowledge_nvim(state);
                } else {
                    self.graph.update_nvim(state);
                }
                if let Some(pending) = self.pending_nvim.get(&id) {
                    for navigation in pending {
                        self.graph.predict_nvim_focus(&id, navigation.direction);
                    }
                }
            }
            AdapterMessage::ZellijSnapshot { state } => {
                debug!(
                    session = %state.client.session,
                    client_id = state.client.client_id,
                    window_id = state.niri_window_id,
                    revision = state.revision,
                    focused_pane = state.focused_pane,
                    acknowledged_sequence = ?state.acknowledged_sequence,
                    "received Zellij snapshot"
                );
                let client = state.client.clone();
                self.zellij.insert(client.clone(), sink);
                let sequence_acknowledged = state.acknowledged_sequence.is_some_and(|sequence| {
                    acknowledge_pending(self.pending_zellij.get_mut(&client), sequence)
                });
                let observed_acknowledged = acknowledge_observed(
                    self.pending_zellij.get_mut(&client),
                    u64::from(state.focused_pane),
                );
                let acknowledged = sequence_acknowledged || observed_acknowledged;
                let has_pending = self
                    .pending_zellij
                    .get(&client)
                    .is_some_and(|pending| !pending.is_empty());
                if acknowledged || has_pending {
                    self.graph.acknowledge_zellij(state);
                } else {
                    self.graph.update_zellij(state);
                }
                if let Some(pending) = self.pending_zellij.get(&client) {
                    for navigation in pending {
                        self.graph
                            .predict_zellij_focus(&client, navigation.direction);
                    }
                }
            }
            AdapterMessage::NvimClosed { id } => {
                self.nvim.remove(&id);
                self.pending_nvim.remove(&id);
                self.graph.remove_nvim(&id);
            }
        }
    }
}

pub async fn run_daemon() -> anyhow::Result<()> {
    let niri_mode = Config::load()?.active_mode()?;
    info!(?niri_mode, "loaded Niri navigation mode");
    let path = socket_path()?;
    remove_stale_socket(&path)?;
    let listener =
        UnixListener::bind(&path).with_context(|| format!("could not bind {}", path.display()))?;
    secure_socket(&path)?;
    info!(path = %path.display(), "listening");

    let (events_tx, mut events_rx) = mpsc::channel(1024);
    let niri_state = start_event_thread(events_tx.clone());
    tokio::spawn(accept_loop(listener, events_tx.clone()));

    let mut daemon = Daemon::new(NiriExecutor::start(
        niri_mode,
        events_tx.clone(),
        niri_state,
    ));
    let mut zellij = BridgeManager::default();
    while let Some(event) = events_rx.recv().await {
        if let DaemonEvent::NiriSnapshot { windows, .. } = &event {
            zellij.observe(windows, &events_tx);
        }
        if let DaemonEvent::ZellijBridgeStopped { session } = &event {
            zellij.bridge_stopped(session);
        }
        daemon.handle(event);
    }
    Ok(())
}

fn remove_stale_socket(path: &Path) -> anyhow::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("could not remove {}", path.display())),
    }
}

fn secure_socket(path: &Path) -> anyhow::Result<()> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .with_context(|| format!("could not secure {}", path.display()))
}

async fn accept_loop(listener: UnixListener, events: mpsc::Sender<DaemonEvent>) {
    let connections = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let Ok(permit) = connections.clone().try_acquire_owned() else {
                    warn!(limit = MAX_CONNECTIONS, "connection limit reached");
                    continue;
                };
                let events = events.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    if let Err(error) = handle_connection(stream, events).await {
                        debug!(%error, "client disconnected");
                    }
                });
            }
            Err(error) => warn!(%error, "socket accept failed"),
        }
    }
}

async fn handle_connection(
    mut stream: UnixStream,
    events: mpsc::Sender<DaemonEvent>,
) -> anyhow::Result<()> {
    let first = timeout(HANDSHAKE_TIMEOUT, stream.read_u8())
        .await
        .context("connection handshake timed out")??;
    if let Some(direction) = Direction::from_wire_byte(first) {
        events.send(DaemonEvent::Navigate(direction)).await?;
        return Ok(());
    }
    anyhow::ensure!(first == adapter_magic(), "invalid protocol byte {first}");

    let (reader, mut writer) = stream.into_split();
    let (sink, mut outgoing) = mpsc::channel(ADAPTER_OUTGOING_CAPACITY);
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            let Ok(mut encoded) = serde_json::to_vec(&message) else {
                continue;
            };
            encoded.push(b'\n');
            if writer.write_all(&encoded).await.is_err() {
                break;
            }
        }
    });

    let read_result = read_adapter_messages(BufReader::new(reader), events, sink).await;
    writer_task.abort();
    let _ = writer_task.await;
    read_result
}

async fn read_adapter_messages<R>(
    mut reader: R,
    events: mpsc::Sender<DaemonEvent>,
    sink: Sink,
) -> anyhow::Result<()>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    while read_bounded_line(&mut reader, &mut frame).await? {
        let message = serde_json::from_slice(&frame)?;
        events
            .send(DaemonEvent::Adapter {
                message,
                sink: sink.clone(),
            })
            .await?;
    }
    Ok(())
}

async fn read_bounded_line<R>(reader: &mut R, frame: &mut Vec<u8>) -> anyhow::Result<bool>
where
    R: AsyncBufRead + Unpin,
{
    frame.clear();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(!frame.is_empty());
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let data_len = newline.unwrap_or(available.len());
        anyhow::ensure!(
            frame.len() + data_len <= MAX_ADAPTER_FRAME_BYTES,
            "adapter frame exceeds {MAX_ADAPTER_FRAME_BYTES} bytes"
        );
        frame.extend_from_slice(&available[..data_len]);
        reader.consume(newline.map_or(data_len, |position| position + 1));
        if newline.is_some() {
            return Ok(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{os::unix::fs::PermissionsExt, os::unix::net::UnixListener as StdUnixListener};

    use tokio::io::BufReader;

    use super::*;

    #[test]
    fn daemon_socket_is_user_only() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("daemon.sock");
        let _listener = StdUnixListener::bind(&path).unwrap();

        secure_socket(&path).unwrap();

        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[tokio::test]
    async fn adapter_lines_are_size_bounded() {
        let input = vec![b'x'; MAX_ADAPTER_FRAME_BYTES + 1];
        let mut reader = BufReader::new(input.as_slice());
        let mut frame = Vec::new();

        let error = read_bounded_line(&mut reader, &mut frame)
            .await
            .unwrap_err();

        assert!(error.to_string().contains("adapter frame exceeds"));
    }

    #[tokio::test]
    async fn adapter_lines_accept_a_final_frame_without_a_newline() {
        let mut reader = BufReader::new(&b"{}"[..]);
        let mut frame = Vec::new();

        assert!(read_bounded_line(&mut reader, &mut frame).await.unwrap());
        assert_eq!(frame, b"{}");
        assert!(!read_bounded_line(&mut reader, &mut frame).await.unwrap());
    }
}
