use std::collections::BTreeMap;

use niri_zvim_core::{
    AdapterMessage, DaemonMessage, NeighborMap, ProtocolMessage, Rect, ZellijClient,
    ZellijClientState, directional_neighbors,
};
use niri_zvim_zellij::assigned_window_id;
use zellij_tile::prelude::{
    Direction as ZellijDirection, Event, EventType, PaneManifest, PermissionStatus, PermissionType,
    PipeMessage, PipeSource, ZellijPlugin, block_cli_pipe_input, cli_pipe_output,
    get_focused_pane_info, get_plugin_ids, get_session_environment_variables, get_session_list,
    hide_self, list_clients, move_focus, register_plugin, report_panic, request_permission,
    set_selectable, set_timeout, subscribe, unblock_cli_pipe_input,
};

const PUBLISH_RETRY_INTERVAL_SECONDS: f64 = 0.05;
const MAX_PUBLISH_RETRIES: u8 = 20;

type TopologySignature = Vec<(
    usize,
    u32,
    bool,
    bool,
    bool,
    bool,
    usize,
    usize,
    usize,
    usize,
)>;
type PaneNeighbors = BTreeMap<String, NeighborMap<u32>>;

#[derive(Default)]
struct Plugin {
    client_id: u16,
    session: Option<String>,
    graph_session: Option<String>,
    client_ids: Vec<u16>,
    niri_window_ids: Vec<u64>,
    direct_window_id: Option<u64>,
    revision: u64,
    pending_sequence: Option<u64>,
    pending_origin: Option<u32>,
    predicted_focus: Option<u32>,
    acknowledged_sequence: Option<u64>,
    pipe_id: Option<String>,
    input: String,
    publish_retries: u8,
    publish_retry_pending: bool,
    pane_manifest: Option<PaneManifest>,
    topology_signature: TopologySignature,
    neighbor_cache: BTreeMap<(usize, bool), PaneNeighbors>,
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
            EventType::ListClients,
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
                list_clients();
                self.state_changed();
            }
            Event::ModeUpdate(mode) => {
                self.session = mode.session_name;
                self.state_changed();
            }
            Event::TabUpdate(_) => {
                self.revision = self.revision.wrapping_add(1);
                self.state_changed();
            }
            Event::PaneUpdate(manifest) => {
                let signature = topology_signature(&manifest);
                if signature != self.topology_signature {
                    self.topology_signature = signature;
                    self.neighbor_cache.clear();
                }
                self.pane_manifest = Some(manifest);
                self.revision = self.revision.wrapping_add(1);
                self.state_changed();
            }
            Event::ListClients(clients) => {
                self.client_ids = clients.into_iter().map(|client| client.client_id).collect();
                self.client_ids.sort_unstable();
                self.client_ids.dedup();
                self.publish_clients();
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
            if serde_json::from_str::<ProtocolMessage<DaemonMessage>>(self.input.trim()).is_ok() {
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
        let Ok(message) = serde_json::from_str::<ProtocolMessage<DaemonMessage>>(line) else {
            return;
        };
        let Ok(message) = message.into_current() else {
            return;
        };
        match message {
            DaemonMessage::ZellijSync {
                session,
                mut window_ids,
            } => {
                window_ids.sort_unstable();
                window_ids.dedup();
                self.niri_window_ids = window_ids;
                self.direct_window_id = None;
                self.graph_session = Some(session.clone());
                self.session = Some(session);
                list_clients();
                self.defer_publish();
            }
            DaemonMessage::ZellijSyncClient {
                session,
                graph_session,
                window_id,
            } => {
                self.niri_window_ids.clear();
                self.direct_window_id = Some(window_id);
                self.graph_session = Some(graph_session);
                self.session = Some(session);
                list_clients();
                self.defer_publish();
            }
            DaemonMessage::ZellijNavigate {
                client_id,
                sequence,
                direction,
            } if client_id == self.client_id => {
                self.pending_sequence = Some(sequence);
                let (origin, target) = self.predicted_transition(direction);
                self.pending_origin = origin;
                self.predicted_focus = target;
                move_focus(match direction {
                    niri_zvim_core::Direction::Left => ZellijDirection::Left,
                    niri_zvim_core::Direction::Down => ZellijDirection::Down,
                    niri_zvim_core::Direction::Up => ZellijDirection::Up,
                    niri_zvim_core::Direction::Right => ZellijDirection::Right,
                });
            }
            DaemonMessage::Navigate { .. } | DaemonMessage::ZellijNavigate { .. } => {}
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

    fn publish(&mut self) -> bool {
        let (Some(pipe_id), Some(session), Some(graph_session)) = (
            self.pipe_id.clone(),
            self.session.clone(),
            self.graph_session.clone(),
        ) else {
            return false;
        };
        let Some(niri_window_id) = assigned_window_id(
            self.client_id,
            self.direct_window_id,
            &self.client_ids,
            &self.niri_window_ids,
        ) else {
            return false;
        };
        let Ok((tab, focused)) = get_focused_pane_info() else {
            return false;
        };
        let zellij_tile::prelude::PaneId::Terminal(focused_pane) = focused else {
            return false;
        };
        if self
            .pending_origin
            .is_some_and(|origin| origin != focused_pane)
        {
            self.acknowledged_sequence = self.pending_sequence.take();
            self.pending_origin = None;
            self.predicted_focus = None;
        }
        let pane_neighbors = if self.pane_manifest.is_some() {
            let Some(neighbors) = self.cached_neighbors(tab, focused_pane) else {
                return false;
            };
            neighbors.clone()
        } else {
            let Ok(sessions) = get_session_list() else {
                return false;
            };
            let Some(session_info) = sessions
                .live_sessions
                .iter()
                .find(|info| info.name == session)
            else {
                return false;
            };
            pane_neighbors(&session_info.panes, tab, focused_pane)
        };
        Self::write_adapter(
            &pipe_id,
            AdapterMessage::ZellijSnapshot {
                state: ZellijClientState {
                    client: ZellijClient {
                        session: graph_session,
                        client_id: self.client_id,
                    },
                    niri_window_id,
                    revision: self.revision,
                    acknowledged_sequence: self.acknowledged_sequence,
                    focused_pane,
                    pane_neighbors,
                },
            },
        )
    }

    fn publish_clients(&self) {
        let (Some(pipe_id), Some(graph_session)) = (&self.pipe_id, &self.graph_session) else {
            return;
        };
        let client_ids = if self.direct_window_id.is_some() {
            vec![self.client_id]
        } else {
            self.client_ids.clone()
        };
        Self::write_adapter(
            pipe_id,
            AdapterMessage::ZellijClients {
                session: graph_session.clone(),
                client_ids,
            },
        );
    }

    fn write_adapter(pipe_id: &str, message: AdapterMessage) -> bool {
        if let Ok(mut encoded) = serde_json::to_string(&ProtocolMessage::new(message)) {
            encoded.push('\n');
            cli_pipe_output(pipe_id, &encoded);
            true
        } else {
            false
        }
    }

    fn predicted_transition(
        &mut self,
        direction: niri_zvim_core::Direction,
    ) -> (Option<u32>, Option<u32>) {
        let Ok((tab, observed)) = get_focused_pane_info() else {
            return (None, None);
        };
        let zellij_tile::prelude::PaneId::Terminal(observed) = observed else {
            return (None, None);
        };
        let focused = self.predicted_focus.unwrap_or(observed);
        let target = self
            .cached_neighbors(tab, focused)
            .and_then(|neighbors| neighbors.get(&focused.to_string()))
            .and_then(|neighbors| neighbors.get(direction))
            .copied();
        (Some(focused), target)
    }

    fn cached_neighbors(&mut self, tab: usize, focused_pane: u32) -> Option<&PaneNeighbors> {
        let manifest = self.pane_manifest.as_ref()?;
        let panes = manifest.panes.get(&tab)?;
        let floating = panes
            .iter()
            .find(|pane| !pane.is_plugin && pane.id == focused_pane)?
            .is_floating;
        let key = (tab, floating);
        Some(
            self.neighbor_cache
                .entry(key)
                .or_insert_with(|| pane_neighbors_for_layer(manifest, tab, floating)),
        )
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
    pane_neighbors_for_layer(manifest, tab, focused_is_floating)
}

fn pane_neighbors_for_layer(manifest: &PaneManifest, tab: usize, floating: bool) -> PaneNeighbors {
    let Some(panes) = manifest.panes.get(&tab) else {
        return BTreeMap::new();
    };
    let rectangles: Vec<_> = panes
        .iter()
        .filter(|pane| {
            !pane.is_plugin
                && pane.is_selectable
                && !pane.is_suppressed
                && pane.is_floating == floating
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

fn topology_signature(manifest: &PaneManifest) -> TopologySignature {
    let mut signature: Vec<_> = manifest
        .panes
        .iter()
        .flat_map(|(tab, panes)| {
            panes.iter().map(|pane| {
                (
                    *tab,
                    pane.id,
                    pane.is_plugin,
                    pane.is_floating,
                    pane.is_suppressed,
                    pane.is_selectable,
                    pane.pane_x,
                    pane.pane_y,
                    pane.pane_rows,
                    pane.pane_columns,
                )
            })
        })
        .collect();
    signature.sort_unstable();
    signature
}
