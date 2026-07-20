use std::{
    fs,
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
    assert_version(
        "niri-zvim-zellij-bridge",
        env!("CARGO_BIN_EXE_niri-zvim-zellij-bridge"),
    );
}

#[test]
fn zellij_bridge_requires_an_explicit_forwarded_socket() {
    let output = Command::new(env!("CARGO_BIN_EXE_niri-zvim-zellij-bridge"))
        .env_remove("NIRI_ZVIM_SOCKET")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("NIRI_ZVIM_SOCKET must name the SSH-forwarded local daemon socket")
    );
}

#[test]
fn zellij_bridge_requires_a_zellij_client() {
    let output = Command::new(env!("CARGO_BIN_EXE_niri-zvim-zellij-bridge"))
        .env("NIRI_ZVIM_SOCKET", "/run/user/1/forwarded.sock")
        .env_remove("ZELLIJ_SESSION_NAME")
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("ZELLIJ_SESSION_NAME is required")
    );
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

#[test]
fn config_check_and_show_report_the_resolved_file() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("config.json");
    fs::write(
        &config_path,
        r#"{
            "active_mode": "desktop",
            "modes": {
                "desktop": {
                    "left": "focus-column-or-monitor-left",
                    "down": "focus-window-or-workspace-down",
                    "up": "focus-window-or-workspace-up",
                    "right": "focus-column-or-monitor-right"
                }
            }
        }"#,
    )
    .unwrap();

    let check = run_config(&config_path, &["config", "check"]);
    assert!(check.status.success());
    let check = String::from_utf8(check.stdout).unwrap();
    assert!(check.contains(&format!("valid: {} (file)\n", config_path.display())));
    assert!(check.contains("active mode: desktop\n"));

    let show = run_config(&config_path, &["config", "show", "--json"]);
    assert!(show.status.success());
    let show: serde_json::Value = serde_json::from_slice(&show.stdout).unwrap();
    assert_eq!(show["path"], config_path.to_string_lossy().as_ref());
    assert_eq!(show["source"], "file");
    assert_eq!(show["config"]["active_mode"], "desktop");
    assert_eq!(
        show["config"]["zellij"]["terminal_app_ids"][0],
        "com.mitchellh.ghostty"
    );
}

#[test]
fn config_check_rejects_semantically_invalid_config() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("config.json");
    fs::write(
        &config_path,
        r#"{
            "active_mode": "missing",
            "modes": {}
        }"#,
    )
    .unwrap();

    let output = run_config(&config_path, &["config", "check"]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("navigation mode \"missing\" is not defined")
    );
}

#[test]
fn config_show_exposes_missing_file_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let config_path = temp.path().join("missing.json");
    let output = run_config(&config_path, &["config", "show", "--json"]);
    assert!(output.status.success());
    let output: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(output["source"], "defaults");
    assert_eq!(output["config"]["active_mode"], "default");
}

fn run_config(config_path: &std::path::Path, arguments: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_niri-zvim"))
        .args(arguments)
        .env("NIRI_ZVIM_CONFIG", config_path)
        .output()
        .unwrap()
}
