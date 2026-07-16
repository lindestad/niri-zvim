use std::{
    collections::{BTreeMap, VecDeque},
    fs,
    io::ErrorKind,
    path::Path,
};

use anyhow::Context;
use niri_zvim_core::{
    AdapterMessage, DaemonMessage, Direction, NavigationAction, NavigationGraph, NiriWindow,
    ZellijClient,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::mpsc,
};
use tracing::{debug, info, warn};

use crate::{
    config::Config,
    niri::{NiriExecutor, start_event_thread},
    socket::{adapter_magic, socket_path},
    zellij::BridgeManager,
};

type Sink = mpsc::UnboundedSender<DaemonMessage>;

#[derive(Clone, Copy)]
struct PendingNavigation {
    sequence: u64,
    direction: Direction,
    expected: u64,
}

pub(crate) enum DaemonEvent {
    Navigate(Direction),
    NiriSnapshot {
        windows: Vec<NiriWindow>,
        focused: Option<u64>,
        acknowledges_focus: bool,
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
    pending_niri: VecDeque<Direction>,
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
                acknowledges_focus,
            } => {
                self.graph.replace_niri_windows(windows, focused);
                if acknowledges_focus {
                    self.pending_niri.pop_front();
                }
                for direction in self.pending_niri.iter().copied() {
                    self.graph.predict_niri_focus(direction);
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
            self.niri.navigate(direction);
            return;
        };
        debug!(?direction, ?action, "routed navigation");

        let sent = match action {
            NavigationAction::Niri { direction } => {
                self.pending_niri.push_back(direction);
                self.niri.navigate(direction);
                true
            }
            NavigationAction::Nvim { id, direction } => {
                let sent = self.nvim.get(&id).is_some_and(|sink| {
                    sink.send(DaemonMessage::Navigate {
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
                    sink.send(DaemonMessage::Navigate {
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
            self.pending_niri.push_back(direction);
            self.graph.predict_niri_focus(direction);
            self.niri.navigate(direction);
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

fn acknowledge_pending(
    pending: Option<&mut VecDeque<PendingNavigation>>,
    acknowledged_sequence: u64,
) -> bool {
    let Some(pending) = pending else {
        return false;
    };
    let before = pending.len();
    while pending
        .front()
        .is_some_and(|navigation| navigation.sequence <= acknowledged_sequence)
    {
        pending.pop_front();
    }
    before != pending.len()
}

fn acknowledge_observed(pending: Option<&mut VecDeque<PendingNavigation>>, observed: u64) -> bool {
    let Some(pending) = pending else {
        return false;
    };
    let Some(position) = pending
        .iter()
        .rposition(|navigation| navigation.expected == observed)
    else {
        return false;
    };
    pending.drain(..=position);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_acknowledgement_clears_every_covered_prediction() {
        let mut pending = VecDeque::from([
            PendingNavigation {
                sequence: 4,
                direction: Direction::Right,
                expected: 11,
            },
            PendingNavigation {
                sequence: 5,
                direction: Direction::Right,
                expected: 12,
            },
            PendingNavigation {
                sequence: 7,
                direction: Direction::Left,
                expected: 11,
            },
        ]);

        assert!(acknowledge_pending(Some(&mut pending), 5));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().sequence, 7);
        assert!(!acknowledge_pending(Some(&mut pending), 5));
    }

    #[test]
    fn observed_target_acknowledges_the_matching_prediction_prefix() {
        let mut pending = VecDeque::from([
            PendingNavigation {
                sequence: 4,
                direction: Direction::Right,
                expected: 11,
            },
            PendingNavigation {
                sequence: 5,
                direction: Direction::Right,
                expected: 12,
            },
            PendingNavigation {
                sequence: 6,
                direction: Direction::Right,
                expected: 13,
            },
        ]);

        assert!(acknowledge_observed(Some(&mut pending), 12));
        assert_eq!(pending.len(), 1);
        assert_eq!(pending.front().unwrap().expected, 13);
        assert!(!acknowledge_observed(Some(&mut pending), 12));
    }
}

pub async fn run_daemon() -> anyhow::Result<()> {
    let niri_mode = Config::load()?.active_mode()?;
    info!(?niri_mode, "loaded Niri navigation mode");
    let path = socket_path();
    remove_stale_socket(&path)?;
    let listener =
        UnixListener::bind(&path).with_context(|| format!("could not bind {}", path.display()))?;
    info!(path = %path.display(), "listening");

    let (events_tx, mut events_rx) = mpsc::channel(1024);
    start_event_thread(events_tx.clone());
    tokio::spawn(accept_loop(listener, events_tx.clone()));

    let mut daemon = Daemon::new(NiriExecutor::start(niri_mode));
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

async fn accept_loop(listener: UnixListener, events: mpsc::Sender<DaemonEvent>) {
    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let events = events.clone();
                tokio::spawn(async move {
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
    let first = stream.read_u8().await?;
    if let Some(direction) = Direction::from_wire_byte(first) {
        events.send(DaemonEvent::Navigate(direction)).await?;
        return Ok(());
    }
    anyhow::ensure!(first == adapter_magic(), "invalid protocol byte {first}");

    let (reader, mut writer) = stream.into_split();
    let (sink, mut outgoing) = mpsc::unbounded_channel();
    tokio::spawn(async move {
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

    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let message = serde_json::from_str(&line)?;
        events
            .send(DaemonEvent::Adapter {
                message,
                sink: sink.clone(),
            })
            .await?;
    }
    Ok(())
}
