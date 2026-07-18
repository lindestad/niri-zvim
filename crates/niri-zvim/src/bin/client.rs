use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

use anyhow::Context;
use niri_zvim::socket_path;
use niri_zvim_core::{
    ControlResponse, DaemonStatus, Direction, NvimParent, ProtocolMessage, status_frame,
};

const STATUS_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_STATUS_BYTES: u64 = 1024 * 1024;

fn main() -> anyhow::Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    match arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["left"] => navigate(Direction::Left),
        ["down"] => navigate(Direction::Down),
        ["up"] => navigate(Direction::Up),
        ["right"] => navigate(Direction::Right),
        ["status"] => print_status(false),
        ["status", "--json"] => print_status(true),
        ["-V" | "--version"] => {
            println!("niri-zvim {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        ["-h" | "--help"] => {
            println!("{}", usage());
            Ok(())
        }
        _ => anyhow::bail!(usage()),
    }
}

fn navigate(direction: Direction) -> anyhow::Result<()> {
    let path = socket_path()?;
    let mut socket = UnixStream::connect(&path)
        .with_context(|| format!("could not connect to daemon at {}", path.display()))?;
    socket.write_all(&direction.control_frame())?;
    Ok(())
}

fn print_status(json: bool) -> anyhow::Result<()> {
    let status = request_status()?;
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    println!("daemon: niri-zvimd {}", status.version);
    println!("protocol: {}", status.protocol_version);
    println!("uptime: {}s", status.uptime_seconds);
    println!("mode: {}", status.active_mode);
    println!("socket: {}", status.socket_path);
    println!("navigation sequence: {}", status.navigation_sequence);
    println!(
        "niri: {} windows, focused {}, {} pending",
        status.niri.window_count,
        optional_id(status.niri.focused_window),
        status.niri.pending_navigations,
    );
    println!("zellij: {} clients", status.zellij.len());
    for client in &status.zellij {
        println!(
            "  {}#{}: niri {}, pane {}, {} panes, {}, {} pending",
            client.session,
            client.client_id,
            client.niri_window_id,
            client.focused_pane,
            client.pane_count,
            connection_state(client.connected),
            client.pending_navigations,
        );
    }
    println!("neovim: {} instances", status.nvim.len());
    for instance in &status.nvim {
        println!(
            "  {}: {}, window {}, {} windows, {}, {} pending",
            instance.id,
            parent_name(&instance.parent),
            instance.focused_window,
            instance.window_count,
            connection_state(instance.connected),
            instance.pending_navigations,
        );
    }
    Ok(())
}

fn request_status() -> anyhow::Result<DaemonStatus> {
    let path = socket_path()?;
    let mut socket = UnixStream::connect(&path)
        .with_context(|| format!("could not connect to daemon at {}", path.display()))?;
    socket.set_read_timeout(Some(STATUS_TIMEOUT))?;
    socket.set_write_timeout(Some(STATUS_TIMEOUT))?;
    socket.write_all(&status_frame())?;
    let mut encoded = Vec::new();
    socket
        .take(MAX_STATUS_BYTES + 1)
        .read_to_end(&mut encoded)
        .context("could not read daemon status")?;
    anyhow::ensure!(
        !encoded.is_empty(),
        "daemon returned no status; client and daemon protocols may not match"
    );
    anyhow::ensure!(
        encoded.len() as u64 <= MAX_STATUS_BYTES,
        "daemon status exceeds {MAX_STATUS_BYTES} bytes"
    );
    let response = serde_json::from_slice::<ProtocolMessage<ControlResponse>>(&encoded)
        .context("daemon returned an invalid status response")?
        .into_current()?;
    match response {
        ControlResponse::Status { status } => Ok(status),
    }
}

fn optional_id(id: Option<u64>) -> String {
    id.map_or_else(|| "none".into(), |id| id.to_string())
}

const fn connection_state(connected: bool) -> &'static str {
    if connected {
        "connected"
    } else {
        "disconnected"
    }
}

fn parent_name(parent: &NvimParent) -> String {
    match parent {
        NvimParent::FocusedNiriWindow => "unresolved niri window".into(),
        NvimParent::NiriWindow(window) => format!("niri {window}"),
        NvimParent::ZellijPane { client, pane_id } => {
            format!(
                "zellij {}#{} pane {pane_id}",
                client.session, client.client_id
            )
        }
    }
}

fn usage() -> &'static str {
    "usage: niri-zvim <left|down|up|right|status [--json]>"
}
