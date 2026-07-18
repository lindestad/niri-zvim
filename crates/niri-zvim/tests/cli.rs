use std::{
    io::{Read, Write},
    os::unix::net::UnixListener,
    process::Command,
    thread,
};

use niri_zvim_core::{
    ControlResponse, DaemonStatus, NiriStatus, PROTOCOL_VERSION, ProtocolMessage, status_frame,
};

#[test]
fn clients_report_the_package_version() {
    assert_version("niri-zvim", env!("CARGO_BIN_EXE_niri-zvim"));
    assert_version("niri-zvimd", env!("CARGO_BIN_EXE_niri-zvimd"));
}

fn assert_version(name: &str, executable: &str) {
    let output = Command::new(executable)
        .arg("--version")
        .output()
        .expect("version command should run");

    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("version output should be UTF-8"),
        format!("{name} {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn status_supports_human_and_json_output() {
    let human = run_status(&["status"]);
    assert!(human.status.success());
    let human = String::from_utf8(human.stdout).unwrap();
    assert!(human.contains(&format!(
        "daemon: niri-zvimd {}\n",
        env!("CARGO_PKG_VERSION")
    )));
    assert!(human.contains("niri: 4 windows, focused 42, 0 pending\n"));

    let json = run_status(&["status", "--json"]);
    assert!(json.status.success());
    assert_eq!(
        serde_json::from_slice::<DaemonStatus>(&json.stdout).unwrap(),
        status_fixture()
    );
}

fn run_status(arguments: &[&str]) -> std::process::Output {
    let temp = tempfile::tempdir().unwrap();
    let socket = temp.path().join("daemon.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let status = status_fixture();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0; 3];
        stream.read_exact(&mut request).unwrap();
        assert_eq!(request, status_frame());
        let response = ProtocolMessage::new(ControlResponse::Status { status });
        writeln!(stream, "{}", serde_json::to_string(&response).unwrap()).unwrap();
    });
    let output = Command::new(env!("CARGO_BIN_EXE_niri-zvim"))
        .args(arguments)
        .env("NIRI_ZVIM_SOCKET", socket)
        .output()
        .unwrap();
    server.join().unwrap();
    output
}

fn status_fixture() -> DaemonStatus {
    DaemonStatus {
        version: env!("CARGO_PKG_VERSION").into(),
        protocol_version: PROTOCOL_VERSION,
        uptime_seconds: 12,
        active_mode: "desktop".into(),
        socket_path: "/run/user/1000/niri-zvim.sock".into(),
        navigation_sequence: 9,
        niri: NiriStatus {
            window_count: 4,
            focused_window: Some(42),
            pending_navigations: 0,
        },
        zellij: Vec::new(),
        nvim: Vec::new(),
    }
}
