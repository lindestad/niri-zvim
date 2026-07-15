# niri-zvim

State-synchronized directional navigation across niri windows, Zellij panes,
and Neovim splits.

niri-zvimd keeps a live graph of focus and topology. Neovim and a background
Zellij plugin push changes to it; the niri-zvim command sends one-byte
navigation requests from niri key bindings. A request is routed directly to
the deepest layer that has a neighbor in that direction, otherwise it falls
through to niri.

Focus changes are applied optimistically. Repeated requests route against the
predicted graph without waiting for acknowledgements; authoritative snapshots
then confirm or reconcile the prediction.

The project is under active development. See docs/architecture.md for the
protocol and invariants.

## Development

Run the checks and latency benchmarks with:

    cargo test --workspace --exclude niri-zvim-zellij
    cargo clippy --workspace --all-targets --exclude niri-zvim-zellij -- -D warnings
    cargo clippy -p niri-zvim-zellij --target wasm32-wasip1 -- -D warnings
    scripts/bench

The benchmark suite measures both in-memory optimistic routing and the Unix
socket connect/write operation used by a keypress.
