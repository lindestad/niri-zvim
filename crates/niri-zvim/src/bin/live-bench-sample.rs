use std::{
    process::{Command, ExitStatus},
    thread,
    time::{Duration, Instant},
};

use anyhow::Context;
use serde_json::Value;

const DEADLINE: Duration = Duration::from_secs(3);
const POLL_INTERVAL: Duration = Duration::from_micros(200);

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let backend = args.next().context("missing backend")?;
    let mode = args.next().context("missing mode")?;
    let direction = args.next().context("missing direction")?;
    let expected = args.next().context("missing expected focus")?;
    let count: usize = args
        .next()
        .as_deref()
        .unwrap_or("1")
        .parse()
        .context("invalid command count")?;
    anyhow::ensure!(count > 0, "command count must be positive");

    let start = Instant::now();
    for _ in 0..count {
        let status = trigger(&backend, &mode, &direction)?;
        anyhow::ensure!(
            status.success(),
            "{mode} {backend} trigger failed: {status}"
        );
    }
    let dispatched = start.elapsed();

    while !focus_matches(&backend, &expected)? {
        anyhow::ensure!(
            start.elapsed() < DEADLINE,
            "{backend} {mode} {direction} x{count} focus did not converge to {expected}"
        );
        thread::sleep(POLL_INTERVAL);
    }
    let converged = start.elapsed();
    println!("{} {}", dispatched.as_nanos(), converged.as_nanos());
    Ok(())
}

fn trigger(backend: &str, mode: &str, direction: &str) -> anyhow::Result<ExitStatus> {
    match mode {
        "daemon" => {
            Command::new(std::env::var_os("NIRI_ZVIM_CLIENT").unwrap_or_else(|| "niri-zvim".into()))
                .arg(direction)
                .status()
                .context("could not run niri-zvim")
        }
        "vim-niri-nav" => Command::new(required_env("NIRI_ZVIM_BENCH_VIM_NIRI_NAV")?)
            .arg(direction)
            .status()
            .context("could not run vim-niri-nav"),
        "native" => native_trigger(backend, direction),
        _ => anyhow::bail!("unknown mode {mode}"),
    }
}

fn native_trigger(backend: &str, direction: &str) -> anyhow::Result<ExitStatus> {
    match backend {
        "niri" => Command::new("niri")
            .args(["msg", "action", &format!("focus-column-{direction}")])
            .status()
            .context("could not run native Niri action"),
        "zellij" => Command::new("zellij")
            .args([
                "--session",
                &required_env("NIRI_ZVIM_BENCH_SESSION")?,
                "action",
                "move-focus",
                direction,
            ])
            .status()
            .context("could not run native Zellij action"),
        "nvim" => {
            let key = match direction {
                "left" => "h",
                "right" => "l",
                "up" => "k",
                "down" => "j",
                _ => anyhow::bail!("unknown Neovim direction {direction}"),
            };
            Command::new("nvim")
                .args([
                    "--headless",
                    "--clean",
                    "--server",
                    &required_env("NIRI_ZVIM_BENCH_NVIM")?,
                    "--remote-expr",
                    &format!("execute('wincmd {key}')"),
                ])
                .status()
                .context("could not run native Neovim action")
        }
        _ => anyhow::bail!("unknown backend {backend}"),
    }
}

fn focus_matches(backend: &str, expected: &str) -> anyhow::Result<bool> {
    match backend {
        "niri" => {
            let output = Command::new("niri")
                .args(["msg", "--json", "focused-window"])
                .output()
                .context("could not query Niri focus")?;
            anyhow::ensure!(output.status.success(), "Niri focus query failed");
            let value: Value = serde_json::from_slice(&output.stdout)?;
            Ok(value
                .get("id")
                .and_then(Value::as_u64)
                .map(|id| id.to_string())
                == Some(expected.to_owned()))
        }
        "zellij" => {
            let output = Command::new("zellij")
                .args([
                    "--session",
                    &required_env("NIRI_ZVIM_BENCH_SESSION")?,
                    "action",
                    "list-clients",
                ])
                .output()
                .context("could not query Zellij focus")?;
            anyhow::ensure!(output.status.success(), "Zellij focus query failed");
            let output = String::from_utf8(output.stdout)?;
            let focused = output
                .lines()
                .skip(1)
                .find_map(|line| line.split_whitespace().nth(1)?.strip_prefix("terminal_"));
            Ok(focused == Some(expected))
        }
        "nvim" => {
            let output = Command::new("nvim")
                .args([
                    "--headless",
                    "--clean",
                    "--server",
                    &required_env("NIRI_ZVIM_BENCH_NVIM")?,
                    "--remote-expr",
                    "win_getid()",
                ])
                .output()
                .context("could not query Neovim focus")?;
            anyhow::ensure!(output.status.success(), "Neovim focus query failed");
            Ok(String::from_utf8(output.stdout)?.trim() == expected)
        }
        _ => anyhow::bail!("unknown backend {backend}"),
    }
}

fn required_env(name: &str) -> anyhow::Result<String> {
    std::env::var(name).with_context(|| format!("{name} is not set"))
}
