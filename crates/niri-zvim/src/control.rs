use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
    time::Duration,
};

use anyhow::Context;
use niri_zvim_core::{ControlResponse, DaemonStatus, ProtocolMessage, status_frame};

use crate::socket::socket_path;

const STATUS_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_STATUS_BYTES: u64 = 1024 * 1024;

pub fn request_status() -> anyhow::Result<DaemonStatus> {
    let path = socket_path()?;
    let mut socket = UnixStream::connect(&path)
        .with_context(|| format!("could not connect to daemon at {}", path.display()))?;
    socket.set_read_timeout(Some(STATUS_TIMEOUT))?;
    socket.set_write_timeout(Some(STATUS_TIMEOUT))?;
    socket.write_all(&status_frame())?;
    let mut encoded = Vec::new();
    socket
        .take(MAX_STATUS_BYTES + 1)
        .read_to_end(&mut encoded)
        .context("could not read daemon status")?;
    anyhow::ensure!(
        !encoded.is_empty(),
        "daemon returned no status; client and daemon protocols may not match"
    );
    anyhow::ensure!(
        encoded.len() as u64 <= MAX_STATUS_BYTES,
        "daemon status exceeds {MAX_STATUS_BYTES} bytes"
    );
    let response = serde_json::from_slice::<ProtocolMessage<ControlResponse>>(&encoded)
        .context("daemon returned an invalid status response")?
        .into_current()?;
    match response {
        ControlResponse::Status { status } => Ok(status),
    }
}
