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

Requirements are Niri, Zellij 0.44, Neovim 0.10 or newer, Ghostty, Rust, Just,
and systemd user services. Install the binaries, adapters, and daemon with:

    just install

The installer puts the client and daemon in `~/.local/bin`, the Zellij plugin
in `~/.config/zellij/plugins`, and the Neovim adapter in its user site runtime.
It creates `~/.config/niri-zvim/config.json` when missing, enables
`niri-zvim.service` immediately, and enables Ghostty's packaged user service
for the next graphical login.

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

## Navigation modes

The daemon reads named modes from `~/.config/niri-zvim/config.json`. Without a
config file it uses workspace-local Niri navigation: Left/Right focus columns,
and Up/Down focus windows without crossing a monitor or workspace boundary.

The installed config selects the `desktop` mode, matching this layout:

```json
{
  "active_mode": "desktop",
  "modes": {
    "default": {
      "left": "focus-column-left",
      "down": "focus-window-down",
      "up": "focus-window-up",
      "right": "focus-column-right"
    },
    "desktop": {
      "left": "focus-column-or-monitor-left",
      "down": "focus-window-or-workspace-down",
      "up": "focus-window-or-workspace-up",
      "right": "focus-column-or-monitor-right"
    }
  }
}
```

Restart `niri-zvim.service` after changing the active mode. Niri fallthrough
actions are sent over the daemon's persistent Niri IPC socket; no `niri msg`
process is launched on a keypress.

## Development

Run the complete test suite, including the live desktop tests, and the latency
benchmarks with:

    just test
    just bench

The test command checks the required tool versions and active user services.
For individual development checks, run:

    cargo test
    cargo clippy --workspace --all-targets --exclude niri-zvim-zellij -- -D warnings
    cargo clippy -p niri-zvim-zellij --target wasm32-wasip1 -- -D warnings

The live part launches disposable Ghostty windows with an isolated test config.
It traverses empty terminals, direct Neovim instances, one- and three-pane
Zellij sessions, nested Neovim windows inside Zellij panes, and three temporary
workspaces in both directions. Do not interact with the desktop until it
finishes. It terminates only test processes, removes only unique test workspace
names, verifies that the original windows and workspaces are unchanged, and
restores the previously focused Niri window. Run it with a normal Niri window
focused, not from overview or while a layer-shell surface owns keyboard focus.
Each live operation fails after 3 seconds, with a 20-second overall safety cap
per case. Failures print the last checkpoint plus the current Niri, Zellij,
Neovim, and process state before cleanup. Set `NIRI_ZVIM_LIVE_TIMEOUT` to change
the per-case cap when debugging. Test Neovim instances use a minimal init from
the repository rather than the user's configuration. Run one case in isolation
with, for example, `scripts/test-live direct`.

`scripts/test-live` handles preflight checks and orchestration. Shared fixture
helpers and the individual scenario files live under `scripts/live-tests/`.

The benchmark suite measures both in-memory optimistic routing and the Unix
socket connect/write operation used by a keypress.
