use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Context;
use niri_zvim_core::{AdapterMessage, DaemonMessage, ProtocolMessage};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{mpsc, watch},
    time::{Duration, Instant, sleep, sleep_until},
};

const FALLBACK_REFRESH_DELAY: Duration = Duration::from_millis(50);

#[derive(Clone, Copy)]
pub(super) enum RefreshEvent {
    Navigate(u64),
    Acknowledge(u64),
    MetadataChanged,
}

#[derive(Default)]
struct RefreshState {
    pending_sequence: Option<u64>,
    metadata_dirty: bool,
}

impl RefreshState {
    fn update(&mut self, event: RefreshEvent) {
        match event {
            RefreshEvent::Navigate(sequence) => {
                self.pending_sequence = Some(
                    self.pending_sequence
                        .map_or(sequence, |pending| pending.max(sequence)),
                );
            }
            RefreshEvent::Acknowledge(sequence)
                if self
                    .pending_sequence
                    .is_some_and(|pending| sequence >= pending) =>
            {
                self.pending_sequence = None;
            }
            RefreshEvent::Acknowledge(_) => {}
            RefreshEvent::MetadataChanged => self.metadata_dirty = true,
        }
    }

    fn needs_refresh(&self) -> bool {
        self.pending_sequence.is_some() || self.metadata_dirty
    }
}
use tracing::{debug, info};

use crate::daemon::{ADAPTER_OUTGOING_CAPACITY, DaemonEvent};

use super::{
    metadata::{plugin_path, watch_session_metadata},
    snapshot::query_snapshots,
};

pub(super) async fn run_bridge(
    session: &str,
    mut windows: watch::Receiver<Vec<u64>>,
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
    let (sink, mut actions) = mpsc::channel(ADAPTER_OUTGOING_CAPACITY);
    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel();
    let revisions = Arc::new(AtomicU64::new(0));

    let initial_windows = windows.borrow().clone();
    for mut state in query_snapshots(session, &initial_windows)
        .await
        .unwrap_or_default()
    {
        state.revision = revisions.fetch_add(1, Ordering::Relaxed) + 1;
        events
            .send(DaemonEvent::Adapter {
                message: AdapterMessage::ZellijSnapshot { state },
                sink: sink.clone(),
            })
            .await?;
    }

    let metadata_watcher = tokio::spawn(watch_session_metadata(
        session.to_owned(),
        refresh_tx.clone(),
    ));
    let fallback_refresher = tokio::spawn(fallback_refresh_loop(
        session.to_owned(),
        windows.clone(),
        events.clone(),
        sink.clone(),
        revisions.clone(),
        refresh_rx,
    ));

    let bind = serde_json::to_vec(&ProtocolMessage::new(DaemonMessage::ZellijSync {
        session: session.to_owned(),
        window_ids: windows.borrow().clone(),
    }))?;
    stdin.write_all(&bind).await?;
    stdin.write_u8(b'\n').await?;
    stdin.flush().await?;
    info!(%session, windows = windows.borrow().len(), "connected Zellij bridge");

    let action_refresh = refresh_tx.clone();
    let writer_session = session.to_owned();
    tokio::spawn(async move {
        loop {
            let message = tokio::select! {
                message = actions.recv() => {
                    let Some(message) = message else {
                        break;
                    };
                    message
                }
                changed = windows.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    DaemonMessage::ZellijSync {
                        session: writer_session.clone(),
                        window_ids: windows.borrow().clone(),
                    }
                }
            };
            let sequence = match &message {
                DaemonMessage::ZellijNavigate { sequence, .. } => Some(*sequence),
                DaemonMessage::Navigate { .. }
                | DaemonMessage::ZellijSync { .. }
                | DaemonMessage::ZellijSyncClient { .. } => None,
            };
            let Ok(mut encoded) = serde_json::to_vec(&ProtocolMessage::new(message)) else {
                continue;
            };
            encoded.push(b'\n');
            if stdin.write_all(&encoded).await.is_err() || stdin.flush().await.is_err() {
                break;
            }
            if let Some(sequence) = sequence {
                let _ = action_refresh.send(RefreshEvent::Navigate(sequence));
            }
        }
    });

    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        match serde_json::from_str::<ProtocolMessage<AdapterMessage>>(&line).and_then(|message| {
            message
                .into_current()
                .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))
        }) {
            Ok(AdapterMessage::ZellijSnapshot { mut state }) => {
                if let Some(sequence) = state.acknowledged_sequence {
                    let _ = refresh_tx.send(RefreshEvent::Acknowledge(sequence));
                }
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
    fallback_refresher.abort();
    anyhow::bail!("Zellij pipe exited with {status}")
}

async fn fallback_refresh_loop(
    session: String,
    windows: watch::Receiver<Vec<u64>>,
    events: mpsc::Sender<DaemonEvent>,
    sink: mpsc::Sender<DaemonMessage>,
    revisions: Arc<AtomicU64>,
    mut refreshes: mpsc::UnboundedReceiver<RefreshEvent>,
) {
    let mut state = RefreshState::default();
    while let Some(event) = refreshes.recv().await {
        state.update(event);
        if !state.needs_refresh() {
            continue;
        }

        let timer = sleep_until(Instant::now() + FALLBACK_REFRESH_DELAY);
        tokio::pin!(timer);
        loop {
            tokio::select! {
                event = refreshes.recv() => {
                    let Some(event) = event else {
                        return;
                    };
                    let reset = matches!(event, RefreshEvent::Navigate(_));
                    state.update(event);
                    if !state.needs_refresh() {
                        break;
                    }
                    if reset {
                        timer.as_mut().reset(Instant::now() + FALLBACK_REFRESH_DELAY);
                    }
                }
                () = &mut timer => {
                    state.pending_sequence = None;
                    state.metadata_dirty = false;
                    let window_ids = windows.borrow().clone();
                    let Ok(snapshots) = query_snapshots(&session, &window_ids).await else {
                        break;
                    };
                    for mut snapshot in snapshots {
                        snapshot.revision = revisions.fetch_add(1, Ordering::Relaxed) + 1;
                        if events.send(DaemonEvent::Adapter {
                            message: AdapterMessage::ZellijSnapshot { state: snapshot },
                            sink: sink.clone(),
                        }).await.is_err() {
                            return;
                        }
                    }
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_refreshes_coalesce_and_acknowledgements_cancel() {
        let mut state = RefreshState::default();
        state.update(RefreshEvent::Navigate(4));
        state.update(RefreshEvent::Navigate(6));
        assert_eq!(state.pending_sequence, Some(6));

        state.update(RefreshEvent::Acknowledge(5));
        assert_eq!(state.pending_sequence, Some(6));
        state.update(RefreshEvent::Acknowledge(6));
        assert_eq!(state.pending_sequence, None);

        state.update(RefreshEvent::MetadataChanged);
        assert!(state.needs_refresh());
    }
}
