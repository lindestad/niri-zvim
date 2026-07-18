mod geometry;
mod graph;
mod protocol;

pub use geometry::{Rect, directional_neighbors};
pub use graph::{NavigationGraph, RouteError};
pub use protocol::{
    ADAPTER_MAGIC, AdapterMessage, CONTROL_MAGIC, ControlResponse, DaemonMessage, DaemonStatus,
    Direction, NavigationAction, NeighborMap, NiriStatus, NiriWindow, NvimInstance, NvimParent,
    NvimStatus, PROTOCOL_VERSION, PaneId, ProtocolMessage, ProtocolVersionError, Revision,
    STATUS_OPCODE, ZellijClient, ZellijClientId, ZellijClientState, ZellijStatus, adapter_prelude,
    status_frame,
};
