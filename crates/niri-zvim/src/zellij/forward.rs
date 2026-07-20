use std::{
    collections::hash_map::DefaultHasher,
    env,
    ffi::OsString,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    process::Stdio,
};

use anyhow::Context;
use niri_zvim_core::{DaemonMessage, ProtocolMessage, adapter_prelude};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt, copy},
    net::UnixStream,
    process::Command,
    runtime::Builder,
    time::{Duration, sleep},
};

use crate::{control::request_status, socket::socket_path};

use super::metadata::plugin_path;

const PLUGIN_START_DELAY: Duration = Duration::from_millis(50);

struct ForwardedIdentity {
    graph_session: String,
    plugin_configuration: String,
}

#[derive(Debug, PartialEq, Eq)]
enum Disconnect {
    Daemon,
    Plugin,
}

impl ForwardedIdentity {
    fn new(socket: &Path, session: &str, window_id: u64) -> Self {
        let mut hasher = DefaultHasher::new();
        socket.hash(&mut hasher);
        session.hash(&mut hasher);
        window_id.hash(&mut hasher);
        let id = format!("{:016x}", hasher.finish());
        Self {
            graph_session: format!("ssh:{id}:{session}"),
            plugin_configuration: format!("niri_zvim_bridge={id}"),
        }
    }
}

pub fn run_forwarded_bridge() -> anyhow::Result<()> {
    let explicit_socket = env::var_os("NIRI_ZVIM_SOCKET").filter(|path| !path.is_empty());
    anyhow::ensure!(
        explicit_socket.is_some(),
        "NIRI_ZVIM_SOCKET must name the SSH-forwarded local daemon socket"
    );
    let session = required_environment("ZELLIJ_SESSION_NAME")?;
    let socket = socket_path()?;
    let status = request_status()?;
    let window_id = status.niri.focused_window.context(
        "the forwarded daemon has no focused Niri window; focus this SSH terminal before starting the bridge",
    )?;
    let plugin = plugin_path();
    anyhow::ensure!(
        plugin.is_file(),
        "Zellij plugin not found at {}",
        plugin.display()
    );
    let identity = ForwardedIdentity::new(&socket, &session, window_id);

    Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the forwarded Zellij bridge runtime")?
        .block_on(relay(socket, plugin, session, identity, window_id))
}

async fn relay(
    socket: PathBuf,
    plugin: PathBuf,
    session: String,
    identity: ForwardedIdentity,
    window_id: u64,
) -> anyhow::Result<()> {
    let plugin_url = format!("file:{}", plugin.display());
    let status = Command::new("zellij")
        .args([
            "--session",
            &session,
            "action",
            "start-or-reload-plugin",
            "--configuration",
            &identity.plugin_configuration,
            &plugin_url,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .status()
        .await
        .with_context(|| format!("could not start the plugin in Zellij session {session}"))?;
    anyhow::ensure!(
        status.success(),
        "could not start the plugin in Zellij session {session}"
    );
    sleep(PLUGIN_START_DELAY).await;

    let mut pipe = Command::new("zellij")
        .args([
            "--session",
            &session,
            "pipe",
            "--name",
            "niri-zvim",
            "--plugin",
            &plugin_url,
            "--plugin-configuration",
            &identity.plugin_configuration,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("could not open a pipe to Zellij session {session}"))?;
    let pipe_input = pipe.stdin.take().context("Zellij pipe has no stdin")?;
    let pipe_output = pipe.stdout.take().context("Zellij pipe has no stdout")?;

    let daemon = UnixStream::connect(&socket).await.with_context(|| {
        format!(
            "could not connect to forwarded daemon at {}",
            socket.display()
        )
    })?;
    let binding = ProtocolMessage::new(DaemonMessage::ZellijSyncClient {
        session: session.clone(),
        graph_session: identity.graph_session.clone(),
        window_id,
    });
    let mut binding = serde_json::to_vec(&binding)?;
    binding.push(b'\n');

    println!(
        "forwarding Zellij {session:?} to Niri window {window_id} as {:?}",
        identity.graph_session
    );

    let disconnected = proxy(daemon, pipe_output, pipe_input, &binding).await?;
    let _ = pipe.kill().await;
    let status = pipe
        .wait()
        .await
        .context("could not wait for the Zellij plugin pipe")?;
    match disconnected {
        Disconnect::Daemon => anyhow::bail!("forwarded daemon disconnected"),
        Disconnect::Plugin => anyhow::bail!("Zellij plugin pipe exited with {status}"),
    }
}

async fn proxy<D, R, W>(
    mut daemon: D,
    mut plugin_output: R,
    mut plugin_input: W,
    binding: &[u8],
) -> anyhow::Result<Disconnect>
where
    D: AsyncRead + AsyncWrite + Unpin,
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    daemon.write_all(&adapter_prelude()).await?;
    plugin_input.write_all(binding).await?;
    plugin_input.flush().await?;

    let (mut daemon_input, mut daemon_output) = tokio::io::split(daemon);
    let daemon_to_plugin = copy(&mut daemon_input, &mut plugin_input);
    let plugin_to_daemon = copy(&mut plugin_output, &mut daemon_output);
    tokio::pin!(daemon_to_plugin, plugin_to_daemon);

    tokio::select! {
        result = &mut daemon_to_plugin => {
            result.context("could not forward daemon messages to Zellij")?;
            Ok(Disconnect::Daemon)
        }
        result = &mut plugin_to_daemon => {
            result.context("could not forward Zellij snapshots to the daemon")?;
            Ok(Disconnect::Plugin)
        }
    }
}

fn required_environment(name: &str) -> anyhow::Result<String> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(OsString::into_string)
        .transpose()
        .map_err(|_| anyhow::anyhow!("{name} is not valid UTF-8"))?
        .with_context(|| format!("{name} is required; run this command inside Zellij"))
}

#[cfg(test)]
mod tests {
    use niri_zvim_core::{AdapterMessage, Direction, ZellijClient, ZellijClientState};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader, duplex};

    use super::*;

    #[test]
    fn forwarded_identity_is_stable_and_connection_scoped() {
        let first = ForwardedIdentity::new(Path::new("/run/user/1/client-a.sock"), "work", 42);
        let repeated = ForwardedIdentity::new(Path::new("/run/user/1/client-a.sock"), "work", 42);
        let second = ForwardedIdentity::new(Path::new("/run/user/1/client-b.sock"), "work", 42);

        assert_eq!(first.graph_session, repeated.graph_session);
        assert_eq!(first.plugin_configuration, repeated.plugin_configuration);
        assert_ne!(first.graph_session, second.graph_session);
        assert!(first.graph_session.ends_with(":work"));
        assert!(first.plugin_configuration.starts_with("niri_zvim_bridge="));
    }

    #[tokio::test]
    async fn proxy_preserves_the_versioned_bidirectional_protocol() {
        let (relay_daemon, mut daemon) = duplex(4096);
        let (relay_plugin_output, mut plugin_output) = duplex(4096);
        let (mut plugin_input, relay_plugin_input) = duplex(4096);
        let mut binding =
            serde_json::to_vec(&ProtocolMessage::new(DaemonMessage::ZellijSyncClient {
                session: "work".into(),
                graph_session: "ssh:test:work".into(),
                window_id: 42,
            }))
            .unwrap();
        binding.push(b'\n');
        let relay = tokio::spawn(async move {
            proxy(
                relay_daemon,
                relay_plugin_output,
                relay_plugin_input,
                &binding,
            )
            .await
        });

        let mut prelude = [0; 2];
        daemon.read_exact(&mut prelude).await.unwrap();
        assert_eq!(prelude, adapter_prelude());

        let mut plugin_input = BufReader::new(&mut plugin_input);
        let mut line = String::new();
        plugin_input.read_line(&mut line).await.unwrap();
        let sync = serde_json::from_str::<ProtocolMessage<DaemonMessage>>(&line)
            .unwrap()
            .into_current()
            .unwrap();
        assert!(matches!(
            sync,
            DaemonMessage::ZellijSyncClient { window_id: 42, .. }
        ));

        let snapshot = AdapterMessage::ZellijSnapshot {
            state: ZellijClientState {
                client: ZellijClient {
                    session: "ssh:test:work".into(),
                    client_id: 1,
                },
                niri_window_id: 42,
                revision: 1,
                acknowledged_sequence: None,
                focused_pane: 2,
                pane_neighbors: Default::default(),
            },
        };
        let snapshot = format!(
            "{}\n",
            serde_json::to_string(&ProtocolMessage::new(snapshot)).unwrap()
        );
        plugin_output.write_all(snapshot.as_bytes()).await.unwrap();
        let mut daemon = BufReader::new(daemon);
        line.clear();
        daemon.read_line(&mut line).await.unwrap();
        assert!(
            serde_json::from_str::<ProtocolMessage<AdapterMessage>>(&line)
                .unwrap()
                .into_current()
                .is_ok()
        );

        let navigate = ProtocolMessage::new(DaemonMessage::ZellijNavigate {
            client_id: 1,
            sequence: 9,
            direction: Direction::Left,
        });
        let encoded = format!("{}\n", serde_json::to_string(&navigate).unwrap());
        daemon
            .get_mut()
            .write_all(encoded.as_bytes())
            .await
            .unwrap();
        line.clear();
        plugin_input.read_line(&mut line).await.unwrap();
        assert_eq!(
            serde_json::from_str::<ProtocolMessage<DaemonMessage>>(&line)
                .unwrap()
                .into_current()
                .unwrap(),
            navigate.message
        );

        drop(daemon);
        assert_eq!(relay.await.unwrap().unwrap(), Disconnect::Daemon);
    }
}
