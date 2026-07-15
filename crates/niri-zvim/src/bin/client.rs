use std::{io::Write, os::unix::net::UnixStream};

use niri_zvim::socket_path;
use niri_zvim_core::Direction;

fn main() -> anyhow::Result<()> {
    let direction = match std::env::args().nth(1).as_deref() {
        Some("left") => Direction::Left,
        Some("down") => Direction::Down,
        Some("up") => Direction::Up,
        Some("right") => Direction::Right,
        _ => anyhow::bail!("usage: niri-zvim <left|down|up|right>"),
    };
    let mut socket = UnixStream::connect(socket_path())?;
    socket.write_all(&[direction.wire_byte()])?;
    Ok(())
}
