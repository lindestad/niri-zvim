use anyhow::Context;
use niri_zvim_core::{NeighborMap, Rect, ZellijClient, ZellijClientState, directional_neighbors};
use serde::Deserialize;
use tokio::process::Command;

#[derive(Debug, Deserialize)]
struct ListedPane {
    id: u32,
    is_plugin: bool,
    is_floating: bool,
    is_suppressed: bool,
    is_selectable: bool,
    pane_x: usize,
    pane_y: usize,
    pane_rows: usize,
    pane_columns: usize,
    tab_position: usize,
}

pub(super) async fn query_snapshot(
    session: &str,
    window_id: u64,
    client_id: u16,
) -> anyhow::Result<ZellijClientState> {
    let panes = Command::new("zellij")
        .args([
            "--session",
            session,
            "action",
            "list-panes",
            "--all",
            "--json",
        ])
        .output()
        .await
        .with_context(|| format!("could not list panes in Zellij session {session}"))?;
    anyhow::ensure!(
        panes.status.success(),
        "could not list panes in Zellij session {session}"
    );
    let clients = Command::new("zellij")
        .args(["--session", session, "action", "list-clients"])
        .output()
        .await
        .with_context(|| format!("could not list clients in Zellij session {session}"))?;
    anyhow::ensure!(
        clients.status.success(),
        "could not list clients in Zellij session {session}"
    );
    let focused_pane = focused_pane_from_clients(&clients.stdout)
        .context("Zellij does not have exactly one connected terminal client")?;
    let panes: Vec<ListedPane> = serde_json::from_slice(&panes.stdout)?;
    snapshot_from_panes(session, window_id, client_id, focused_pane, &panes)
        .context("Zellij has no focused terminal pane")
}

fn focused_pane_from_clients(output: &[u8]) -> Option<u32> {
    let output = std::str::from_utf8(output).ok()?;
    let mut panes = output.lines().skip(1).filter_map(|line| {
        line.split_whitespace()
            .nth(1)?
            .strip_prefix("terminal_")?
            .parse::<u32>()
            .ok()
    });
    let focused = panes.next()?;
    panes.next().is_none().then_some(focused)
}

fn snapshot_from_panes(
    session: &str,
    window_id: u64,
    client_id: u16,
    focused_pane: u32,
    panes: &[ListedPane],
) -> Option<ZellijClientState> {
    let focused = panes
        .iter()
        .find(|pane| !pane.is_plugin && pane.id == focused_pane)?;
    let rectangles: Vec<_> = panes
        .iter()
        .filter(|pane| {
            !pane.is_plugin
                && pane.is_selectable
                && !pane.is_suppressed
                && pane.tab_position == focused.tab_position
                && pane.is_floating == focused.is_floating
        })
        .map(|pane| {
            (
                u64::from(pane.id),
                Rect {
                    x: pane.pane_x as f64,
                    y: pane.pane_y as f64,
                    width: pane.pane_columns as f64,
                    height: pane.pane_rows as f64,
                },
            )
        })
        .collect();
    let pane_neighbors = directional_neighbors(rectangles)
        .into_iter()
        .map(|(id, neighbors)| {
            (
                (id as u32).to_string(),
                NeighborMap {
                    left: neighbors.left.map(|id| id as u32),
                    down: neighbors.down.map(|id| id as u32),
                    up: neighbors.up.map(|id| id as u32),
                    right: neighbors.right.map(|id| id as u32),
                },
            )
        })
        .collect();
    Some(ZellijClientState {
        client: ZellijClient {
            session: session.to_owned(),
            client_id,
        },
        niri_window_id: window_id,
        revision: 0,
        acknowledged_sequence: None,
        focused_pane,
        pane_neighbors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_initial_snapshot_from_listed_panes() {
        let pane = |id, x| ListedPane {
            id,
            is_plugin: false,
            is_floating: false,
            is_suppressed: false,
            is_selectable: true,
            pane_x: x,
            pane_y: 0,
            pane_rows: 20,
            pane_columns: 40,
            tab_position: 0,
        };
        let state = snapshot_from_panes("dev", 42, 0, 1, &[pane(1, 0), pane(2, 40)]).unwrap();

        assert_eq!(state.focused_pane, 1);
        assert_eq!(state.pane_neighbors["1"].right, Some(2));
        assert_eq!(state.pane_neighbors["2"].left, Some(1));
    }

    #[test]
    fn extracts_the_only_connected_terminal_client_focus() {
        let one = b"CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND\n1 terminal_37 N/A\n";
        let two =
            b"CLIENT_ID ZELLIJ_PANE_ID RUNNING_COMMAND\n1 terminal_37 N/A\n2 terminal_9 N/A\n";

        assert_eq!(focused_pane_from_clients(one), Some(37));
        assert_eq!(focused_pane_from_clients(two), None);
    }
}
