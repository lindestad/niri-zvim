use std::{collections::HashMap, sync::mpsc, thread, time::Duration};

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

pub struct NiriExecutor {
    actions: mpsc::Sender<NiriCommand>,
}

#[derive(Clone, Copy)]
struct NiriCommand {
    sequence: u64,
    direction: Direction,
}

impl NiriExecutor {
    pub fn start(mode: NavigationMode, events: Sender<DaemonEvent>) -> Self {
        let (actions, receiver) = mpsc::channel();
        let (snapshots, snapshot_receiver) = mpsc::channel();
        thread::Builder::new()
            .name("niri-zvim-actions".into())
            .spawn(move || action_loop(receiver, mode, snapshots))
            .expect("failed to start niri action thread");
        thread::Builder::new()
            .name("niri-zvim-snapshots".into())
            .spawn(move || snapshot_loop(snapshot_receiver, events))
            .expect("failed to start niri snapshot thread");
        Self { actions }
    }

    pub fn navigate(&self, sequence: u64, direction: Direction) {
        if self
            .actions
            .send(NiriCommand {
                sequence,
                direction,
            })
            .is_err()
        {
            warn!("niri action worker stopped");
        }
    }
}

pub fn start_event_thread(events: Sender<DaemonEvent>) {
    thread::Builder::new()
        .name("niri-zvim-events".into())
        .spawn(move || event_loop(events))
        .expect("failed to start niri event thread");
}

fn event_loop(events: Sender<DaemonEvent>) {
    loop {
        if let Err(error) = event_stream(&events) {
            warn!(%error, "niri event stream disconnected");
            thread::sleep(Duration::from_millis(250));
        }
    }
}

fn event_stream(events: &Sender<DaemonEvent>) -> anyhow::Result<()> {
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
            let (windows, focused) = convert_windows(state.windows.windows.values());
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
    mode: NavigationMode,
    snapshots: mpsc::Sender<u64>,
) {
    let mut socket = None;
    while let Ok(NiriCommand {
        sequence,
        direction,
    }) = receiver.recv()
    {
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

fn snapshot_loop(receiver: mpsc::Receiver<u64>, events: Sender<DaemonEvent>) {
    let mut socket = None;
    while let Ok(first_sequence) = receiver.recv() {
        let sequence = coalesce_snapshot_sequence(first_sequence, &receiver);
        let Some(windows) = request_windows(&mut socket) else {
            warn!(sequence, "could not snapshot Niri after navigation");
            continue;
        };
        let (windows, focused) = convert_windows(windows.iter());
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
}
