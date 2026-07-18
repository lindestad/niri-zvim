use std::{io::Write, os::unix::net::UnixStream};

use anyhow::Context;
use niri_zvim::{Config, config_path, doctor_report, request_status, socket_path};
use niri_zvim_core::{Direction, NvimParent};
use serde::Serialize;

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
        ["doctor"] => print_doctor(false),
        ["doctor", "--json"] => print_doctor(true),
        ["config", "check"] => check_config(false),
        ["config", "check", "--json"] => check_config(true),
        ["config", "show"] => show_config(false),
        ["config", "show", "--json"] => show_config(true),
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

fn print_doctor(json: bool) -> anyhow::Result<()> {
    let report = doctor_report();
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        report.print_human();
    }
    if !report.healthy {
        std::process::exit(1);
    }
    Ok(())
}

#[derive(Serialize)]
struct ConfigSummary<'a> {
    valid: bool,
    path: &'a std::path::Path,
    source: &'static str,
    active_mode: &'a str,
}

#[derive(Serialize)]
struct ConfigDisplay<'a> {
    path: &'a std::path::Path,
    source: &'static str,
    config: &'a Config,
}

fn load_config() -> anyhow::Result<(std::path::PathBuf, &'static str, Config)> {
    let path = config_path();
    let source = if path.is_file() { "file" } else { "defaults" };
    let config = Config::load_validated()?;
    Ok((path, source, config))
}

fn check_config(json: bool) -> anyhow::Result<()> {
    let (path, source, config) = load_config()?;
    let summary = ConfigSummary {
        valid: true,
        path: &path,
        source,
        active_mode: config.active_mode_name(),
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&summary)?);
    } else {
        println!("valid: {} ({source})", path.display());
        println!("active mode: {}", config.active_mode_name());
    }
    Ok(())
}

fn show_config(json: bool) -> anyhow::Result<()> {
    let (path, source, config) = load_config()?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&ConfigDisplay {
                path: &path,
                source,
                config: &config,
            })?
        );
    } else {
        println!("path: {}", path.display());
        println!("source: {source}");
        println!("{}", serde_json::to_string_pretty(&config)?);
    }
    Ok(())
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
    "usage: niri-zvim <left|down|up|right|status [--json]|doctor [--json]|config <check|show> [--json]>"
}
