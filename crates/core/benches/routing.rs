use std::collections::BTreeMap;

use criterion::{Criterion, criterion_group, criterion_main};
use niri_zvim_core::{
    Direction, NavigationGraph, NeighborMap, NiriWindow, NvimInstance, NvimParent,
};

fn graph() -> NavigationGraph {
    let mut graph = NavigationGraph::default();
    graph.replace_niri_windows(
        [NiriWindow {
            id: 1,
            app_id: Some("com.mitchellh.ghostty".into()),
            title: Some("dev".into()),
            neighbors: NeighborMap::default(),
        }],
        Some(1),
    );
    graph.update_nvim(NvimInstance {
        id: "bench".into(),
        parent: NvimParent::NiriWindow(1),
        terminal_focused: true,
        revision: 0,
        acknowledged_sequence: None,
        focused_window: 1,
        window_neighbors: BTreeMap::from([
            (
                "1".into(),
                NeighborMap {
                    right: Some(2),
                    ..NeighborMap::default()
                },
            ),
            (
                "2".into(),
                NeighborMap {
                    left: Some(1),
                    ..NeighborMap::default()
                },
            ),
        ]),
    });
    graph
}

fn routing(c: &mut Criterion) {
    let mut graph = graph();
    c.bench_function("optimistic nested route", |b| {
        b.iter(|| {
            graph.route_optimistically(Direction::Right).unwrap();
            graph.route_optimistically(Direction::Left).unwrap();
        });
    });
}

criterion_group!(benches, routing);
criterion_main!(benches);
