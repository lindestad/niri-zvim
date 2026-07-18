use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use thiserror::Error;

pub const PROTOCOL_VERSION: u8 = 1;
pub const CONTROL_MAGIC: u8 = 0x7e;
pub const ADAPTER_MAGIC: u8 = 0x7f;

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

    pub const fn control_opcode(self) -> u8 {
        match self {
            Self::Left => 1,
            Self::Down => 2,
            Self::Up => 3,
            Self::Right => 4,
        }
    }

    pub const fn from_control_opcode(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Left),
            2 => Some(Self::Down),
            3 => Some(Self::Up),
            4 => Some(Self::Right),
            _ => None,
        }
    }

    pub const fn control_frame(self) -> [u8; 3] {
        [CONTROL_MAGIC, PROTOCOL_VERSION, self.control_opcode()]
    }
}

pub const fn adapter_prelude() -> [u8; 2] {
    [ADAPTER_MAGIC, PROTOCOL_VERSION]
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtocolMessage<T> {
    pub protocol_version: u8,
    pub message: T,
}

impl<T> ProtocolMessage<T> {
    pub const fn new(message: T) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            message,
        }
    }

    pub fn into_current(self) -> Result<T, ProtocolVersionError> {
        if self.protocol_version == PROTOCOL_VERSION {
            Ok(self.message)
        } else {
            Err(ProtocolVersionError {
                received: self.protocol_version,
                supported: PROTOCOL_VERSION,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("protocol version {received} is not supported; expected {supported}")]
pub struct ProtocolVersionError {
    pub received: u8,
    pub supported: u8,
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
    FocusedNiriWindow,
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
    #[serde(deserialize_with = "deserialize_required_option")]
    pub acknowledged_sequence: Option<u64>,
    pub focused_pane: PaneId,
    pub pane_neighbors: BTreeMap<String, NeighborMap<PaneId>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NvimInstance {
    pub id: String,
    pub parent: NvimParent,
    pub terminal_focused: bool,
    pub revision: Revision,
    #[serde(deserialize_with = "deserialize_required_option")]
    pub acknowledged_sequence: Option<u64>,
    pub focused_window: u64,
    pub window_neighbors: BTreeMap<String, NeighborMap<u64>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AdapterMessage {
    ZellijSnapshot { state: ZellijClientState },
    NvimSnapshot { state: NvimInstance },
    NvimClosed { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonMessage {
    BindNiriWindow { window_id: u64, session: String },
    Navigate { sequence: u64, direction: Direction },
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

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_state_requires_every_current_field() {
        let zellij = ZellijClientState {
            client: ZellijClient {
                session: "current".into(),
                client_id: 1,
            },
            niri_window_id: 2,
            revision: 3,
            acknowledged_sequence: None,
            focused_pane: 4,
            pane_neighbors: BTreeMap::new(),
        };
        assert_field_is_required::<ZellijClientState>(zellij, "acknowledged_sequence");

        let nvim = NvimInstance {
            id: "current".into(),
            parent: NvimParent::NiriWindow(2),
            terminal_focused: true,
            revision: 3,
            acknowledged_sequence: None,
            focused_window: 4,
            window_neighbors: BTreeMap::new(),
        };
        assert_field_is_required::<NvimInstance>(nvim.clone(), "terminal_focused");
        assert_field_is_required::<NvimInstance>(nvim, "acknowledged_sequence");
    }

    #[test]
    fn control_and_adapter_connections_are_explicitly_versioned() {
        assert_eq!(Direction::Left.control_frame(), [0x7e, 1, 1]);
        assert_eq!(adapter_prelude(), [0x7f, 1]);
        assert_eq!(Direction::from_control_opcode(4), Some(Direction::Right));
        assert_eq!(Direction::from_control_opcode(CONTROL_MAGIC), None);
    }

    #[test]
    fn nested_protocol_messages_reject_other_versions() {
        let encoded = serde_json::to_string(&ProtocolMessage::new(DaemonMessage::Navigate {
            sequence: 4,
            direction: Direction::Up,
        }))
        .unwrap();
        assert_eq!(
            encoded,
            r#"{"protocol_version":1,"message":{"type":"navigate","sequence":4,"direction":"up"}}"#
        );

        let stale = ProtocolMessage {
            protocol_version: 0,
            message: DaemonMessage::Navigate {
                sequence: 4,
                direction: Direction::Up,
            },
        };
        assert_eq!(stale.into_current().unwrap_err().received, 0);
    }

    fn assert_field_is_required<T>(state: T, field: &str)
    where
        T: Serialize + serde::de::DeserializeOwned,
    {
        let mut value = serde_json::to_value(state).unwrap();
        value.as_object_mut().unwrap().remove(field).unwrap();
        assert!(serde_json::from_value::<T>(value).is_err());
    }
}
