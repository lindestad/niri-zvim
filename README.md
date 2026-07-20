# niri-zvim

State-synchronized directional navigation across niri windows, Zellij panes,
and Neovim splits.

Version 0.3 is an enthusiast release for the current niri, Zellij, and Neovim
stack documented below. Zellij navigation is live-tested in Ghostty, Alacritty,
Kitty, Foot, and WezTerm. See
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

## Install

The complete integration requires niri 26.04, Zellij 0.44.3, Neovim 0.12.4,
one supported Wayland terminal, Rust 1.97.1 or newer, Just, and systemd user
services. From a source checkout, install the binaries, adapters, and daemon
with:

    just install

The installer puts the client, daemon, and SSH Zellij bridge in
`~/.local/bin`, the Zellij plugin in `~/.config/zellij/plugins`, and an owned
copy of the Neovim adapter under `~/.local/share/niri-zvim`. It links that
stable copy into Neovim's user site, so the source checkout can be removed
afterward. It creates
`~/.config/niri-zvim/config.json` only when missing and starts the systemd user
service. Pass `--no-service` to install the files without enabling or
restarting the service.

GitHub release archives contain the same complete integration prebuilt for
their named Linux target and do not require Rust. Extract an archive, verify
its adjacent `.sha256` file, and run its `install` script.

`cargo install --locked niri-zvim` installs only the native client, daemon,
and SSH Zellij bridge. It does not install the Zellij WASM plugin, Neovim
adapter, configuration, or service; the source or release-archive installer is
the complete path.

Enable the installed Neovim adapter explicitly in your configuration:

```lua
require("niri-zvim").setup()
```

Loading its runtime files alone does not connect to the daemon. The complete
setup surface is deliberately small:

```lua
require("niri-zvim").setup({
  enabled = true,
  socket_path = nil,          -- NIRI_ZVIM_SOCKET, then XDG_RUNTIME_DIR
  reconnect_interval_ms = 250,
})
```

Unknown or invalid options are rejected. `require("niri-zvim").disable()`
disconnects, removes its autocommands, and removes the instance from the daemon
graph. `:NiriZvimEnable` and `:NiriZvimDisable` provide the same runtime toggle;
enabling again retains the last configured socket and retry interval.

Zellij requires a one-time interactive permission approval. From any Zellij
session, run:

    zellij action launch-or-focus-plugin --floating \
      file:$HOME/.config/zellij/plugins/niri-zvim.wasm

Approve the prompt. The plugin then hides itself; later sessions are connected
automatically when the daemon sees their terminal window.

Bind the compositor keys to the client:

    Mod+Left  { spawn "niri-zvim" "left"; }
    Mod+Down  { spawn "niri-zvim" "down"; }
    Mod+Up    { spawn "niri-zvim" "up"; }
    Mod+Right { spawn "niri-zvim" "right"; }

The Zellij bridge does not require a visible title bar. It reads the Wayland
app ID and window title reported by niri. The defaults recognize Ghostty,
Alacritty, Kitty, Foot/Footclient, and WezTerm. The title must be either the
Zellij session name or `<session> | <command>`. Hiding client-side decorations
is fine; overriding the terminal title with a static value prevents discovery.
Each Zellij client must occupy its own Wayland toplevel; terminal-native tabs
and splits are outside niri-zvim's graph.

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
  },
  "zellij": {
    "terminal_app_ids": [
      "com.mitchellh.ghostty",
      "Alacritty",
      "kitty",
      "foot",
      "footclient",
      "org.wezfurlong.wezterm"
    ],
    "session_title_separator": " | "
  }
}
```

Inspect or validate the effective configuration without starting the daemon:

    niri-zvim config check
    niri-zvim config show

Both commands accept `--json`. Niri fallthrough actions are sent over the
daemon's persistent Niri IPC socket; no `niri msg` process is launched on a
keypress. The daemon watches this file and applies valid mode and discovery
changes without a restart. An invalid edit is logged and the last valid
configuration remains active.

## Zellij discovery

`zellij.terminal_app_ids` is an exact allowlist of the Wayland app IDs that
may contain Zellij. `zellij.session_title_separator` splits the window title at
its first occurrence; the part before it is treated as the session name. A
title without the separator is treated as the complete session name. In both
cases a same-named Zellij session socket must exist before a bridge is opened.

The defaults contain the tested terminals' normal Wayland app IDs. Another
terminal can be used when niri reports a stable app ID and its title preserves
the Zellij session name. Inspect both with `niri msg --json windows` and add the
app ID to the allowlist. Valid configuration changes are applied without a
service restart. Discovery configuration selects windows and derives session
names; it does not launch or reconfigure the terminal.

The installer preserves an existing config. An installation upgraded from 0.2
therefore keeps its previous Ghostty-only allowlist until the additional app IDs
above are added explicitly.

Terminal-specific settings can defeat discovery. Do not set Ghostty's `title`,
Alacritty's `window.dynamic_title = false`, Kitty's `--title` or
`os_window_title`, Foot's `locked-title`, or a WezTerm `format-window-title`
hook that removes the active pane title. `niri-zvim doctor` reports definite
conflicts it finds in installed Ghostty, Alacritty, and Foot config files. It
stays silent when a terminal or config file is absent.

Several terminal windows may attach to one Zellij session. The daemon pairs
Zellij client IDs with Niri window IDs in creation order and targets the
background plugin instance for the focused window. Each attached surface must
therefore retain the session-bearing title described above; check status to
see the resulting client-to-window bindings.

An SSH-attached client cannot use same-machine session-socket discovery. The
explicit `niri-zvim-zellij-bridge` command connects the remote plugin to the
Niri-side daemon through an OpenSSH-forwarded Unix socket without adding SSH or
discovery work to normal key handling. See
[Zellij navigation over SSH](https://github.com/lindestad/niri-zvim/blob/main/docs/ssh.md)
for setup, performance properties, and current limits.

## Status

Query the running daemon and its current graph with:

    niri-zvim status
    niri-zvim status --json

Status reports the daemon and protocol versions, uptime, active Niri mode,
socket, current Niri focus, navigation sequence, pending commands, and every
known Zellij client and Neovim instance. A disconnected entry means topology
remains in the graph but its executor is no longer attached; this is useful
diagnostic state rather than a successful health check.

## Doctor

Diagnose the complete installation without changing it:

    niri-zvim doctor
    niri-zvim doctor --json

Doctor validates the config and exact supported niri, Zellij, and Neovim
versions; checks the Niri and private daemon sockets; queries the running daemon
and systemd user services; verifies the Zellij WASM and Neovim runtime files;
reports whether the configured plugin has its four required Zellij permissions;
and warns about installed terminal settings that definitely suppress dynamic
titles. Missing terminal binaries and config files are not findings. Failed
required checks produce a non-zero exit status. Missing Zellij approval is a
warning because approving the normal first-use prompt can complete that step
without reinstalling.

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

The exhaustive live scenarios launch disposable Ghostty windows with an
isolated test config. Optional portability scenarios launch one disposable
Alacritty, Kitty, Foot, and WezTerm Zellij client apiece; a missing optional
terminal is reported as `SKIP`, not a failure. Immediately before the suite
takes desktop control, it shows a small two-second Ghostty warning and restores
the previously focused window when the warning closes.
Disposable Zellij sessions use an isolated permission cache that grants the
repository's plugin only the four permissions documented above; the user's
Zellij permission cache is not read or changed. Their session metadata remains
visible to the daemon so the test exercises its normal topology refresh path.
It traverses empty terminals, a Niri tabbed column, direct Neovim instances,
one- and three-pane Zellij sessions, two clients attached to one session,
nested Neovim windows inside Zellij panes, and three temporary workspaces in
both directions. Do not interact with the desktop until it finishes.
Unexpected focus on a window outside the test is
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
configuration. The disposable Zellij cache also suppresses release notes so a
first-run plugin pane cannot masquerade as a terminal client. Run one case in
isolation with, for example, `scripts/test-live tabs`, or run only the optional
terminal matrix with `scripts/test-live terminals`.

`scripts/test-live` handles preflight checks and orchestration. Shared fixture
helpers and the individual scenario files live under `scripts/live-tests/`.

The deterministic benchmark suite measures size-parameterized routing,
topology construction, Unix socket dispatch, and the short-lived keypress
client. `just bench-live` separately compares native and daemon-triggered Niri,
Zellij, and Neovim focus convergence on the running desktop, with an optional
pinned `vim-niri-nav` comparison. See
[the benchmarking documentation](https://github.com/lindestad/niri-zvim/blob/main/docs/benchmarking.md)
for methodology and interpretation limits.

See
[the changelog](https://github.com/lindestad/niri-zvim/blob/main/CHANGELOG.md)
for release notes.

## Release process

Finalize the version heading in `CHANGELOG.md`, run `just test`, and validate
the tag metadata with `just release-check vX.Y.Z`. After explicit release
approval, create and push the matching annotated tag. The tag workflow repeats
the complete nonvisual checks, builds and self-tests a host-targeted release
archive, and writes its SHA-256 file before publishing anything. It then uses
crates.io trusted publishing to release `niri-zvim-core` followed by
`niri-zvim`; the Zellij WASM crate remains bundle-only. The GitHub release is
created from the matching changelog section only after both crates succeed.
