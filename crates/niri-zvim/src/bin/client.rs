use std::{io::Write, os::unix::net::UnixStream};

use niri_zvim::socket_path;
use niri_zvim_core::Direction;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args();
    let _program = args.next();
    let argument = args.next();
    if args.next().is_some() {
        anyhow::bail!(usage());
    }

    let direction = match argument.as_deref() {
        Some("left") => Direction::Left,
        Some("down") => Direction::Down,
        Some("up") => Direction::Up,
        Some("right") => Direction::Right,
        Some("-V" | "--version") => {
            println!("niri-zvim {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("-h" | "--help") => {
            println!("{}", usage());
            return Ok(());
        }
        _ => anyhow::bail!(usage()),
    };
    let mut socket = UnixStream::connect(socket_path()?)?;
    socket.write_all(&[direction.wire_byte()])?;
    Ok(())
}

fn usage() -> &'static str {
    "usage: niri-zvim <left|down|up|right>"
}
