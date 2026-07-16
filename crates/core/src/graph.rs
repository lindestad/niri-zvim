use std::collections::BTreeMap;

use thiserror::Error;

use crate::{
    Direction, NavigationAction, NiriWindow, NvimInstance, NvimParent, ZellijClient,
    ZellijClientState,
};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteError {
    #[error("niri has not published focused-window state yet")]
    FocusUnknown,
}

#[derive(Debug, Default)]
pub struct NavigationGraph {
    windows: BTreeMap<u64, NiriWindow>,
    focused_niri_window: Option<u64>,
    zellij: BTreeMap<ZellijClient, ZellijClientState>,
    nvim: BTreeMap<String, NvimInstance>,
}

impl NavigationGraph {
    pub fn replace_niri_windows(
        &mut self,
        windows: impl IntoIterator<Item = NiriWindow>,
        focused: Option<u64>,
    ) {
        self.windows = windows
            .into_iter()
            .map(|window| (window.id, window))
            .collect();
        self.focused_niri_window = focused;
        if let Some(focused) = focused {
            for state in self.nvim.values_mut() {
                if matches!(state.parent, NvimParent::FocusedNiriWindow) && state.terminal_focused {
                    state.parent = NvimParent::NiriWindow(focused);
                }
            }
        }
    }

    pub fn set_focused_niri_window(&mut self, focused: Option<u64>) {
        self.focused_niri_window = focused;
    }

    pub fn update_zellij(&mut self, state: ZellijClientState) {
        let accept = self
            .zellij
            .get(&state.client)
            .is_none_or(|current| state.revision >= current.revision);
        if accept {
            self.zellij.insert(state.client.clone(), state);
        }
    }

    pub fn update_nvim(&mut self, mut state: NvimInstance) {
        if matches!(state.parent, NvimParent::FocusedNiriWindow) {
            state.parent = self
                .nvim
                .get(&state.id)
                .and_then(|current| match &current.parent {
                    NvimParent::FocusedNiriWindow => None,
                    parent => Some(parent.clone()),
                })
                .or_else(|| {
                    state
                        .terminal_focused
                        .then(|| self.focused_niri_window.map(NvimParent::NiriWindow))
                        .flatten()
                })
                .unwrap_or(NvimParent::FocusedNiriWindow);
        }
        let accept = self
            .nvim
            .get(&state.id)
            .is_none_or(|current| state.revision >= current.revision);
        if accept {
            self.nvim.insert(state.id.clone(), state);
        }
    }

    pub fn remove_nvim(&mut self, id: &str) {
        self.nvim.remove(id);
    }

    pub fn predict_niri_focus(&mut self, direction: Direction) {
        if let Some(current) = self.focused_niri_window {
            self.move_niri_prediction(current, direction);
        }
    }

    pub fn route_optimistically(
        &mut self,
        direction: Direction,
    ) -> Result<NavigationAction, RouteError> {
        let niri_window = self.focused_niri_window.ok_or(RouteError::FocusUnknown)?;

        if let Some((client, pane_id)) = self.focused_zellij(niri_window) {
            if let Some(id) = self.nvim_in_zellij_pane(&client, pane_id)
                && self.move_nvim_prediction(&id, direction)
            {
                return Ok(NavigationAction::Nvim { id, direction });
            }

            if self.move_zellij_prediction(&client, direction) {
                return Ok(NavigationAction::Zellij { client, direction });
            }
        } else if let Some(id) = self.direct_nvim(niri_window)
            && self.move_nvim_prediction(&id, direction)
        {
            return Ok(NavigationAction::Nvim { id, direction });
        }

        self.move_niri_prediction(niri_window, direction);
        Ok(NavigationAction::Niri { direction })
    }

    fn focused_zellij(&self, window_id: u64) -> Option<(ZellijClient, u32)> {
        self.zellij
            .values()
            .find(|state| state.niri_window_id == window_id)
            .map(|state| (state.client.clone(), state.focused_pane))
    }

    fn direct_nvim(&self, window_id: u64) -> Option<String> {
        self.nvim.values().find_map(|state| match state.parent {
            NvimParent::NiriWindow(id) if id == window_id => Some(state.id.clone()),
            _ => None,
        })
    }

    fn nvim_in_zellij_pane(&self, client: &ZellijClient, pane_id: u32) -> Option<String> {
        self.nvim.values().find_map(|state| match &state.parent {
            NvimParent::ZellijPane {
                client: parent,
                pane_id: parent_pane,
            } if parent.session == client.session
                && (parent.client_id == 0 || parent.client_id == client.client_id)
                && *parent_pane == pane_id =>
            {
                Some(state.id.clone())
            }
            _ => None,
        })
    }

    fn move_nvim_prediction(&mut self, id: &str, direction: Direction) -> bool {
        let state = self
            .nvim
            .get_mut(id)
            .expect("instance was selected from this map");
        let Some(next) = state.window_neighbors[&state.focused_window.to_string()]
            .get(direction)
            .copied()
        else {
            return false;
        };
        state.focused_window = next;
        state.revision = state.revision.saturating_add(1);
        true
    }

    fn move_zellij_prediction(&mut self, client: &ZellijClient, direction: Direction) -> bool {
        let state = self
            .zellij
            .get_mut(client)
            .expect("client was selected from this map");
        let Some(next) = state.pane_neighbors[&state.focused_pane.to_string()]
            .get(direction)
            .copied()
        else {
            return false;
        };
        state.focused_pane = next;
        state.revision = state.revision.saturating_add(1);
        true
    }

    fn move_niri_prediction(&mut self, current: u64, direction: Direction) {
        self.focused_niri_window = self
            .windows
            .get(&current)
            .and_then(|window| window.neighbors.get(direction))
            .copied();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NeighborMap;

    fn neighbors(left: Option<u64>, right: Option<u64>) -> NeighborMap<u64> {
        NeighborMap {
            left,
            right,
            ..NeighborMap::default()
        }
    }

    fn nested_graph(nvim_right: Option<u64>, zellij_right: Option<u32>) -> NavigationGraph {
        let mut graph = NavigationGraph::default();
        graph.replace_niri_windows(
            [
                NiriWindow {
                    id: 1,
                    app_id: Some("ghostty".into()),
                    title: None,
                    neighbors: neighbors(None, Some(2)),
                },
                NiriWindow {
                    id: 2,
                    app_id: Some("browser".into()),
                    title: None,
                    neighbors: neighbors(Some(1), None),
                },
            ],
            Some(1),
        );
        let client = ZellijClient {
            session: "dev".into(),
            client_id: 1,
        };
        graph.update_zellij(ZellijClientState {
            client: client.clone(),
            niri_window_id: 1,
            revision: 1,
            focused_pane: 10,
            pane_neighbors: BTreeMap::from([(
                "10".into(),
                NeighborMap {
                    right: zellij_right,
                    ..NeighborMap::default()
                },
            )]),
        });
        graph.update_nvim(NvimInstance {
            id: "nvim-a".into(),
            parent: NvimParent::ZellijPane {
                client,
                pane_id: 10,
            },
            terminal_focused: false,
            revision: 1,
            focused_window: 100,
            window_neighbors: BTreeMap::from([(
                "100".into(),
                NeighborMap {
                    right: nvim_right,
                    ..NeighborMap::default()
                },
            )]),
        });
        graph
    }

    #[test]
    fn extreme_nested_edge_routes_directly_to_niri() {
        let mut graph = nested_graph(None, None);
        assert_eq!(
            graph.route_optimistically(Direction::Right).unwrap(),
            NavigationAction::Niri {
                direction: Direction::Right
            }
        );
    }

    #[test]
    fn deepest_available_neighbor_wins() {
        let mut graph = nested_graph(Some(101), Some(11));
        assert!(matches!(
            graph.route_optimistically(Direction::Right),
            Ok(NavigationAction::Nvim { .. })
        ));
    }

    #[test]
    fn repeated_input_uses_predicted_state_before_ack() {
        let mut graph = nested_graph(Some(101), Some(11));
        graph
            .nvim
            .get_mut("nvim-a")
            .unwrap()
            .window_neighbors
            .insert("101".into(), NeighborMap::default());
        assert!(matches!(
            graph.route_optimistically(Direction::Right),
            Ok(NavigationAction::Nvim { .. })
        ));
        assert!(matches!(
            graph.route_optimistically(Direction::Right),
            Ok(NavigationAction::Zellij { .. })
        ));
    }

    #[test]
    fn authoritative_snapshot_reconciles_a_wrong_prediction() {
        let mut graph = nested_graph(Some(101), None);
        graph.route_optimistically(Direction::Right).unwrap();
        let mut correction = graph.nvim["nvim-a"].clone();
        correction.revision += 1;
        correction.focused_window = 100;
        graph.update_nvim(correction);
        assert!(matches!(
            graph.route_optimistically(Direction::Right),
            Ok(NavigationAction::Nvim { .. })
        ));
    }

    #[test]
    fn cli_snapshot_wins_over_stale_plugin_snapshot() {
        let mut graph = nested_graph(None, None);
        graph.update_zellij(ZellijClientState {
            client: ZellijClient {
                session: "dev".into(),
                client_id: 0,
            },
            niri_window_id: 1,
            revision: 1,
            focused_pane: 10,
            pane_neighbors: BTreeMap::from([(
                "10".into(),
                NeighborMap {
                    right: Some(11),
                    ..NeighborMap::default()
                },
            )]),
        });

        assert!(matches!(
            graph.route_optimistically(Direction::Right),
            Ok(NavigationAction::Zellij {
                client: ZellijClient { client_id: 0, .. },
                ..
            })
        ));
    }

    #[test]
    fn direct_nvim_keeps_its_original_niri_parent() {
        let mut graph = NavigationGraph::default();
        graph.replace_niri_windows([], Some(1));
        let state = NvimInstance {
            id: "direct".into(),
            parent: NvimParent::FocusedNiriWindow,
            terminal_focused: true,
            revision: 1,
            focused_window: 100,
            window_neighbors: BTreeMap::new(),
        };
        graph.update_nvim(state.clone());
        graph.set_focused_niri_window(Some(2));
        graph.update_nvim(NvimInstance {
            revision: 2,
            ..state
        });
        assert!(matches!(
            graph.nvim["direct"].parent,
            NvimParent::NiriWindow(1)
        ));
    }

    #[test]
    fn unfocused_direct_nvim_does_not_claim_the_focused_window() {
        let mut graph = NavigationGraph::default();
        graph.replace_niri_windows([], Some(1));
        graph.update_nvim(NvimInstance {
            id: "background".into(),
            parent: NvimParent::FocusedNiriWindow,
            terminal_focused: false,
            revision: 1,
            focused_window: 100,
            window_neighbors: BTreeMap::new(),
        });

        assert!(matches!(
            graph.nvim["background"].parent,
            NvimParent::FocusedNiriWindow
        ));
    }

    #[test]
    fn focused_direct_nvim_binds_on_the_next_niri_snapshot() {
        let mut graph = NavigationGraph::default();
        graph.update_nvim(NvimInstance {
            id: "pending".into(),
            parent: NvimParent::FocusedNiriWindow,
            terminal_focused: true,
            revision: 1,
            focused_window: 100,
            window_neighbors: BTreeMap::new(),
        });
        graph.replace_niri_windows([], Some(7));

        assert!(matches!(
            graph.nvim["pending"].parent,
            NvimParent::NiriWindow(7)
        ));
    }
}
