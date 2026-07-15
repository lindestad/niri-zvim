use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type Revision = u64;
pub type PaneId = u32;
pub type ZellijClientId = u16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Left,
    Down,
    Up,
    Right,
}

impl Direction {
    pub const ALL: [Self; 4] = [Self::Left, Self::Down, Self::Up, Self::Right];

    pub const fn wire_byte(self) -> u8 {
        match self {
            Self::Left => 1,
            Self::Down => 2,
            Self::Up => 3,
            Self::Right => 4,
        }
    }

    pub const fn from_wire_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Left),
            2 => Some(Self::Down),
            3 => Some(Self::Up),
            4 => Some(Self::Right),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborMap<T> {
    pub left: Option<T>,
    pub down: Option<T>,
    pub up: Option<T>,
    pub right: Option<T>,
}

impl<T> Default for NeighborMap<T> {
    fn default() -> Self {
        Self {
            left: None,
            down: None,
            up: None,
            right: None,
        }
    }
}

impl<T> NeighborMap<T> {
    pub fn get(&self, direction: Direction) -> Option<&T> {
        match direction {
            Direction::Left => self.left.as_ref(),
            Direction::Down => self.down.as_ref(),
            Direction::Up => self.up.as_ref(),
            Direction::Right => self.right.as_ref(),
        }
    }

    pub fn set(&mut self, direction: Direction, value: Option<T>) {
        match direction {
            Direction::Left => self.left = value,
            Direction::Down => self.down = value,
            Direction::Up => self.up = value,
            Direction::Right => self.right = value,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ZellijClient {
    pub session: String,
    pub client_id: ZellijClientId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NvimParent {
    NiriWindow(u64),
    ZellijPane {
        client: ZellijClient,
        pane_id: PaneId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NiriWindow {
    pub id: u64,
    pub app_id: Option<String>,
    pub title: Option<String>,
    pub neighbors: NeighborMap<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ZellijClientState {
    pub client: ZellijClient,
    pub niri_window_id: u64,
    pub revision: Revision,
    pub focused_pane: PaneId,
    pub pane_neighbors: BTreeMap<PaneId, NeighborMap<PaneId>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NvimInstance {
    pub id: String,
    pub parent: NvimParent,
    pub revision: Revision,
    pub focused_window: u64,
    pub window_neighbors: BTreeMap<u64, NeighborMap<u64>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdapterMessage {
    ZellijSnapshot(ZellijClientState),
    NvimSnapshot(NvimInstance),
    NvimClosed { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationAction {
    Niri {
        direction: Direction,
    },
    Zellij {
        client: ZellijClient,
        direction: Direction,
    },
    Nvim {
        id: String,
        direction: Direction,
    },
}
