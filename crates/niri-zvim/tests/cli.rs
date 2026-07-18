use std::process::Command;

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
