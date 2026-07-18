use std::{collections::BTreeMap, fs, io::ErrorKind, path::PathBuf};

use anyhow::Context;
use niri_zvim_core::Direction;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum NiriNavigation {
    #[serde(rename = "focus-column-left")]
    ColumnLeft,
    #[serde(rename = "focus-column-right")]
    ColumnRight,
    #[serde(rename = "focus-column-or-monitor-left")]
    ColumnOrMonitorLeft,
    #[serde(rename = "focus-column-or-monitor-right")]
    ColumnOrMonitorRight,
    #[serde(rename = "focus-window-down")]
    WindowDown,
    #[serde(rename = "focus-window-up")]
    WindowUp,
    #[serde(rename = "focus-window-or-workspace-down")]
    WindowOrWorkspaceDown,
    #[serde(rename = "focus-window-or-workspace-up")]
    WindowOrWorkspaceUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NavigationMode {
    pub left: NiriNavigation,
    pub down: NiriNavigation,
    pub up: NiriNavigation,
    pub right: NiriNavigation,
}

impl NavigationMode {
    pub const fn workspace_local() -> Self {
        Self {
            left: NiriNavigation::ColumnLeft,
            down: NiriNavigation::WindowDown,
            up: NiriNavigation::WindowUp,
            right: NiriNavigation::ColumnRight,
        }
    }

    pub const fn get(self, direction: Direction) -> NiriNavigation {
        match direction {
            Direction::Left => self.left,
            Direction::Down => self.down,
            Direction::Up => self.up,
            Direction::Right => self.right,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    active_mode: String,
    modes: BTreeMap<String, NavigationMode>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            active_mode: "default".into(),
            modes: BTreeMap::from([("default".into(), NavigationMode::workspace_local())]),
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path();
        let contents = match fs::read_to_string(&path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(error) => {
                return Err(error).with_context(|| format!("could not read {}", path.display()));
            }
        };
        serde_json::from_str(&contents)
            .with_context(|| format!("could not parse {}", path.display()))
    }

    pub fn active_mode(&self) -> anyhow::Result<NavigationMode> {
        self.modes
            .get(&self.active_mode)
            .copied()
            .with_context(|| format!("navigation mode {:?} is not defined", self.active_mode))
    }

    pub fn active_mode_name(&self) -> &str {
        &self.active_mode
    }
}

fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRI_ZVIM_CONFIG") {
        return path.into();
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    config.join("niri-zvim/config.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_config_defaults_to_workspace_local_navigation() {
        let config = Config::default();
        let mode = config.active_mode().unwrap();

        assert_eq!(mode.left, NiriNavigation::ColumnLeft);
        assert_eq!(mode.up, NiriNavigation::WindowUp);
    }

    #[test]
    fn selects_a_named_navigation_mode() {
        let config: Config = serde_json::from_str(
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

        assert_eq!(
            config.active_mode().unwrap(),
            NavigationMode {
                left: NiriNavigation::ColumnOrMonitorLeft,
                down: NiriNavigation::WindowOrWorkspaceDown,
                up: NiriNavigation::WindowOrWorkspaceUp,
                right: NiriNavigation::ColumnOrMonitorRight,
            }
        );
    }

    #[test]
    fn rejects_an_undefined_active_mode() {
        let config: Config = serde_json::from_str(
            r#"{
                "active_mode": "missing",
                "modes": {}
            }"#,
        )
        .unwrap();

        assert!(config.active_mode().is_err());
    }
}
