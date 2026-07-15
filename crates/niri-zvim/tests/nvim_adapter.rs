use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixListener,
    process::{Command, Stdio},
    time::Duration,
};

use niri_zvim_core::{AdapterMessage, DaemonMessage, Direction};

#[test]
fn nvim_publishes_topology_and_accepts_navigation() {
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
            && state.window_neighbors.len() == 2
        {
            break state;
        }
    };
    let neighbors = &initial.window_neighbors[&initial.focused_window.to_string()];
    let direction = Direction::ALL
        .into_iter()
        .find(|direction| neighbors.get(*direction).is_some())
        .expect("a two-split layout has a directional neighbor");
    let command = DaemonMessage::Navigate {
        sequence: 1,
        direction,
    };
    writeln!(writer, "{}", serde_json::to_string(&command).unwrap()).unwrap();

    loop {
        let message = read_message(&mut reader);
        if let AdapterMessage::NvimSnapshot { state } = message
            && state.focused_window != initial.focused_window
        {
            break;
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
