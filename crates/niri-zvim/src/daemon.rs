use std::{collections::BTreeMap, fs, io::ErrorKind, path::Path};

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
    niri::{NiriExecutor, start_event_thread},
    socket::{adapter_magic, socket_path},
};

type Sink = mpsc::UnboundedSender<DaemonMessage>;

pub(crate) enum DaemonEvent {
    Navigate(Direction),
    NiriSnapshot {
        windows: Vec<NiriWindow>,
        focused: Option<u64>,
    },
    Adapter {
        message: AdapterMessage,
        sink: Sink,
    },
}

struct Daemon {
    graph: NavigationGraph,
    niri: NiriExecutor,
    nvim: BTreeMap<String, Sink>,
    zellij: BTreeMap<ZellijClient, Sink>,
    sequence: u64,
}

impl Daemon {
    fn new() -> Self {
        Self {
            graph: NavigationGraph::default(),
            niri: NiriExecutor::start(),
            nvim: BTreeMap::new(),
            zellij: BTreeMap::new(),
            sequence: 0,
        }
    }

    fn handle(&mut self, event: DaemonEvent) {
        match event {
            DaemonEvent::Navigate(direction) => self.navigate(direction),
            DaemonEvent::NiriSnapshot { windows, focused } => {
                self.graph.replace_niri_windows(windows, focused);
            }
            DaemonEvent::Adapter { message, sink } => self.update_adapter(message, sink),
        }
    }

    fn navigate(&mut self, direction: Direction) {
        self.sequence = self.sequence.wrapping_add(1);
        let sequence = self.sequence;
        let Ok(action) = self.graph.route_optimistically(direction) else {
            self.niri.navigate(direction);
            return;
        };

        let sent = match action {
            NavigationAction::Niri { direction } => {
                self.niri.navigate(direction);
                true
            }
            NavigationAction::Nvim { id, direction } => self.nvim.get(&id).is_some_and(|sink| {
                sink.send(DaemonMessage::Navigate {
                    sequence,
                    direction,
                })
                .is_ok()
            }),
            NavigationAction::Zellij { client, direction } => {
                self.zellij.get(&client).is_some_and(|sink| {
                    sink.send(DaemonMessage::Navigate {
                        sequence,
                        direction,
                    })
                    .is_ok()
                })
            }
        };

        if !sent {
            warn!(
                ?direction,
                "nested executor unavailable; falling back to niri"
            );
            self.niri.navigate(direction);
        }
    }

    fn update_adapter(&mut self, message: AdapterMessage, sink: Sink) {
        match message {
            AdapterMessage::NvimSnapshot { state } => {
                self.nvim.insert(state.id.clone(), sink);
                self.graph.update_nvim(state);
            }
            AdapterMessage::ZellijSnapshot { state } => {
                self.zellij.insert(state.client.clone(), sink);
                self.graph.update_zellij(state);
            }
            AdapterMessage::NvimClosed { id } => {
                self.nvim.remove(&id);
                self.graph.remove_nvim(&id);
            }
        }
    }
}

pub async fn run_daemon() -> anyhow::Result<()> {
    let path = socket_path();
    remove_stale_socket(&path)?;
    let listener =
        UnixListener::bind(&path).with_context(|| format!("could not bind {}", path.display()))?;
    info!(path = %path.display(), "listening");

    let (events_tx, mut events_rx) = mpsc::channel(1024);
    start_event_thread(events_tx.clone());
    tokio::spawn(accept_loop(listener, events_tx));

    let mut daemon = Daemon::new();
    while let Some(event) = events_rx.recv().await {
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
