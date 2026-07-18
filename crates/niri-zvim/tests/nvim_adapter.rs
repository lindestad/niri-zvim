use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixListener,
    process::{Command, Stdio},
    time::Duration,
};

use niri_zvim_core::{AdapterMessage, DaemonMessage, Direction, ProtocolMessage, adapter_prelude};

#[test]
fn nvim_publishes_topology_and_drains_queued_navigation() {
    let temp = tempfile::tempdir().unwrap();
    let socket_path = temp.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../nvim")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("nvim")
        .args([
            "--clean",
            "--headless",
            "--cmd",
            &format!("set runtimepath+={}", plugin.display()),
            "--cmd",
            "set splitright | vsplit | vsplit",
            "--cmd",
            r#"lua require("niri-zvim").setup()"#,
        ])
        .env("NIRI_ZVIM_SOCKET", &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Neovim must be installed for adapter tests");

    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut prelude = [0; 2];
    stream.read_exact(&mut prelude).unwrap();
    assert_eq!(prelude, adapter_prelude());
    let mut writer = stream.try_clone().unwrap();
    let mut reader = BufReader::new(stream);

    let initial = loop {
        let message = read_message(&mut reader);
        if let AdapterMessage::NvimSnapshot { state } = message
            && state.window_neighbors.len() == 3
        {
            break state;
        }
    };
    let (direction, first_target, final_target) = Direction::ALL
        .into_iter()
        .find_map(|direction| {
            let first = initial.window_neighbors[&initial.focused_window.to_string()]
                .get(direction)
                .copied()?;
            let second = initial.window_neighbors[&first.to_string()]
                .get(direction)
                .copied()?;
            Some((direction, first, second))
        })
        .expect("a three-split layout has two neighbors in one direction");
    let commands = [
        DaemonMessage::Navigate {
            sequence: 1,
            direction,
        },
        DaemonMessage::Navigate {
            sequence: 2,
            direction,
        },
    ];
    let encoded = commands
        .iter()
        .map(|command| serde_json::to_string(&ProtocolMessage::new(command)).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    writeln!(writer, "{encoded}").unwrap();

    loop {
        if let AdapterMessage::NvimSnapshot { state } = read_message(&mut reader) {
            if state.focused_window == final_target {
                assert_eq!(state.acknowledged_sequence, Some(2));
                break;
            }
            assert_ne!(
                state.focused_window, initial.focused_window,
                "queued navigation returned to its initial window"
            );
            assert_eq!(
                state.focused_window, first_target,
                "queued navigation focused an unrelated window"
            );
        }
    }

    child.kill().ok();
    child.wait().ok();
}

#[test]
fn nvim_reports_a_focused_float_outside_normal_window_topology() {
    let temp = tempfile::tempdir().unwrap();
    let socket_path = temp.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../nvim")
        .canonicalize()
        .unwrap();
    let mut child = Command::new("nvim")
        .args([
            "--clean",
            "--headless",
            "--cmd",
            &format!("set runtimepath+={}", plugin.display()),
            "--cmd",
            "vsplit",
            "--cmd",
            r#"autocmd VimEnter * ++once lua local buffer = vim.api.nvim_create_buf(false, true); vim.api.nvim_open_win(buffer, true, { relative = "editor", row = 1, col = 1, width = 10, height = 2 })"#,
            "--cmd",
            r#"lua require("niri-zvim").setup()"#,
        ])
        .env("NIRI_ZVIM_SOCKET", &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Neovim must be installed for adapter tests");

    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut prelude = [0; 2];
    stream.read_exact(&mut prelude).unwrap();
    assert_eq!(prelude, adapter_prelude());
    let mut reader = BufReader::new(stream);

    loop {
        if let AdapterMessage::NvimSnapshot { state } = read_message(&mut reader)
            && state.window_neighbors.len() == 2
            && !state
                .window_neighbors
                .contains_key(&state.focused_window.to_string())
        {
            break;
        }
    }

    child.kill().ok();
    child.wait().ok();
}

#[test]
fn nvim_runtime_installation_does_not_connect_without_setup() {
    let temp = tempfile::tempdir().unwrap();
    let socket_path = temp.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    listener.set_nonblocking(true).unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../nvim")
        .canonicalize()
        .unwrap();
    let status = Command::new("nvim")
        .args([
            "--clean",
            "--headless",
            "--cmd",
            &format!("set runtimepath+={}", plugin.display()),
            "-c",
            "lua vim.wait(100)",
            "-c",
            "qa",
        ])
        .env("NIRI_ZVIM_SOCKET", &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .expect("Neovim must be installed for adapter tests");

    assert!(status.success());
    let error = listener.accept().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
}

#[test]
fn nvim_setup_options_and_disable_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let socket_path = temp.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let plugin = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../nvim")
        .canonicalize()
        .unwrap();
    let lua_socket = serde_json::to_string(&socket_path.to_string_lossy()).unwrap();
    let setup = format!(
        r#"lua require("niri-zvim").setup({{ socket_path = {lua_socket}, reconnect_interval_ms = 10 }})"#
    );
    let mut child = Command::new("nvim")
        .args([
            "--clean",
            "--headless",
            "--cmd",
            &format!("set runtimepath+={}", plugin.display()),
            "--cmd",
            &setup,
            "-c",
            r#"lua vim.defer_fn(function() require("niri-zvim").disable() end, 50); vim.wait(150)"#,
            "-c",
            "qa",
        ])
        .env("NIRI_ZVIM_SOCKET", temp.path().join("wrong.sock"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Neovim must be installed for adapter tests");

    let (mut stream, _) = listener.accept().unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut prelude = [0; 2];
    stream.read_exact(&mut prelude).unwrap();
    assert_eq!(prelude, adapter_prelude());
    let mut reader = BufReader::new(stream);
    let mut saw_snapshot = false;
    loop {
        match read_message(&mut reader) {
            AdapterMessage::NvimSnapshot { .. } => saw_snapshot = true,
            AdapterMessage::NvimClosed { .. } => break,
            AdapterMessage::ZellijSnapshot { .. } | AdapterMessage::ZellijClients { .. } => {}
        }
    }
    assert!(saw_snapshot);

    assert!(child.wait().unwrap().success());
}

fn read_message(reader: &mut BufReader<std::os::unix::net::UnixStream>) -> AdapterMessage {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    if line.is_empty() {
        panic!("Neovim adapter disconnected");
    }
    serde_json::from_str::<ProtocolMessage<AdapterMessage>>(&line)
        .unwrap_or_else(|error| panic!("invalid adapter message {line:?}: {error}"))
        .into_current()
        .unwrap()
}
