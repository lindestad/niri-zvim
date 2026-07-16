# Benchmarking specification

The project has two benchmark layers because dispatch cost and desktop focus
latency answer different questions.

## Deterministic microbenchmarks

`just bench` runs Criterion without controlling the desktop. It measures:

- optimistic Niri, direct Neovim, and nested Neovim routing with 2, 16, and 64
  nodes;
- directional topology construction with 8, 32, and 128 rectangles;
- Unix socket connect/write cost;
- the complete short-lived keypress client process.

These measurements isolate CPU and IPC overhead. They do not include executor
scheduling, application focus changes, adapter acknowledgements, or compositor
frame scheduling.

## Live comparative benchmarks

`just bench-live` opens disposable Ghostty windows and compares native command
paths with `niri-zvim` for Niri columns, Zellij panes, and Neovim windows. Do
not use the keyboard or mouse until its final CPU-strain summary.

Zellij's native and daemon populations use equivalent three-pane sessions,
but separate instances. This prevents a native reset from racing the daemon's
optimistic acknowledgement queue. Resets and fixture synchronization happen
outside measured intervals; the two commands inside each burst have no delay.

For each backend it reports distributions for:

1. trigger completion: time until the native command or `niri-zvim` client
   exits;
2. observed focus: time until an authoritative backend query sees the target;
3. two-step convergence: time to dispatch two immediate moves and observe the
   final target.

The default is 12 single samples in each direction and 6 burst samples. Set
`NIRI_ZVIM_BENCH_SAMPLES` and `NIRI_ZVIM_BENCH_BURSTS` to positive integers to
change those counts. Pass `niri`, `zellij`, or `nvim` directly to
`scripts/bench-live` to isolate one backend while investigating it.

Results include mean, p50, p95, p99, and maximum latency. They are descriptive,
not correctness thresholds. The benchmark prints Linux load and CPU pressure
at the start and end, including on failure, so a strained run is recognizable.

### Interpretation limits

The native comparison is the supported command-line interface, not an internal
application keybinding:

- Niri uses `niri msg action`, which includes process startup and a new IPC
  connection. An internal Niri binding is a lower bound that the benchmark
  cannot inject directly.
- Zellij uses `zellij action move-focus`. Authoritative observation uses
  `list-clients`, whose process and session-query cost is substantial and is
  included equally in native and daemon totals.
- Neovim uses its remote API to execute `wincmd` and query `win_getid()`.

Trigger-completion numbers best show client and dispatch overhead. Observed
focus numbers best represent practical external convergence, but include the
observer costs above. Compare native and daemon results within the same run;
do not compare absolute totals across different machines or desktop loads.

## Performance invariants

Normal key handling must not launch Niri, Zellij, or Neovim query processes.
The daemon routes from its cached graph and sends exactly one executor command.
Authoritative work happens after dispatch and is coalesced:

- Niri actions use one persistent action socket. A separate worker coalesces
  command sequences after a short quiet period and requests one window
  snapshot, so no-op actions are acknowledged without blocking a burst.
- Zellij trusts plugin acknowledgements. One per-session fallback timer is
  reset by rapid input, cancelled by a sufficient acknowledgement, and launches
  CLI state queries only if the plugin remains late. Focus and topology metadata
  changes enter that same debounced worker, so native changes remain observable
  without starting a second query path.
- Neovim caches window topology until a structural or resize event and reuses
  one graph while draining a queued burst.
- The Zellij plugin caches geometry-derived neighbors until pane geometry,
  visibility, selectability, tab membership, or floating state changes.

## Deferred optimizations

The daemon still uses small `BTreeMap`s, scans adapter ownership maps, clones
some identifiers, and converts numeric neighbor IDs to strings for protocol
lookups. A short-lived stream and Tokio task are also created per keypress.
Current measurements put the complete client dispatch below one millisecond,
so these changes are intentionally deferred until size-parameterized or live
benchmarks identify them as meaningful.
