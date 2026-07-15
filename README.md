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

See docs/architecture.md for the protocol and invariants.

## Install

Requirements are Niri, Zellij 0.44, Neovim 0.10 or newer, Ghostty, Rust, and
systemd user services. Install the binaries, adapters, and daemon with:

    scripts/install

The installer puts the client and daemon in `~/.local/bin`, the Zellij plugin
in `~/.config/zellij/plugins`, and the Neovim adapter in its user site runtime.
It enables `niri-zvim.service` immediately and enables Ghostty's packaged
user service for the next graphical login.

Zellij requires a one-time interactive permission approval. From any Zellij
session, run:

    zellij action launch-or-focus-plugin --floating \
      file:$HOME/.config/zellij/plugins/niri-zvim.wasm

Approve the prompt. The plugin then hides itself; later sessions are connected
automatically when the daemon sees their Ghostty window.

Bind the compositor keys to the client:

    Mod+Left  { spawn "niri-zvim" "left"; }
    Mod+Down  { spawn "niri-zvim" "down"; }
    Mod+Up    { spawn "niri-zvim" "up"; }
    Mod+Right { spawn "niri-zvim" "right"; }

For systemd-managed Ghostty, launch terminal windows with `ghostty +new-window`.
Do not disable Ghostty's GTK single-instance mode.

## Development

Run the checks and latency benchmarks with:

    cargo test --workspace --exclude niri-zvim-zellij
    cargo clippy --workspace --all-targets --exclude niri-zvim-zellij -- -D warnings
    cargo clippy -p niri-zvim-zellij --target wasm32-wasip1 -- -D warnings
    scripts/bench

The benchmark suite measures both in-memory optimistic routing and the Unix
socket connect/write operation used by a keypress.
