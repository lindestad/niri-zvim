use std::collections::BTreeMap;

use niri_zvim_core::{
    AdapterMessage, DaemonMessage, NeighborMap, Rect, ZellijClient, ZellijClientState,
    directional_neighbors,
};
use zellij_tile::prelude::{
    Direction as ZellijDirection, Event, EventType, PaneManifest, PermissionStatus, PermissionType,
    PipeMessage, PipeSource, ZellijPlugin, block_cli_pipe_input, cli_pipe_output,
    get_focused_pane_info, get_plugin_ids, get_session_environment_variables, get_session_list,
    hide_self, move_focus, register_plugin, report_panic, request_permission, set_selectable,
    set_timeout, subscribe, unblock_cli_pipe_input,
};

const PUBLISH_RETRY_INTERVAL_SECONDS: f64 = 0.05;
const MAX_PUBLISH_RETRIES: u8 = 20;

#[derive(Default)]
struct Plugin {
    client_id: u16,
    session: Option<String>,
    niri_window_id: Option<u64>,
    revision: u64,
    pending_sequence: Option<u64>,
    acknowledged_sequence: Option<u64>,
    pipe_id: Option<String>,
    input: String,
    publish_retries: u8,
    publish_retry_pending: bool,
    active_tab: Option<usize>,
    pane_manifest: Option<PaneManifest>,
}

register_plugin!(Plugin);

impl ZellijPlugin for Plugin {
    fn load(&mut self, _configuration: BTreeMap<String, String>) {
        self.client_id = get_plugin_ids().client_id;
        subscribe(&[
            EventType::PermissionRequestResult,
            EventType::Timer,
            EventType::ModeUpdate,
            EventType::TabUpdate,
            EventType::PaneUpdate,
            EventType::BeforeClose,
        ]);
        request_permission(&[
            PermissionType::ReadApplicationState,
            PermissionType::ChangeApplicationState,
            PermissionType::ReadCliPipes,
            PermissionType::ReadSessionEnvironmentVariables,
        ]);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::PermissionRequestResult(PermissionStatus::Granted) => {
                self.session = get_session_environment_variables().remove("ZELLIJ_SESSION_NAME");
                set_selectable(false);
                hide_self();
                self.state_changed();
            }
            Event::ModeUpdate(mode) => {
                self.session = mode.session_name;
                self.state_changed();
            }
            Event::TabUpdate(tabs) => {
                self.active_tab = tabs.iter().find(|tab| tab.active).map(|tab| tab.position);
                self.revision = self.revision.wrapping_add(1);
                self.state_changed();
            }
            Event::PaneUpdate(manifest) => {
                self.pane_manifest = Some(manifest);
                if let Some(sequence) = self.pending_sequence.take() {
                    self.acknowledged_sequence = Some(sequence);
                }
                self.revision = self.revision.wrapping_add(1);
                self.state_changed();
            }
            Event::Timer(_) if self.publish_retry_pending => {
                self.publish_retry_pending = false;
                self.publish_or_retry();
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
            while let Some(end) = self.input.find('\n') {
                let line: String = self.input.drain(..=end).collect();
                self.handle_daemon_message(line.trim());
            }
            if serde_json::from_str::<DaemonMessage>(self.input.trim()).is_ok() {
                let line = std::mem::take(&mut self.input);
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
            DaemonMessage::BindNiriWindow { window_id, session } => {
                self.niri_window_id = Some(window_id);
                self.session = Some(session);
                self.defer_publish();
            }
            DaemonMessage::Navigate {
                sequence,
                direction,
            } => {
                self.pending_sequence = Some(sequence);
                move_focus(match direction {
                    niri_zvim_core::Direction::Left => ZellijDirection::Left,
                    niri_zvim_core::Direction::Down => ZellijDirection::Down,
                    niri_zvim_core::Direction::Up => ZellijDirection::Up,
                    niri_zvim_core::Direction::Right => ZellijDirection::Right,
                });
            }
        }
    }

    fn state_changed(&mut self) {
        self.publish_retries = 0;
        self.publish_or_retry();
    }

    fn defer_publish(&mut self) {
        self.publish_retries = 0;
        if !self.publish_retry_pending {
            self.publish_retry_pending = true;
            set_timeout(0.01);
        }
    }

    fn publish_or_retry(&mut self) {
        if self.publish() {
            self.publish_retries = 0;
            self.publish_retry_pending = false;
        } else if !self.publish_retry_pending && self.publish_retries < MAX_PUBLISH_RETRIES {
            self.publish_retries += 1;
            self.publish_retry_pending = true;
            set_timeout(PUBLISH_RETRY_INTERVAL_SECONDS);
        }
    }

    fn publish(&self) -> bool {
        let (Some(pipe_id), Some(session), Some(niri_window_id)) = (
            self.pipe_id.as_ref(),
            self.session.as_ref(),
            self.niri_window_id,
        ) else {
            return false;
        };
        let cached_focus =
            self.active_tab
                .zip(self.pane_manifest.as_ref())
                .and_then(|(tab, manifest)| {
                    manifest.panes.get(&tab).and_then(|panes| {
                        panes
                            .iter()
                            .find(|pane| !pane.is_plugin && pane.is_focused)
                            .map(|pane| (pane.id, pane_neighbors(manifest, tab, pane.id)))
                    })
                });
        let (focused_pane, pane_neighbors) = if let Some(cached_focus) = cached_focus {
            cached_focus
        } else {
            let Ok((tab, focused)) = get_focused_pane_info() else {
                return false;
            };
            let zellij_tile::prelude::PaneId::Terminal(focused_pane) = focused else {
                return false;
            };
            let Ok(sessions) = get_session_list() else {
                return false;
            };
            let Some(session_info) = sessions
                .live_sessions
                .iter()
                .find(|info| info.name == *session)
            else {
                return false;
            };
            (
                focused_pane,
                pane_neighbors(&session_info.panes, tab, focused_pane),
            )
        };
        let message = AdapterMessage::ZellijSnapshot {
            state: ZellijClientState {
                client: ZellijClient {
                    session: session.clone(),
                    client_id: self.client_id,
                },
                niri_window_id,
                revision: self.revision,
                acknowledged_sequence: self.acknowledged_sequence,
                focused_pane,
                pane_neighbors,
            },
        };
        if let Ok(mut encoded) = serde_json::to_string(&message) {
            encoded.push('\n');
            cli_pipe_output(pipe_id, &encoded);
            true
        } else {
            false
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
