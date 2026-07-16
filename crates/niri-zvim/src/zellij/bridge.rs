use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use anyhow::Context;
use niri_zvim_core::{AdapterMessage, DaemonMessage};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::mpsc,
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

use crate::daemon::DaemonEvent;

use super::{
    metadata::{plugin_path, watch_session_metadata},
    snapshot::query_snapshot,
};

pub(super) async fn run_bridge(
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
    let (refresh_tx, refresh_rx) = mpsc::unbounded_channel();
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
        refresh_tx.clone(),
    ));
    let fallback_refresher = tokio::spawn(fallback_refresh_loop(
        session.to_owned(),
        window_id,
        events.clone(),
        sink.clone(),
        revisions.clone(),
        refresh_rx,
    ));

    let bind = serde_json::to_vec(&DaemonMessage::BindNiriWindow {
        window_id,
        session: session.to_owned(),
    })?;
    stdin.write_all(&bind).await?;
    stdin.write_u8(b'\n').await?;
    stdin.flush().await?;
    info!(%session, window_id, "connected Zellij bridge");

    let action_refresh = refresh_tx.clone();
    tokio::spawn(async move {
        while let Some(message) = actions.recv().await {
            let sequence = match &message {
                DaemonMessage::Navigate { sequence, .. } => Some(*sequence),
                DaemonMessage::BindNiriWindow { .. } => None,
            };
            let Ok(mut encoded) = serde_json::to_vec(&message) else {
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
        match serde_json::from_str::<AdapterMessage>(&line) {
            Ok(AdapterMessage::ZellijSnapshot { mut state }) => {
                if let Some(sequence) = state.acknowledged_sequence {
                    let _ = refresh_tx.send(RefreshEvent::Acknowledge(sequence));
                }
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
    fallback_refresher.abort();
    anyhow::bail!("Zellij pipe exited with {status}")
}

async fn fallback_refresh_loop(
    session: String,
    window_id: u64,
    events: mpsc::Sender<DaemonEvent>,
    sink: mpsc::UnboundedSender<DaemonMessage>,
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
                    let Ok(mut snapshot) = query_snapshot(&session, window_id, 0).await else {
                        break;
                    };
                    snapshot.revision = revisions.fetch_add(1, Ordering::Relaxed) + 1;
                    if events.send(DaemonEvent::Adapter {
                        message: AdapterMessage::ZellijSnapshot { state: snapshot },
                        sink: sink.clone(),
                    }).await.is_err() {
                        return;
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
