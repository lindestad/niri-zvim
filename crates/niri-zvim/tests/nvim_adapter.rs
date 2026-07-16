use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixListener,
    process::{Command, Stdio},
    time::Duration,
};

use niri_zvim_core::{AdapterMessage, DaemonMessage, Direction};

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
    let mut magic = [0];
    stream.read_exact(&mut magic).unwrap();
    assert_eq!(magic[0], niri_zvim::adapter_magic());
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
        .map(|command| serde_json::to_string(command).unwrap())
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

fn read_message(reader: &mut BufReader<std::os::unix::net::UnixStream>) -> AdapterMessage {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    if line.is_empty() {
        panic!("Neovim adapter disconnected");
    }
    serde_json::from_str(&line)
        .unwrap_or_else(|error| panic!("invalid adapter message {line:?}: {error}"))
}
