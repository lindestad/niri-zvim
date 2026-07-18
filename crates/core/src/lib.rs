mod geometry;
mod graph;
mod protocol;

pub use geometry::{Rect, directional_neighbors};
pub use graph::{NavigationGraph, RouteError};
pub use protocol::{
    ADAPTER_MAGIC, AdapterMessage, CONTROL_MAGIC, DaemonMessage, Direction, NavigationAction,
    NeighborMap, NiriWindow, NvimInstance, NvimParent, PROTOCOL_VERSION, PaneId, ProtocolMessage,
    ProtocolVersionError, Revision, ZellijClient, ZellijClientId, ZellijClientState,
    adapter_prelude,
};
