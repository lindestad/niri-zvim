use std::{collections::BTreeMap, hint::black_box};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use niri_zvim_core::{
    Direction, NavigationGraph, NeighborMap, NiriWindow, NvimInstance, NvimParent, Rect,
    ZellijClient, ZellijClientState, directional_neighbors,
};

fn chain_neighbors(index: usize, count: usize) -> NeighborMap<u64> {
    NeighborMap {
        left: index.checked_sub(1).map(|value| value as u64),
        right: (index + 1 < count).then_some((index + 1) as u64),
        ..NeighborMap::default()
    }
}

fn niri_graph(count: usize) -> NavigationGraph {
    let mut graph = NavigationGraph::default();
    graph.replace_niri_windows(
        (0..count).map(|index| NiriWindow {
            id: index as u64,
            app_id: Some("com.mitchellh.ghostty".into()),
            title: None,
            neighbors: chain_neighbors(index, count),
        }),
        Some(0),
    );
    graph
}

fn nvim_graph(count: usize, nested: bool) -> NavigationGraph {
    let mut graph = niri_graph(1);
    let client = ZellijClient {
        session: "bench".into(),
        client_id: 0,
    };
    let parent = if nested {
        graph.update_zellij(ZellijClientState {
            client: client.clone(),
            niri_window_id: 0,
            revision: 0,
            acknowledged_sequence: None,
            focused_pane: 1,
            pane_neighbors: BTreeMap::from([("1".into(), NeighborMap::default())]),
        });
        NvimParent::ZellijPane { client, pane_id: 1 }
    } else {
        NvimParent::NiriWindow(0)
    };
    graph.update_nvim(NvimInstance {
        id: "bench".into(),
        parent,
        terminal_focused: true,
        revision: 0,
        acknowledged_sequence: None,
        focused_window: 0,
        window_neighbors: (0..count)
            .map(|index| (index.to_string(), chain_neighbors(index, count)))
            .collect(),
    });
    graph
}

fn direct_nvim_graph(count: usize) -> NavigationGraph {
    nvim_graph(count, false)
}

fn nested_nvim_graph(count: usize) -> NavigationGraph {
    nvim_graph(count, true)
}

fn routing(c: &mut Criterion) {
    let builders: [(&str, fn(usize) -> NavigationGraph); 3] = [
        ("niri", niri_graph),
        ("direct-neovim", direct_nvim_graph),
        ("nested-neovim", nested_nvim_graph),
    ];
    for (name, builder) in builders {
        let mut group = c.benchmark_group(format!("optimistic-route/{name}"));
        for count in [2, 16, 64] {
            let mut graph = builder(count);
            group.throughput(Throughput::Elements(2));
            group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
                b.iter(|| {
                    black_box(graph.route_optimistically(Direction::Right).unwrap());
                    black_box(graph.route_optimistically(Direction::Left).unwrap());
                });
            });
        }
        group.finish();
    }
}

fn topology(c: &mut Criterion) {
    let mut group = c.benchmark_group("directional-topology");
    for count in [8, 32, 128] {
        let rectangles: Vec<_> = (0..count)
            .map(|index| {
                (
                    index as u64,
                    Rect {
                        x: index as f64 * 100.0,
                        y: 0.0,
                        width: 100.0,
                        height: 100.0,
                    },
                )
            })
            .collect();
        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(count),
            &rectangles,
            |b, input| {
                b.iter(|| black_box(directional_neighbors(input.iter().copied())));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, routing, topology);
criterion_main!(benches);
