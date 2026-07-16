use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Component, Path, PathBuf},
};

use tokio::{
    sync::mpsc,
    time::{Duration, sleep},
};

use super::bridge::RefreshEvent;

pub(super) async fn watch_session_metadata(
    session: String,
    refreshes: mpsc::UnboundedSender<RefreshEvent>,
) {
    let metadata = session_metadata_path(&session);
    let mut last_topology = metadata_topology_stamp(&metadata);
    loop {
        sleep(Duration::from_millis(50)).await;
        let topology = metadata_topology_stamp(&metadata);
        if topology == last_topology {
            continue;
        }
        sleep(Duration::from_millis(20)).await;
        last_topology = metadata_topology_stamp(&metadata);
        if refreshes.send(RefreshEvent::MetadataChanged).is_err() {
            break;
        }
    }
}

fn session_metadata_path(session: &str) -> PathBuf {
    let cache = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("."));
    cache
        .join("zellij/contract_version_1/session_info")
        .join(session)
        .join("session-metadata.kdl")
}

fn metadata_topology_stamp(path: &Path) -> Option<u64> {
    let contents = fs::read_to_string(path).ok()?;
    Some(metadata_topology_fingerprint(&contents))
}

fn metadata_topology_fingerprint(contents: &str) -> u64 {
    const STATE_FIELDS: &[&str] = &[
        "position ",
        "active ",
        "are_floating_panes_visible ",
        "id ",
        "is_plugin ",
        "is_focused ",
        "is_floating ",
        "is_suppressed ",
        "pane_x ",
        "pane_y ",
        "pane_rows ",
        "pane_columns ",
        "is_selectable ",
        "tab_position ",
    ];
    let mut hasher = DefaultHasher::new();
    for line in contents.lines().map(str::trim) {
        if STATE_FIELDS.iter().any(|field| line.starts_with(field)) {
            line.hash(&mut hasher);
        }
    }
    hasher.finish()
}

pub(super) fn plugin_path() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRI_ZVIM_ZELLIJ_PLUGIN") {
        return path.into();
    }
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    config.join("zellij/plugins/niri-zvim.wasm")
}

pub(super) fn session_socket_exists(session: &str) -> bool {
    let mut components = Path::new(session).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return false;
    }
    let root = std::env::var_os("ZELLIJ_SOCKET_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("XDG_RUNTIME_DIR").map(|runtime| PathBuf::from(runtime).join("zellij"))
        })
        .unwrap_or_else(|| PathBuf::from("/tmp/zellij"));
    Path::new(&root)
        .join("contract_version_1")
        .join(session)
        .exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_ghostty_titles_that_are_not_session_names() {
        for title in ["", ".", "..", "/home/dl", "project/src"] {
            assert!(!session_socket_exists(title), "accepted {title:?}");
        }
    }

    #[test]
    fn metadata_fingerprint_ignores_cursor_but_tracks_topology() {
        let first = "id 1\npane_columns 40\ncursor_coordinates_in_pane 2 3\n";
        let moved_cursor = "id 1\npane_columns 40\ncursor_coordinates_in_pane 9 8\n";
        let resized = "id 1\npane_columns 80\ncursor_coordinates_in_pane 9 8\n";

        assert_eq!(
            metadata_topology_fingerprint(first),
            metadata_topology_fingerprint(moved_cursor)
        );
        assert_ne!(
            metadata_topology_fingerprint(first),
            metadata_topology_fingerprint(resized)
        );
    }
}
