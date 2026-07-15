use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use anyhow::Context;
use niri_zvim_core::{AdapterMessage, DaemonMessage, NiriWindow};
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
            let Some(session) = window.title.as_deref() else {
                continue;
            };
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

async fn run_bridge(
    session: &str,
    window_id: u64,
    events: mpsc::Sender<DaemonEvent>,
) -> anyhow::Result<()> {
    let plugin = plugin_path();
    anyhow::ensure!(plugin.is_file(), "plugin not found at {}", plugin.display());
    let mut child = Command::new("zellij")
        .args([
            "--session",
            session,
            "pipe",
            "--plugin",
            &format!("file:{}", plugin.display()),
            "--name",
            "niri-zvim",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .with_context(|| format!("could not connect to Zellij session {session}"))?;
    let mut stdin = child.stdin.take().context("Zellij pipe has no stdin")?;
    let stdout = child.stdout.take().context("Zellij pipe has no stdout")?;
    let (sink, mut actions) = mpsc::unbounded_channel();

    let bind = serde_json::to_vec(&DaemonMessage::BindNiriWindow { window_id })?;
    stdin.write_all(&bind).await?;
    stdin.write_u8(b'\n').await?;
    info!(%session, window_id, "connected Zellij bridge");

    tokio::spawn(async move {
        while let Some(message) = actions.recv().await {
            let Ok(mut encoded) = serde_json::to_vec(&message) else {
                continue;
            };
            encoded.push(b'\n');
            if stdin.write_all(&encoded).await.is_err() {
                break;
            }
        }
    });

    let mut lines = BufReader::new(stdout).lines();
    while let Some(line) = lines.next_line().await? {
        match serde_json::from_str::<AdapterMessage>(&line) {
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
    anyhow::bail!("Zellij pipe exited with {status}")
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
