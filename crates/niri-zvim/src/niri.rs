use std::{collections::HashMap, sync::mpsc, thread, time::Duration};

use niri_ipc::{
    Action, Event, Request, Response, Window,
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

pub struct NiriExecutor {
    actions: mpsc::Sender<Direction>,
}

impl NiriExecutor {
    pub fn start(mode: NavigationMode) -> Self {
        let (actions, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("niri-zvim-actions".into())
            .spawn(move || action_loop(receiver, mode))
            .expect("failed to start niri action thread");
        Self { actions }
    }

    pub fn navigate(&self, direction: Direction) {
        if self.actions.send(direction).is_err() {
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
        let acknowledges_focus = matches!(event, Event::WindowFocusChanged { .. });
        state.apply(event);
        if is_window_event {
            let (windows, focused) = convert_windows(state.windows.windows.values());
            events.blocking_send(DaemonEvent::NiriSnapshot {
                windows,
                focused,
                acknowledges_focus,
            })?;
        }
    }
}

fn action_loop(receiver: mpsc::Receiver<Direction>, mode: NavigationMode) {
    let mut socket = None;
    while let Ok(direction) = receiver.recv() {
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
        let result = socket
            .as_mut()
            .expect("socket was initialized")
            .send(request.clone());
        if let Err(error) = result {
            debug!(%error, "reconnecting niri action socket");
            socket = Socket::connect().ok();
            let retry = socket.as_mut().map(|connection| connection.send(request));
            if let Some(Err(error)) = retry {
                debug!(%error, "retrying niri action failed");
                socket = None;
            }
        }
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
        let Some((x, y)) = window.layout.tile_pos_in_workspace_view else {
            continue;
        };
        let (width, height) = window.layout.tile_size;
        by_workspace.entry(window.workspace_id).or_default().push((
            window.id,
            Rect {
                x,
                y,
                width,
                height,
            },
        ));
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
