use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::ErrorKind,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use niri_zvim_core::{
    ADAPTER_MAGIC, AdapterMessage, CONTROL_MAGIC, ControlResponse, DaemonMessage, DaemonStatus,
    Direction, NavigationAction, NavigationGraph, NiriStatus, NiriWindow, NvimStatus,
    PROTOCOL_VERSION, ProtocolMessage, STATUS_OPCODE, ZellijClient, ZellijStatus,
};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Semaphore, mpsc, oneshot},
    time::{MissedTickBehavior, interval, timeout},
};
use tracing::{debug, info, warn};

use crate::{
    config::{Config, config_path},
    niri::{NiriExecutor, start_event_thread},
    socket::socket_path,
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
const CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(250);
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
    AdapterDisconnected {
        sink: Sink,
    },
    ZellijBridgeStopped {
        session: String,
    },
    Status {
        response: oneshot::Sender<DaemonStatus>,
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
    active_mode: String,
    socket_path: String,
    started_at: Instant,
}

impl Daemon {
    fn new(niri: NiriExecutor, active_mode: String, socket_path: String) -> Self {
        Self {
            graph: NavigationGraph::default(),
            niri,
            nvim: BTreeMap::new(),
            zellij: BTreeMap::new(),
            sequence: 0,
            pending_niri: VecDeque::new(),
            pending_nvim: BTreeMap::new(),
            pending_zellij: BTreeMap::new(),
            active_mode,
            socket_path,
            started_at: Instant::now(),
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
            DaemonEvent::AdapterDisconnected { sink } => self.disconnect_adapter(&sink),
            DaemonEvent::ZellijBridgeStopped { session } => {
                self.zellij.retain(|client, _| client.session != session);
                self.pending_zellij
                    .retain(|client, _| client.session != session);
                self.graph.retain_zellij_clients(&session, &[]);
            }
            DaemonEvent::Status { response } => {
                let _ = response.send(self.status());
            }
        }
    }

    fn status(&self) -> DaemonStatus {
        let zellij = self
            .graph
            .zellij_states()
            .map(|state| ZellijStatus {
                session: state.client.session.clone(),
                client_id: state.client.client_id,
                niri_window_id: state.niri_window_id,
                focused_pane: state.focused_pane,
                pane_count: state.pane_neighbors.len(),
                connected: self
                    .zellij
                    .get(&state.client)
                    .is_some_and(|sink| !sink.is_closed()),
                pending_navigations: self
                    .pending_zellij
                    .get(&state.client)
                    .map_or(0, VecDeque::len),
            })
            .collect();
        let nvim = self
            .graph
            .nvim_instances()
            .map(|state| NvimStatus {
                id: state.id.clone(),
                parent: state.parent.clone(),
                terminal_focused: state.terminal_focused,
                focused_window: state.focused_window,
                window_count: state.window_neighbors.len(),
                connected: self
                    .nvim
                    .get(&state.id)
                    .is_some_and(|sink| !sink.is_closed()),
                pending_navigations: self.pending_nvim.get(&state.id).map_or(0, VecDeque::len),
            })
            .collect();
        DaemonStatus {
            version: env!("CARGO_PKG_VERSION").into(),
            protocol_version: PROTOCOL_VERSION,
            uptime_seconds: self.started_at.elapsed().as_secs(),
            active_mode: self.active_mode.clone(),
            socket_path: self.socket_path.clone(),
            navigation_sequence: self.sequence,
            niri: NiriStatus {
                window_count: self.graph.niri_window_count(),
                focused_window: self.graph.niri_focus(),
                pending_navigations: self.pending_niri.len(),
            },
            zellij,
            nvim,
        }
    }

    fn configure(&mut self, config: &Config) -> anyhow::Result<()> {
        let mode = config.active_mode()?;
        self.niri.configure(mode);
        self.active_mode = config.active_mode_name().to_owned();
        Ok(())
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
                    sink.try_send(DaemonMessage::ZellijNavigate {
                        client_id: client.client_id,
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
            AdapterMessage::ZellijClients {
                session,
                client_ids,
            } => {
                self.zellij.retain(|client, _| {
                    client.session != session || client_ids.contains(&client.client_id)
                });
                self.pending_zellij.retain(|client, _| {
                    client.session != session || client_ids.contains(&client.client_id)
                });
                self.graph.retain_zellij_clients(&session, &client_ids);
            }
            AdapterMessage::NvimClosed { id } => {
                self.nvim.remove(&id);
                self.pending_nvim.remove(&id);
                self.graph.remove_nvim(&id);
            }
        }
    }

    fn disconnect_adapter(&mut self, sink: &Sink) {
        for id in remove_sink_owners(&mut self.nvim, sink) {
            self.pending_nvim.remove(&id);
            self.graph.remove_nvim(&id);
        }
        for client in remove_sink_owners(&mut self.zellij, sink) {
            self.pending_zellij.remove(&client);
            self.graph.remove_zellij(&client);
        }
    }
}

fn remove_sink_owners<K>(owners: &mut BTreeMap<K, Sink>, sink: &Sink) -> Vec<K>
where
    K: Ord + Clone,
{
    let removed: Vec<_> = owners
        .iter()
        .filter(|(_key, owner)| owner.same_channel(sink))
        .map(|(key, _owner)| key.clone())
        .collect();
    for key in &removed {
        owners.remove(key);
    }
    removed
}

pub async fn run_daemon() -> anyhow::Result<()> {
    let mut config = Config::load_validated()?;
    let active_mode = config.active_mode_name().to_owned();
    let niri_mode = config.active_mode()?;
    let zellij_discovery = config.zellij_discovery()?.clone();
    info!(?niri_mode, "loaded Niri navigation mode");
    let path = socket_path()?;
    remove_stale_socket(&path)?;
    let listener =
        UnixListener::bind(&path).with_context(|| format!("could not bind {}", path.display()))?;
    secure_socket(&path)?;
    info!(path = %path.display(), "listening");

    let (events_tx, mut events_rx) = mpsc::channel(1024);
    let (config_updates, mut config_rx) = mpsc::channel(1);
    let niri_state = start_event_thread(events_tx.clone());
    tokio::spawn(accept_loop(listener, events_tx.clone()));
    tokio::spawn(watch_config(config_path(), config_updates));

    let mut daemon = Daemon::new(
        NiriExecutor::start(niri_mode, events_tx.clone(), niri_state),
        active_mode,
        path.display().to_string(),
    );
    let mut zellij = BridgeManager::new(zellij_discovery);
    let mut niri_windows = Vec::new();
    loop {
        tokio::select! {
            update = config_rx.recv() => {
                let Some(()) = update else {
                    break;
                };
                match Config::load_validated() {
                    Ok(updated) if updated != config => {
                        daemon.configure(&updated)?;
                        zellij.configure(updated.zellij_discovery()?.clone());
                        zellij.observe(&niri_windows, &events_tx);
                        info!(mode = updated.active_mode_name(), "reloaded configuration");
                        config = updated;
                    }
                    Ok(_) => {}
                    Err(error) => {
                        warn!(%error, "configuration reload rejected; keeping previous configuration");
                    }
                }
            }
            event = events_rx.recv() => {
                let Some(event) = event else {
                    break;
                };
                if let DaemonEvent::NiriSnapshot { windows, .. } = &event {
                    niri_windows.clone_from(windows);
                    zellij.observe(windows, &events_tx);
                }
                if let DaemonEvent::ZellijBridgeStopped { session } = &event
                    && !zellij.bridge_stopped(session)
                {
                    continue;
                }
                let bridge_stopped = matches!(&event, DaemonEvent::ZellijBridgeStopped { .. });
                daemon.handle(event);
                if bridge_stopped {
                    zellij.observe(&niri_windows, &events_tx);
                }
            }
        }
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
enum ConfigFingerprint {
    Missing,
    Contents(Vec<u8>),
    Unreadable(ErrorKind, Option<i32>),
}

fn config_fingerprint(path: &Path) -> ConfigFingerprint {
    match fs::read(path) {
        Ok(contents) => ConfigFingerprint::Contents(contents),
        Err(error) if error.kind() == ErrorKind::NotFound => ConfigFingerprint::Missing,
        Err(error) => ConfigFingerprint::Unreadable(error.kind(), error.raw_os_error()),
    }
}

async fn watch_config(path: PathBuf, updates: mpsc::Sender<()>) {
    let mut previous = None;
    let mut poll = interval(CONFIG_POLL_INTERVAL);
    poll.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        poll.tick().await;
        let current = config_fingerprint(&path);
        if previous.as_ref() == Some(&current) {
            continue;
        }
        previous = Some(current);
        if updates.send(()).await.is_err() {
            break;
        }
    }
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
    let version = timeout(HANDSHAKE_TIMEOUT, stream.read_u8())
        .await
        .context("connection protocol version timed out")??;
    anyhow::ensure!(
        version == PROTOCOL_VERSION,
        "protocol version {version} is not supported; expected {PROTOCOL_VERSION}"
    );
    if first == CONTROL_MAGIC {
        let opcode = timeout(HANDSHAKE_TIMEOUT, stream.read_u8())
            .await
            .context("control request timed out")??;
        if opcode == STATUS_OPCODE {
            let (response, status) = oneshot::channel();
            events.send(DaemonEvent::Status { response }).await?;
            let status = timeout(HANDSHAKE_TIMEOUT, status)
                .await
                .context("daemon status response timed out")??;
            let mut encoded =
                serde_json::to_vec(&ProtocolMessage::new(ControlResponse::Status { status }))?;
            encoded.push(b'\n');
            stream.write_all(&encoded).await?;
            return Ok(());
        }
        let direction = Direction::from_control_opcode(opcode)
            .with_context(|| format!("invalid control opcode {opcode}"))?;
        events.send(DaemonEvent::Navigate(direction)).await?;
        return Ok(());
    }
    anyhow::ensure!(first == ADAPTER_MAGIC, "invalid protocol byte {first}");

    let (reader, mut writer) = stream.into_split();
    let (sink, mut outgoing) = mpsc::channel(ADAPTER_OUTGOING_CAPACITY);
    let writer_task = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            let Ok(mut encoded) = serde_json::to_vec(&ProtocolMessage::new(message)) else {
                continue;
            };
            encoded.push(b'\n');
            if writer.write_all(&encoded).await.is_err() {
                break;
            }
        }
    });

    let read_result =
        read_adapter_messages(BufReader::new(reader), events.clone(), sink.clone()).await;
    writer_task.abort();
    let _ = writer_task.await;
    let _ = events.send(DaemonEvent::AdapterDisconnected { sink }).await;
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
        let message =
            serde_json::from_slice::<ProtocolMessage<AdapterMessage>>(&frame)?.into_current()?;
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
    use std::{
        io::Write,
        net::Shutdown,
        os::unix::{
            fs::PermissionsExt,
            net::{UnixListener as StdUnixListener, UnixStream as StdUnixStream},
        },
    };

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
    async fn versioned_control_frame_routes_navigation() {
        let (mut client, server) = StdUnixStream::pair().unwrap();
        server.set_nonblocking(true).unwrap();
        let server = UnixStream::from_std(server).unwrap();
        let (events, mut received) = mpsc::channel(1);

        client.write_all(&Direction::Down.control_frame()).unwrap();
        handle_connection(server, events).await.unwrap();

        assert!(matches!(
            received.recv().await,
            Some(DaemonEvent::Navigate(Direction::Down))
        ));
    }

    #[tokio::test]
    async fn status_control_frame_returns_versioned_state() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let (events, mut received) = mpsc::channel(1);
        let connection = tokio::spawn(handle_connection(server, events));
        let expected = DaemonStatus {
            version: "0.2.0".into(),
            protocol_version: PROTOCOL_VERSION,
            uptime_seconds: 12,
            active_mode: "desktop".into(),
            socket_path: "/run/user/1000/niri-zvim.sock".into(),
            navigation_sequence: 9,
            niri: NiriStatus {
                window_count: 4,
                focused_window: Some(42),
                pending_navigations: 0,
            },
            zellij: Vec::new(),
            nvim: Vec::new(),
        };

        client
            .write_all(&niri_zvim_core::status_frame())
            .await
            .unwrap();
        let response = match received.recv().await {
            Some(DaemonEvent::Status { response }) => response,
            _ => panic!("status event was not received"),
        };
        response.send(expected.clone()).unwrap();

        let mut encoded = Vec::new();
        client.read_to_end(&mut encoded).await.unwrap();
        let response = serde_json::from_slice::<ProtocolMessage<ControlResponse>>(&encoded)
            .unwrap()
            .into_current()
            .unwrap();
        assert_eq!(response, ControlResponse::Status { status: expected });
        connection.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn legacy_and_mismatched_connections_are_rejected() {
        for frame in [
            vec![Direction::Left.control_opcode()],
            vec![CONTROL_MAGIC, PROTOCOL_VERSION.wrapping_add(1), 1],
            vec![ADAPTER_MAGIC, PROTOCOL_VERSION.wrapping_add(1)],
        ] {
            let (mut client, server) = StdUnixStream::pair().unwrap();
            server.set_nonblocking(true).unwrap();
            let server = UnixStream::from_std(server).unwrap();
            let (events, mut received) = mpsc::channel(1);
            client.write_all(&frame).unwrap();
            client.shutdown(Shutdown::Write).unwrap();

            assert!(handle_connection(server, events).await.is_err());
            assert!(received.try_recv().is_err());
        }
    }

    #[tokio::test]
    async fn daemon_wraps_outgoing_adapter_messages() {
        let (mut client, server) = UnixStream::pair().unwrap();
        let (events, mut received) = mpsc::channel(1);
        let connection = tokio::spawn(handle_connection(server, events));
        let mut frame = serde_json::to_vec(&ProtocolMessage::new(AdapterMessage::NvimClosed {
            id: "test".into(),
        }))
        .unwrap();
        frame.push(b'\n');

        client
            .write_all(&niri_zvim_core::adapter_prelude())
            .await
            .unwrap();
        client.write_all(&frame).await.unwrap();
        let sink = match received.recv().await {
            Some(DaemonEvent::Adapter { sink, .. }) => sink,
            _ => panic!("adapter event was not received"),
        };
        sink.send(DaemonMessage::Navigate {
            sequence: 9,
            direction: Direction::Right,
        })
        .await
        .unwrap();

        let mut reader = BufReader::new(client);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        let message = serde_json::from_str::<ProtocolMessage<DaemonMessage>>(&line)
            .unwrap()
            .into_current()
            .unwrap();
        assert_eq!(
            message,
            DaemonMessage::Navigate {
                sequence: 9,
                direction: Direction::Right,
            }
        );

        drop(reader);
        connection.await.unwrap().unwrap();
        assert!(matches!(
            received.recv().await,
            Some(DaemonEvent::AdapterDisconnected { .. })
        ));
    }

    #[test]
    fn disconnected_adapter_cleanup_preserves_reconnected_owners() {
        let (disconnected, _disconnected_messages) = mpsc::channel(1);
        let (connected, _connected_messages) = mpsc::channel(1);
        let mut owners =
            BTreeMap::from([("old", disconnected.clone()), ("new", connected.clone())]);

        assert_eq!(remove_sink_owners(&mut owners, &disconnected), vec!["old"]);
        assert_eq!(owners.keys().copied().collect::<Vec<_>>(), vec!["new"]);
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

    #[tokio::test]
    async fn config_watcher_reports_file_content_changes() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("config.json");
        let (updates, mut received) = mpsc::channel(1);
        let watcher = tokio::spawn(watch_config(path.clone(), updates));

        timeout(Duration::from_secs(1), received.recv())
            .await
            .unwrap()
            .unwrap();
        fs::write(&path, b"first").unwrap();
        timeout(Duration::from_secs(1), received.recv())
            .await
            .unwrap()
            .unwrap();
        fs::write(&path, b"second").unwrap();
        timeout(Duration::from_secs(1), received.recv())
            .await
            .unwrap()
            .unwrap();

        watcher.abort();
    }
}
