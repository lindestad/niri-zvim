use std::{
    env, fs,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Output},
};

use serde::Serialize;

use crate::{
    config::{Config, config_path},
    request_status,
    socket::socket_path,
    zellij::configured_plugin_path,
};

const NIRI_VERSION: &str = "26.04";
const ZELLIJ_VERSION: &str = "0.44.3";
const NVIM_VERSION: &str = "0.12.4";
const ZELLIJ_PERMISSIONS: &[&str] = &[
    "ReadApplicationState",
    "ChangeApplicationState",
    "ReadCliPipes",
    "ReadSessionEnvironmentVariables",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DoctorLevel {
    Pass,
    Warning,
    Fail,
}

impl DoctorLevel {
    const fn label(self) -> &'static str {
        match self {
            Self::Pass => "PASS",
            Self::Warning => "WARN",
            Self::Fail => "FAIL",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorCheck {
    pub name: String,
    pub level: DoctorLevel,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub version: String,
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn print_human(&self) {
        for check in &self.checks {
            println!(
                "{:<4} {:<20} {}",
                check.level.label(),
                check.name,
                check.message
            );
        }
        let passed = self
            .checks
            .iter()
            .filter(|check| check.level == DoctorLevel::Pass)
            .count();
        let warnings = self
            .checks
            .iter()
            .filter(|check| check.level == DoctorLevel::Warning)
            .count();
        let failed = self
            .checks
            .iter()
            .filter(|check| check.level == DoctorLevel::Fail)
            .count();
        let warning_label = if warnings == 1 { "warning" } else { "warnings" };
        println!();
        println!("doctor: {passed} passed, {warnings} {warning_label}, {failed} failed");
    }
}

pub fn doctor_report() -> DoctorReport {
    let mut checks = Vec::new();
    check_config(&mut checks);
    check_version(
        &mut checks,
        "niri version",
        "niri",
        &["--version"],
        NIRI_VERSION,
        |output| output.split_whitespace().nth(1).map(str::to_owned),
    );
    check_niri_socket(&mut checks);
    check_service(&mut checks, "daemon service", "niri-zvim.service");
    check_daemon(&mut checks);
    check_daemon_socket(&mut checks);
    check_version(
        &mut checks,
        "zellij version",
        "zellij",
        &["--version"],
        ZELLIJ_VERSION,
        |output| output.split_whitespace().nth(1).map(str::to_owned),
    );
    check_zellij_plugin(&mut checks);
    check_zellij_permissions(&mut checks);
    check_version(
        &mut checks,
        "neovim version",
        "nvim",
        &["--version"],
        NVIM_VERSION,
        |output| {
            output
                .lines()
                .next()?
                .strip_prefix("NVIM v")
                .map(str::to_owned)
        },
    );
    check_neovim_adapter(&mut checks);
    check_terminal_configs(&mut checks);
    let healthy = checks.iter().all(|check| check.level != DoctorLevel::Fail);
    DoctorReport {
        version: env!("CARGO_PKG_VERSION").into(),
        healthy,
        checks,
    }
}

fn check_config(checks: &mut Vec<DoctorCheck>) {
    let path = config_path();
    if !path.exists() {
        warning(
            checks,
            "config",
            format!("{} is missing; using built-in defaults", path.display()),
        );
        return;
    }
    match Config::load_validated().and_then(|config| {
        let mode = config.active_mode_name().to_owned();
        let app_ids = config.zellij_discovery()?.terminal_app_ids.clone();
        Ok((mode, app_ids))
    }) {
        Ok((mode, app_ids)) => {
            pass(
                checks,
                "config",
                format!(
                    "{} selects mode {mode:?} and Zellij app IDs [{}]",
                    path.display(),
                    app_ids.join(", ")
                ),
            );
        }
        Err(error) => {
            fail(checks, "config", error.to_string());
        }
    }
}

fn check_niri_socket(checks: &mut Vec<DoctorCheck>) {
    let Some(path) = env::var_os("NIRI_SOCKET").filter(|path| !path.is_empty()) else {
        fail(checks, "niri socket", "NIRI_SOCKET is not set");
        return;
    };
    let path = PathBuf::from(path);
    match fs::metadata(&path) {
        Ok(metadata) if metadata.file_type().is_socket() => {
            pass(checks, "niri socket", path.display().to_string());
        }
        Ok(_) => fail(
            checks,
            "niri socket",
            format!("{} is not a Unix socket", path.display()),
        ),
        Err(error) => fail(
            checks,
            "niri socket",
            format!("{}: {error}", path.display()),
        ),
    }
}

fn check_daemon(checks: &mut Vec<DoctorCheck>) {
    match request_status() {
        Ok(status) if status.version != env!("CARGO_PKG_VERSION") => fail(
            checks,
            "daemon protocol",
            format!(
                "client {} is talking to daemon {}",
                env!("CARGO_PKG_VERSION"),
                status.version
            ),
        ),
        Ok(status) => {
            pass(
                checks,
                "daemon protocol",
                format!(
                    "niri-zvimd {}, protocol {}",
                    status.version, status.protocol_version
                ),
            );
            if status.niri.window_count == 0 {
                warning(
                    checks,
                    "daemon graph",
                    "daemon has not observed any Niri windows",
                );
            } else {
                pass(
                    checks,
                    "daemon graph",
                    format!(
                        "{} Niri windows, {} Zellij clients, {} Neovim instances",
                        status.niri.window_count,
                        status.zellij.len(),
                        status.nvim.len()
                    ),
                );
            }
        }
        Err(error) => fail(checks, "daemon protocol", error.to_string()),
    }
}

fn check_daemon_socket(checks: &mut Vec<DoctorCheck>) {
    let path = match socket_path() {
        Ok(path) => path,
        Err(error) => {
            fail(checks, "daemon socket", error.to_string());
            return;
        }
    };
    match fs::metadata(&path) {
        Ok(metadata) if !metadata.file_type().is_socket() => fail(
            checks,
            "daemon socket",
            format!("{} is not a Unix socket", path.display()),
        ),
        Ok(metadata) => {
            let mode = metadata.permissions().mode() & 0o777;
            if mode == 0o600 {
                pass(
                    checks,
                    "daemon socket",
                    format!("{} has mode 0600", path.display()),
                );
            } else {
                fail(
                    checks,
                    "daemon socket",
                    format!("{} has mode {mode:04o}, expected 0600", path.display()),
                );
            }
        }
        Err(error) => fail(
            checks,
            "daemon socket",
            format!("{}: {error}", path.display()),
        ),
    }
}

fn check_zellij_plugin(checks: &mut Vec<DoctorCheck>) {
    let path = configured_plugin_path();
    match fs::read(&path) {
        Ok(bytes) if bytes.starts_with(b"\0asm") => {
            pass(checks, "zellij plugin", path.display().to_string());
        }
        Ok(_) => fail(
            checks,
            "zellij plugin",
            format!("{} is not a WASM module", path.display()),
        ),
        Err(error) => fail(
            checks,
            "zellij plugin",
            format!("{}: {error}", path.display()),
        ),
    }
}

fn check_zellij_permissions(checks: &mut Vec<DoctorCheck>) {
    let plugin = configured_plugin_path();
    let permissions = cache_home().join("zellij/permissions.kdl");
    let contents = match fs::read_to_string(&permissions) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            warning(
                checks,
                "zellij permissions",
                format!(
                    "{} is missing; approve the plugin prompt",
                    permissions.display()
                ),
            );
            return;
        }
        Err(error) => {
            warning(
                checks,
                "zellij permissions",
                format!("{}: {error}", permissions.display()),
            );
            return;
        }
    };
    match missing_zellij_permissions(&contents, &plugin) {
        None => warning(
            checks,
            "zellij permissions",
            format!(
                "{} has no entry for {}",
                permissions.display(),
                plugin.display()
            ),
        ),
        Some(missing) if missing.is_empty() => pass(
            checks,
            "zellij permissions",
            format!("{} grants the required permissions", permissions.display()),
        ),
        Some(missing) => warning(
            checks,
            "zellij permissions",
            format!("missing {} for {}", missing.join(", "), plugin.display()),
        ),
    }
}

fn missing_zellij_permissions(contents: &str, plugin: &Path) -> Option<Vec<&'static str>> {
    let quoted = serde_json::to_string(&plugin.to_string_lossy()).ok()?;
    let marker = format!("{quoted} {{");
    let block = contents.split_once(&marker)?.1.split_once('}')?.0;
    Some(
        ZELLIJ_PERMISSIONS
            .iter()
            .copied()
            .filter(|permission| !block.lines().any(|line| line.trim() == *permission))
            .collect(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalConfigFinding {
    line: usize,
    message: &'static str,
}

fn check_terminal_configs(checks: &mut Vec<DoctorCheck>) {
    let config = config_home();
    let home = env::var_os("HOME").map(PathBuf::from);

    if command_exists("ghostty") {
        let mut paths = vec![
            config.join("ghostty/config"),
            config.join("ghostty/config.ghostty"),
        ];
        if let Some(home) = &home {
            paths.push(home.join(".config/ghostty/config"));
            paths.push(home.join(".config/ghostty/config.ghostty"));
        }
        check_terminal_config(checks, &paths, ghostty_config_findings);
    }

    if command_exists("alacritty") {
        let mut paths = vec![
            config.join("alacritty/alacritty.toml"),
            config.join("alacritty.toml"),
        ];
        if let Some(home) = &home {
            paths.push(home.join(".config/alacritty/alacritty.toml"));
            paths.push(home.join(".alacritty.toml"));
        }
        check_terminal_config(checks, &paths, alacritty_config_findings);
    }

    if command_exists("foot") {
        let mut paths = vec![config.join("foot/foot.ini")];
        if let Some(home) = &home {
            paths.push(home.join(".config/foot/foot.ini"));
        }
        check_terminal_config(checks, &paths, foot_config_findings);
    }
}

fn check_terminal_config(
    checks: &mut Vec<DoctorCheck>,
    paths: &[PathBuf],
    inspect: fn(&str) -> Vec<TerminalConfigFinding>,
) {
    let Some(path) = paths.iter().find(|path| path.is_file()) else {
        return;
    };
    let Ok(contents) = fs::read_to_string(path) else {
        return;
    };
    for finding in inspect(&contents) {
        warning(
            checks,
            "terminal config",
            format!("{}:{}: {}", path.display(), finding.line, finding.message),
        );
    }
}

fn ghostty_config_findings(contents: &str) -> Vec<TerminalConfigFinding> {
    key_value_lines(contents)
        .filter(|(_, section, key, value)| {
            section.is_empty() && *key == "title" && !empty_config_value(value)
        })
        .map(|(line, _, _, _)| TerminalConfigFinding {
            line,
            message: "title fixes the window title and prevents Zellij session discovery",
        })
        .collect()
}

fn alacritty_config_findings(contents: &str) -> Vec<TerminalConfigFinding> {
    key_value_lines(contents)
        .filter(|(_, section, key, value)| {
            ((*section == "window" && *key == "dynamic_title")
                || (section.is_empty() && *key == "window.dynamic_title"))
                && value.trim() == "false"
        })
        .map(|(line, _, _, _)| TerminalConfigFinding {
            line,
            message: "window.dynamic_title = false prevents Zellij session discovery",
        })
        .collect()
}

fn foot_config_findings(contents: &str) -> Vec<TerminalConfigFinding> {
    key_value_lines(contents)
        .filter(|(_, section, key, value)| {
            (section.is_empty() || *section == "main")
                && *key == "locked-title"
                && matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "yes" | "true" | "on" | "1"
                )
        })
        .map(|(line, _, _, _)| TerminalConfigFinding {
            line,
            message: "locked-title prevents Zellij session discovery",
        })
        .collect()
}

fn key_value_lines(contents: &str) -> impl Iterator<Item = (usize, &str, &str, &str)> {
    let mut section = "";
    contents
        .lines()
        .enumerate()
        .filter_map(move |(index, raw)| {
            let line = uncommented(raw).trim();
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].trim();
                return None;
            }
            let (key, value) = line.split_once('=')?;
            Some((index + 1, section, key.trim(), value.trim()))
        })
}

fn uncommented(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && quote == Some('"') {
            escaped = true;
            continue;
        }
        if matches!(character, '\'' | '"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            }
            continue;
        }
        if character == '#' && quote.is_none() {
            return &line[..index];
        }
    }
    line
}

fn empty_config_value(value: &str) -> bool {
    matches!(value.trim(), "" | "\"\"" | "''")
}

fn command_exists(command: &str) -> bool {
    env::var_os("PATH").is_some_and(|path| {
        env::split_paths(&path).any(|directory| {
            fs::metadata(directory.join(command)).is_ok_and(|metadata| {
                metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
            })
        })
    })
}

fn check_neovim_adapter(checks: &mut Vec<DoctorCheck>) {
    let site = data_home().join("nvim/site");
    let files = [
        site.join("lua/niri-zvim/init.lua"),
        site.join("plugin/niri-zvim.lua"),
    ];
    let missing: Vec<_> = files.iter().filter(|path| !path.is_file()).collect();
    if missing.is_empty() {
        pass(
            checks,
            "neovim adapter",
            format!(
                "installed under {}; explicit setup() is required",
                site.display()
            ),
        );
    } else {
        fail(
            checks,
            "neovim adapter",
            format!(
                "missing {}",
                missing
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }
}

fn check_service(checks: &mut Vec<DoctorCheck>, name: &str, unit: &str) {
    match command_output("systemctl", &["--user", "is-active", unit]) {
        Ok(output) if output.status.success() && output_text(&output).trim() == "active" => {
            pass(checks, name, format!("{unit} is active"));
        }
        Ok(output) => fail(
            checks,
            name,
            format!("{unit} is not active ({})", output_text(&output).trim()),
        ),
        Err(error) => fail(checks, name, error),
    }
}

fn check_version<F>(
    checks: &mut Vec<DoctorCheck>,
    name: &str,
    command: &str,
    arguments: &[&str],
    expected: &str,
    parse: F,
) where
    F: FnOnce(&str) -> Option<String>,
{
    check_version_with(checks, name, command, arguments, parse, |found| {
        if found == expected {
            Ok(found.to_owned())
        } else {
            Err(format!("found {found}, expected {expected}"))
        }
    });
}

fn check_version_with<F, V>(
    checks: &mut Vec<DoctorCheck>,
    name: &str,
    command: &str,
    arguments: &[&str],
    parse: F,
    validate: V,
) where
    F: FnOnce(&str) -> Option<String>,
    V: FnOnce(&str) -> Result<String, String>,
{
    match command_output(command, arguments) {
        Ok(output) if !output.status.success() => fail(
            checks,
            name,
            format!("{command} exited with {}", output.status),
        ),
        Ok(output) => match parse(&output_text(&output)) {
            Some(found) => match validate(&found) {
                Ok(message) => pass(checks, name, message),
                Err(message) => fail(checks, name, message),
            },
            None => fail(checks, name, format!("could not parse {command} version")),
        },
        Err(error) => fail(checks, name, error),
    }
}

fn command_output(command: &str, arguments: &[&str]) -> Result<Output, String> {
    Command::new(command)
        .args(arguments)
        .output()
        .map_err(|error| format!("could not run {command}: {error}"))
}

fn output_text(output: &Output) -> String {
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text
}

fn config_home() -> PathBuf {
    env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn cache_home() -> PathBuf {
    env::var_os("XDG_CACHE_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn data_home() -> PathBuf {
    env::var_os("XDG_DATA_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn pass(checks: &mut Vec<DoctorCheck>, name: &str, message: impl Into<String>) {
    push(checks, name, DoctorLevel::Pass, message);
}

fn warning(checks: &mut Vec<DoctorCheck>, name: &str, message: impl Into<String>) {
    push(checks, name, DoctorLevel::Warning, message);
}

fn fail(checks: &mut Vec<DoctorCheck>, name: &str, message: impl Into<String>) {
    push(checks, name, DoctorLevel::Fail, message);
}

fn push(checks: &mut Vec<DoctorCheck>, name: &str, level: DoctorLevel, message: impl Into<String>) {
    checks.push(DoctorCheck {
        name: name.into(),
        level,
        message: message.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_check_is_scoped_to_the_configured_plugin() {
        let contents = r#"
"/other/plugin.wasm" {
    ReadApplicationState
}
"/home/user/.config/zellij/plugins/niri-zvim.wasm" {
    ReadApplicationState
    ChangeApplicationState
    ReadCliPipes
    ReadSessionEnvironmentVariables
}
"#;
        assert_eq!(
            missing_zellij_permissions(
                contents,
                Path::new("/home/user/.config/zellij/plugins/niri-zvim.wasm")
            ),
            Some(Vec::new())
        );
        assert!(missing_zellij_permissions(contents, Path::new("/missing.wasm")).is_none());
    }

    #[test]
    fn permission_check_reports_individual_missing_permissions() {
        let contents = r#"
"/plugin.wasm" {
    ReadApplicationState
    ReadCliPipes
}
"#;
        assert_eq!(
            missing_zellij_permissions(contents, Path::new("/plugin.wasm")),
            Some(vec![
                "ChangeApplicationState",
                "ReadSessionEnvironmentVariables"
            ])
        );
    }

    #[test]
    fn ghostty_config_reports_only_a_fixed_nonempty_title() {
        let contents = r#"
# title = ignored
title =
title = ""
title = "project # fixed"
"#;
        assert_eq!(
            ghostty_config_findings(contents),
            vec![TerminalConfigFinding {
                line: 5,
                message: "title fixes the window title and prevents Zellij session discovery",
            }]
        );
    }

    #[test]
    fn alacritty_config_reports_disabled_dynamic_titles() {
        let section = r#"
[window]
dynamic_title = false # incompatible
title = "static initial title"
"#;
        assert_eq!(
            alacritty_config_findings(section),
            vec![TerminalConfigFinding {
                line: 3,
                message: "window.dynamic_title = false prevents Zellij session discovery",
            }]
        );

        let dotted = "window.dynamic_title = false\n";
        assert_eq!(alacritty_config_findings(dotted).len(), 1);
        assert!(alacritty_config_findings("[window]\ndynamic_title = true\n").is_empty());
    }

    #[test]
    fn foot_config_reports_only_enabled_title_locking() {
        let contents = r#"
[main]
locked-title=no
locked-title = YES
"#;
        assert_eq!(
            foot_config_findings(contents),
            vec![TerminalConfigFinding {
                line: 4,
                message: "locked-title prevents Zellij session discovery",
            }]
        );
    }
}
