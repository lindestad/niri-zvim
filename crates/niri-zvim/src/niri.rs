use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::Duration,
};

use niri_ipc::{
    Action, Event, Request, Response, Window, WindowLayout,
    socket::Socket,
    state::{EventStreamState, EventStreamStatePart},
};
use niri_zvim_core::{Direction, NeighborMap, NiriWindow, Rect, directional_neighbors};
use tokio::sync::mpsc::Sender;
use tracing::{debug, warn};

use crate::{
    config::{NavigationMode, NiriNavigation},
    daemon::DaemonEvent,
};

const SNAPSHOT_QUIET_PERIOD: Duration = Duration::from_millis(2);

type ColumnKey = (Option<u64>, usize);
type ColumnMembers = Vec<(usize, u64)>;
type WorkspaceColumns = BTreeMap<usize, ColumnMembers>;

#[derive(Clone, Default)]
pub(crate) struct NiriState {
    active_columns: Arc<Mutex<HashMap<ColumnKey, u64>>>,
}

pub struct NiriExecutor {
    actions: mpsc::Sender<NiriCommand>,
}

#[derive(Clone, Copy)]
enum NiriCommand {
    Navigate { sequence: u64, direction: Direction },
    Configure(NavigationMode),
}

impl NiriExecutor {
    pub fn start(mode: NavigationMode, events: Sender<DaemonEvent>, state: NiriState) -> Self {
        let (actions, receiver) = mpsc::channel();
        let (snapshots, snapshot_receiver) = mpsc::channel();
        thread::Builder::new()
            .name("niri-zvim-actions".into())
            .spawn(move || action_loop(receiver, mode, snapshots))
            .expect("failed to start niri action thread");
        thread::Builder::new()
            .name("niri-zvim-snapshots".into())
            .spawn(move || snapshot_loop(snapshot_receiver, events, state))
            .expect("failed to start niri snapshot thread");
        Self { actions }
    }

    pub fn navigate(&self, sequence: u64, direction: Direction) {
        if self
            .actions
            .send(NiriCommand::Navigate {
                sequence,
                direction,
            })
            .is_err()
        {
            warn!("niri action worker stopped");
        }
    }

    pub fn configure(&self, mode: NavigationMode) {
        if self.actions.send(NiriCommand::Configure(mode)).is_err() {
            warn!("niri action worker stopped");
        }
    }
}

pub fn start_event_thread(events: Sender<DaemonEvent>) -> NiriState {
    let state = NiriState::default();
    let thread_state = state.clone();
    thread::Builder::new()
        .name("niri-zvim-events".into())
        .spawn(move || event_loop(events, thread_state))
        .expect("failed to start niri event thread");
    state
}

fn event_loop(events: Sender<DaemonEvent>, state: NiriState) {
    loop {
        if let Err(error) = event_stream(&events, &state) {
            warn!(%error, "niri event stream disconnected");
            thread::sleep(Duration::from_millis(250));
        }
    }
}

fn event_stream(events: &Sender<DaemonEvent>, column_state: &NiriState) -> anyhow::Result<()> {
    let mut socket = Socket::connect()?;
    let reply = socket.send(Request::EventStream)?;
    anyhow::ensure!(
        matches!(reply, Ok(Response::Handled)),
        "niri rejected event stream: {reply:?}"
    );

    let mut state = EventStreamState::default();
    let mut next_event = socket.read_events();
    loop {
        let event = next_event()?;
        let is_window_event = matches!(
            event,
            Event::WindowsChanged { .. }
                | Event::WindowOpenedOrChanged { .. }
                | Event::WindowClosed { .. }
                | Event::WindowFocusChanged { .. }
        );
        state.apply(event);
        if is_window_event {
            let (windows, focused) = convert_windows(state.windows.windows.values(), column_state);
            events.blocking_send(DaemonEvent::NiriSnapshot {
                windows,
                focused,
                acknowledged_sequence: None,
            })?;
        }
    }
}

fn action_loop(
    receiver: mpsc::Receiver<NiriCommand>,
    mut mode: NavigationMode,
    snapshots: mpsc::Sender<u64>,
) {
    let mut socket = None;
    while let Ok(command) = receiver.recv() {
        let (sequence, direction) = match command {
            NiriCommand::Navigate {
                sequence,
                direction,
            } => (sequence, direction),
            NiriCommand::Configure(configured) => {
                mode = configured;
                continue;
            }
        };
        let request = Request::Action(niri_action(mode.get(direction)));

        if socket.is_none() {
            match Socket::connect() {
                Ok(connected) => socket = Some(connected),
                Err(error) => {
                    warn!(%error, "niri action socket unavailable");
                    continue;
                }
            }
        }
        let mut result = socket
            .as_mut()
            .expect("socket was initialized")
            .send(request.clone());
        if let Err(error) = result {
            debug!(%error, "reconnecting niri action socket");
            socket = Socket::connect().ok();
            result = socket
                .as_mut()
                .map_or_else(|| Err(error), |connection| connection.send(request));
            if let Err(error) = &result {
                debug!(%error, "retrying niri action failed");
                socket = None;
            }
        }
        if !matches!(result, Ok(Ok(Response::Handled))) {
            continue;
        }
        if snapshots.send(sequence).is_err() {
            break;
        }
    }
}

fn snapshot_loop(receiver: mpsc::Receiver<u64>, events: Sender<DaemonEvent>, state: NiriState) {
    let mut socket = None;
    while let Ok(first_sequence) = receiver.recv() {
        let sequence = coalesce_snapshot_sequence(first_sequence, &receiver);
        let Some(windows) = request_windows(&mut socket) else {
            warn!(sequence, "could not snapshot Niri after navigation");
            continue;
        };
        let (windows, focused) = convert_windows(windows.iter(), &state);
        if events
            .blocking_send(DaemonEvent::NiriSnapshot {
                windows,
                focused,
                acknowledged_sequence: Some(sequence),
            })
            .is_err()
        {
            break;
        }
    }
}

fn coalesce_snapshot_sequence(first: u64, receiver: &mpsc::Receiver<u64>) -> u64 {
    let mut latest = first;
    while let Ok(sequence) = receiver.recv_timeout(SNAPSHOT_QUIET_PERIOD) {
        latest = sequence;
    }
    latest
}

fn request_windows(socket: &mut Option<Socket>) -> Option<Vec<Window>> {
    if socket.is_none() {
        *socket = Socket::connect().ok();
    }
    let mut response = socket.as_mut()?.send(Request::Windows);
    if response.is_err() {
        *socket = Socket::connect().ok();
        response = socket.as_mut()?.send(Request::Windows);
    }
    match response.ok()?.ok()? {
        Response::Windows(windows) => Some(windows),
        _ => None,
    }
}

fn niri_action(navigation: NiriNavigation) -> Action {
    match navigation {
        NiriNavigation::ColumnLeft => Action::FocusColumnLeft {},
        NiriNavigation::ColumnRight => Action::FocusColumnRight {},
        NiriNavigation::ColumnOrMonitorLeft => Action::FocusColumnOrMonitorLeft {},
        NiriNavigation::ColumnOrMonitorRight => Action::FocusColumnOrMonitorRight {},
        NiriNavigation::WindowDown => Action::FocusWindowDown {},
        NiriNavigation::WindowUp => Action::FocusWindowUp {},
        NiriNavigation::WindowOrWorkspaceDown => Action::FocusWindowOrWorkspaceDown {},
        NiriNavigation::WindowOrWorkspaceUp => Action::FocusWindowOrWorkspaceUp {},
    }
}

fn convert_windows<'a>(
    windows: impl Iterator<Item = &'a Window>,
    state: &NiriState,
) -> (Vec<NiriWindow>, Option<u64>) {
    let windows: Vec<_> = windows.collect();
    let focused = windows
        .iter()
        .find(|window| window.is_focused)
        .map(|window| window.id);
    let mut by_workspace: HashMap<Option<u64>, Vec<(u64, Rect)>> = HashMap::new();

    for window in &windows {
        let Some(rect) = layout_rect(&window.layout) else {
            continue;
        };
        by_workspace
            .entry(window.workspace_id)
            .or_default()
            .push((window.id, rect));
    }

    let mut neighbors: HashMap<u64, NeighborMap<u64>> = HashMap::new();
    for rectangles in by_workspace.into_values() {
        neighbors.extend(directional_neighbors(rectangles));
    }

    apply_scrolling_layout_neighbors(&windows, &mut neighbors, state);

    let converted = windows
        .into_iter()
        .map(|window| NiriWindow {
            id: window.id,
            app_id: window.app_id.clone(),
            title: window.title.clone(),
            neighbors: neighbors.remove(&window.id).unwrap_or_default(),
        })
        .collect();
    (converted, focused)
}

fn apply_scrolling_layout_neighbors(
    windows: &[&Window],
    neighbors: &mut HashMap<u64, NeighborMap<u64>>,
    state: &NiriState,
) {
    let mut workspaces: HashMap<Option<u64>, WorkspaceColumns> = HashMap::new();
    for window in windows {
        let Some((column, tile)) = window.layout.pos_in_scrolling_layout else {
            continue;
        };
        workspaces
            .entry(window.workspace_id)
            .or_default()
            .entry(column)
            .or_default()
            .push((tile, window.id));
    }
    for columns in workspaces.values_mut() {
        for members in columns.values_mut() {
            members.sort_unstable();
        }
    }

    let mut active_columns = state
        .active_columns
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    active_columns.retain(|(workspace, column), id| {
        workspaces
            .get(workspace)
            .and_then(|columns| columns.get(column))
            .is_some_and(|members| members.iter().any(|(_, member)| member == id))
    });

    if let Some(focused) = windows.iter().find(|window| window.is_focused)
        && let Some((column, _)) = focused.layout.pos_in_scrolling_layout
    {
        active_columns.insert((focused.workspace_id, column), focused.id);
    }

    for (workspace, columns) in &workspaces {
        for (column, members) in columns {
            active_columns
                .entry((*workspace, *column))
                .or_insert_with(|| most_recent_window(windows, members));
        }
    }

    for (workspace, columns) in &workspaces {
        for (column, members) in columns {
            let left = column
                .checked_sub(1)
                .and_then(|column| active_columns.get(&(*workspace, column)).copied());
            let right = active_columns
                .get(&(*workspace, column.saturating_add(1)))
                .copied();
            for (index, (_, id)) in members.iter().enumerate() {
                neighbors.insert(
                    *id,
                    NeighborMap {
                        left,
                        down: members.get(index + 1).map(|(_, id)| *id),
                        up: index.checked_sub(1).map(|index| members[index].1),
                        right,
                    },
                );
            }
        }
    }
}

fn most_recent_window(windows: &[&Window], members: &[(usize, u64)]) -> u64 {
    members
        .iter()
        .max_by_key(|(_, id)| {
            windows
                .iter()
                .find(|window| window.id == *id)
                .and_then(|window| window.focus_timestamp)
                .map(|timestamp| (timestamp.secs, timestamp.nanos))
        })
        .map(|(_, id)| *id)
        .expect("columns contain at least one window")
}

fn layout_rect(layout: &WindowLayout) -> Option<Rect> {
    if let Some((x, y)) = layout.tile_pos_in_workspace_view {
        let (width, height) = layout.tile_size;
        return Some(Rect {
            x,
            y,
            width,
            height,
        });
    }
    layout.pos_in_scrolling_layout.map(|(column, tile)| Rect {
        x: column as f64,
        y: tile as f64,
        width: 1.0,
        height: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_sequences_coalesce_to_the_latest_command() {
        let (sender, receiver) = mpsc::channel();
        sender.send(5).unwrap();
        sender.send(8).unwrap();
        drop(sender);

        let first = receiver.recv().unwrap();
        assert_eq!(coalesce_snapshot_sequence(first, &receiver), 8);
    }

    #[test]
    fn direction_maps_to_expected_niri_action() {
        let action = niri_action(NavigationMode::workspace_local().get(Direction::Right));
        assert!(matches!(action, Action::FocusColumnRight {}));
    }

    #[test]
    fn desktop_mode_maps_to_monitor_and_workspace_actions() {
        let mode = NavigationMode {
            left: NiriNavigation::ColumnOrMonitorLeft,
            down: NiriNavigation::WindowOrWorkspaceDown,
            up: NiriNavigation::WindowOrWorkspaceUp,
            right: NiriNavigation::ColumnOrMonitorRight,
        };

        assert!(matches!(
            niri_action(mode.get(Direction::Left)),
            Action::FocusColumnOrMonitorLeft {}
        ));
        assert!(matches!(
            niri_action(mode.get(Direction::Up)),
            Action::FocusWindowOrWorkspaceUp {}
        ));
    }

    #[test]
    fn executor_queues_runtime_configuration_changes() {
        let (actions, receiver) = mpsc::channel();
        let executor = NiriExecutor { actions };
        let mode = NavigationMode {
            left: NiriNavigation::ColumnOrMonitorLeft,
            down: NiriNavigation::WindowOrWorkspaceDown,
            up: NiriNavigation::WindowOrWorkspaceUp,
            right: NiriNavigation::ColumnOrMonitorRight,
        };

        executor.configure(mode);

        assert!(matches!(
            receiver.recv().unwrap(),
            NiriCommand::Configure(configured) if configured == mode
        ));
    }

    #[test]
    fn scrolling_layout_position_is_used_when_pixel_position_is_absent() {
        let layout = WindowLayout {
            pos_in_scrolling_layout: Some((3, 2)),
            tile_size: (900.0, 700.0),
            window_size: (898, 698),
            tile_pos_in_workspace_view: None,
            window_offset_in_tile: (1.0, 1.0),
        };

        let rect = layout_rect(&layout).unwrap();
        assert_eq!((rect.x, rect.y), (3.0, 2.0));
        assert_eq!((rect.width, rect.height), (1.0, 1.0));
    }

    #[test]
    fn horizontal_neighbors_follow_the_active_window_in_a_tabbed_column() {
        let state = NiriState::default();
        let mut windows = [
            window(1, 1, 1, false),
            window(2, 2, 1, false),
            window(3, 2, 2, true),
            window(4, 3, 1, false),
        ];

        convert_windows(windows.iter(), &state);
        windows[2].is_focused = false;
        windows[0].is_focused = true;
        let (converted, focused) = convert_windows(windows.iter(), &state);
        let converted: HashMap<_, _> = converted
            .into_iter()
            .map(|window| (window.id, window))
            .collect();

        assert_eq!(focused, Some(1));
        assert_eq!(converted[&1].neighbors.right, Some(3));
        assert_eq!(converted[&2].neighbors.down, Some(3));
        assert_eq!(converted[&3].neighbors.up, Some(2));
        assert_eq!(converted[&3].neighbors.right, Some(4));
    }

    fn window(id: u64, column: usize, tile: usize, is_focused: bool) -> Window {
        Window {
            id,
            title: None,
            app_id: None,
            pid: None,
            workspace_id: Some(1),
            is_focused,
            is_floating: false,
            is_urgent: false,
            layout: WindowLayout {
                pos_in_scrolling_layout: Some((column, tile)),
                tile_size: (100.0, 100.0),
                window_size: (100, 100),
                tile_pos_in_workspace_view: None,
                window_offset_in_tile: (0.0, 0.0),
            },
            focus_timestamp: None,
        }
    }
}
