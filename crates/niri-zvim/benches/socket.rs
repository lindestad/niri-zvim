use std::{
    io::{Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    process::Command,
    thread,
};

use criterion::{Criterion, criterion_group, criterion_main};
use niri_zvim_core::Direction;

fn socket_dispatch(c: &mut Criterion) {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("bench.sock");
    let listener = UnixListener::bind(&path).unwrap();
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut direction = [0];
            stream.read_exact(&mut direction).unwrap();
        }
    });

    c.bench_function("connect and send navigation byte", |b| {
        b.iter(|| {
            let mut stream = UnixStream::connect(&path).unwrap();
            stream.write_all(&[Direction::Right.wire_byte()]).unwrap();
        });
    });

    c.bench_function("keypress client process", |b| {
        b.iter(|| {
            let status = Command::new(env!("CARGO_BIN_EXE_niri-zvim"))
                .arg("right")
                .env("NIRI_ZVIM_SOCKET", &path)
                .status()
                .unwrap();
            assert!(status.success());
        });
    });
}

criterion_group!(benches, socket_dispatch);
criterion_main!(benches);
