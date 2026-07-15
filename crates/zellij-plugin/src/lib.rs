use std::collections::BTreeMap;

use niri_zvim_core::{
    AdapterMessage, DaemonMessage, NeighborMap, Rect, ZellijClient, ZellijClientState,
    directional_neighbors,
};
use zellij_tile::prelude::{
    Direction as ZellijDirection, Event, EventType, PaneId as ZellijPaneId, PaneManifest,
    PermissionStatus, PermissionType, PipeMessage, PipeSource, ZellijPlugin, block_cli_pipe_input,
    cli_pipe_output, get_focused_pane_info, get_plugin_ids, hide_self, move_focus, register_plugin,
    report_panic, request_permission, set_selectable, subscribe, unblock_cli_pipe_input,
};

#[derive(Default)]
struct Plugin {
    client_id: u16,
    session: Option<String>,
    niri_window_id: Option<u64>,
    revision: u64,
    panes: PaneManifest,
    pipe_id: Option<String>,
    input: String,
}

register_plugin!(Plugin);

impl ZellijPlugin for Plugin {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        self.client_id = get_plugin_ids().client_id;
        set_selectable(false);
        subscribe(&[EventType::PermissionRequestResult]);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
        ]);
        hide_self();
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                subscribe(&[
                    EventType::ModeUpdate,
                    EventType::PaneUpdate,
                    EventType::BeforeClose,
                ]);
            }
            Event::ModeUpdate(mode) => {
                self.session = mode.session_name;
                self.publish();
            }
            Event::PaneUpdate(panes) => {
                self.panes = panes;
                self.revision = self.revision.wrapping_add(1);
                self.publish();
            }
            Event::BeforeClose => {
                self.pipe_id = None;
            }
            _ => {}
        }
        false
    }

    fn pipe(&mut self, message: PipeMessage) -> bool {
        let PipeSource::Cli(pipe_id) = message.source else {
            return false;
        };
        self.pipe_id = Some(pipe_id.clone());
        block_cli_pipe_input(&pipe_id);

        if let Some(payload) = message.payload {
            self.input.push_str(&payload);
            if !self.input.ends_with('\n') {
                self.input.push('\n');
            }
            while let Some(end) = self.input.find('\n') {
                let line: String = self.input.drain(..=end).collect();
                self.handle_daemon_message(line.trim());
            }
        }

        unblock_cli_pipe_input(&pipe_id);
        false
    }
}

impl Plugin {
    fn handle_daemon_message(&mut self, line: &str) {
        let Ok(message) = serde_json::from_str(line) else {
            return;
        };
        match message {
            DaemonMessage::BindNiriWindow { window_id } => {
                self.niri_window_id = Some(window_id);
                self.publish();
            }
            DaemonMessage::Navigate { direction, .. } => {
                move_focus(match direction {
                    niri_zvim_core::Direction::Left => ZellijDirection::Left,
                    niri_zvim_core::Direction::Down => ZellijDirection::Down,
                    niri_zvim_core::Direction::Up => ZellijDirection::Up,
                    niri_zvim_core::Direction::Right => ZellijDirection::Right,
                });
                self.revision = self.revision.wrapping_add(1);
                self.publish();
            }
        }
    }

    fn publish(&self) {
        let (Some(pipe_id), Some(session), Some(niri_window_id)) = (
            self.pipe_id.as_ref(),
            self.session.as_ref(),
            self.niri_window_id,
        ) else {
            return;
        };
        let Ok((tab, focused)) = get_focused_pane_info() else {
            return;
        };
        let ZellijPaneId::Terminal(focused_pane) = focused else {
            return;
        };
        let pane_neighbors = pane_neighbors(&self.panes, tab, focused_pane);
        let message = AdapterMessage::ZellijSnapshot {
            state: ZellijClientState {
                client: ZellijClient {
                    session: session.clone(),
                    client_id: self.client_id,
                },
                niri_window_id,
                revision: self.revision,
                focused_pane,
                pane_neighbors,
            },
        };
        if let Ok(mut encoded) = serde_json::to_string(&message) {
            encoded.push('\n');
            cli_pipe_output(pipe_id, &encoded);
        }
    }
}

fn pane_neighbors(
    manifest: &PaneManifest,
    tab: usize,
    focused_pane: u32,
) -> BTreeMap<String, NeighborMap<u32>> {
    let Some(panes) = manifest.panes.get(&tab) else {
        return BTreeMap::new();
    };
    let focused_is_floating = panes
        .iter()
        .find(|pane| !pane.is_plugin && pane.id == focused_pane)
        .is_some_and(|pane| pane.is_floating);
    let rectangles: Vec<_> = panes
        .iter()
        .filter(|pane| {
            !pane.is_plugin
                && pane.is_selectable
                && !pane.is_suppressed
                && pane.is_floating == focused_is_floating
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
    let neighbors = directional_neighbors(rectangles);
    neighbors
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
        .collect()
}
