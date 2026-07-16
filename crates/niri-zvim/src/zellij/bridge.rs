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
    time::{Duration, sleep},
};
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
