# niri-zvim

State-synchronized directional navigation across niri windows, Zellij panes,
and Neovim splits.

Version 0.2 is under development. Version 0.1 remains a developer preview for
the stack it was built and tested on.
Zellij discovery is currently specific to Ghostty, and the source installer is
intended for people who are comfortable inspecting and maintaining a local
checkout. See
[support and compatibility](https://github.com/lindestad/niri-zvim/blob/main/docs/support.md)
before installing.

niri-zvimd keeps a live graph of focus and topology. Neovim and a background
Zellij plugin push changes to it; the niri-zvim command sends a three-byte,
versioned navigation request from niri key bindings. A request is routed
directly to the deepest layer that has a neighbor in that direction, otherwise
it falls through to niri.

Focus changes are applied optimistically. Repeated requests route against the
predicted graph without waiting for acknowledgements; authoritative snapshots
then confirm or reconcile the prediction.

See
[the architecture documentation](https://github.com/lindestad/niri-zvim/blob/main/docs/architecture.md)
for the protocol and invariants, and
[the live-testing documentation](https://github.com/lindestad/niri-zvim/blob/main/docs/live-testing.md)
for the desktop race tests and lessons from their fixtures.

## Install from source

The complete 0.1 integration requires niri 26.04, Zellij 0.44.3, Neovim
0.12.4, Ghostty's GTK build, Rust 1.97.1 or newer, Just, and systemd user
services. Install the binaries, adapters, and daemon with:

    just install

The installer puts the client and daemon in `~/.local/bin`, the Zellij plugin
in `~/.config/zellij/plugins`, and the Neovim adapter in its user site runtime.
It creates `~/.config/niri-zvim/config.json` when missing, enables
`niri-zvim.service` immediately, and enables Ghostty's packaged user service
for the next graphical login. The Neovim files are symlinked to this checkout,
so the checkout must remain in place. These side effects are part of the 0.1
source installer and will be removed from the general installer in 0.2.

Once published, `cargo install --locked niri-zvim` installs only the native
client and daemon. It does not install the Zellij WASM plugin, Neovim adapter,
configuration, or service; `just install` is currently the complete path.

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

The Zellij bridge does not require a visible title bar. It reads the Wayland
window title reported by niri. The supported setup expects Ghostty's app ID to
be `com.mitchellh.ghostty` and the title to be either the Zellij session name
or `<session> | <command>`. Hiding client-side decorations is fine; overriding
the terminal title with a static value prevents discovery.

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

Run formatting, unit and integration tests, native and WASM Clippy, ShellCheck,
and crate packaging without controlling the desktop with:

    just check

Run the complete test suite, including the live desktop tests, and the latency
benchmarks separately with:

    just test
    just bench
    just bench-live

The test command checks the required tool versions and active user services.
For individual Rust checks, run:

    cargo test
    cargo clippy --workspace --all-targets --all-features --exclude niri-zvim-zellij -- -D warnings
    cargo clippy -p niri-zvim-zellij --target wasm32-wasip1 -- -D warnings

The live part launches disposable Ghostty windows with an isolated test config.
Immediately before it takes desktop control, it shows a small two-second
Ghostty warning and restores the previously focused window when the warning
closes.
Disposable Zellij sessions use an isolated permission cache that grants the
repository's plugin only the four permissions documented above; the user's
Zellij permission cache is not read or changed. Their session metadata remains
visible to the daemon so the test exercises its normal topology refresh path.
It traverses empty terminals, a Niri tabbed column, direct Neovim instances,
one- and three-pane Zellij sessions, nested Neovim windows inside Zellij panes,
and three temporary workspaces in both directions. Do not interact with the
desktop until it finishes. Unexpected focus on a window outside the test is
reported as an interrupted run rather than a navigation failure. The harness
terminates only test processes, removes only unique test workspace names,
verifies that the original windows and workspaces are unchanged, and restores
the previously focused Niri window. Run it with a normal Niri window focused,
not from overview or while a layer-shell surface owns keyboard focus.
Each live operation fails after 3 seconds, with a 20-second overall safety cap
per case. Failures print the last checkpoint, CPU load and pressure, plus the
current Niri, Zellij, Neovim, and process state before cleanup. Set
`NIRI_ZVIM_LIVE_TIMEOUT` to change the per-case cap when debugging. Test
Neovim instances use a minimal init from the repository rather than the user's
configuration. Run one case in isolation with, for example,
`scripts/test-live tabs`.

`scripts/test-live` handles preflight checks and orchestration. Shared fixture
helpers and the individual scenario files live under `scripts/live-tests/`.

The deterministic benchmark suite measures size-parameterized routing,
topology construction, Unix socket dispatch, and the short-lived keypress
client. `just bench-live` separately compares native and daemon-triggered Niri,
Zellij, and Neovim focus convergence on the running desktop. See
[the benchmarking documentation](https://github.com/lindestad/niri-zvim/blob/main/docs/benchmarking.md)
for methodology and interpretation limits.

See
[the changelog](https://github.com/lindestad/niri-zvim/blob/main/CHANGELOG.md)
for release notes.
