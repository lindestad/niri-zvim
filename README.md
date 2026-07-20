# niri-zvim

[![GitHub release](https://img.shields.io/github/v/release/lindestad/niri-zvim)](https://github.com/lindestad/niri-zvim/releases)
[![crates.io](https://img.shields.io/crates/v/niri-zvim.svg)](https://crates.io/crates/niri-zvim)
[![CI](https://github.com/lindestad/niri-zvim/actions/workflows/ci.yml/badge.svg)](https://github.com/lindestad/niri-zvim/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/github/license/lindestad/niri-zvim)](https://github.com/lindestad/niri-zvim/blob/main/LICENSE)

One set of directional keys for navigating Neovim splits, Zellij panes, and
niri windows, monitors, and workspaces.

> **Demo GIF placeholder:** hold a navigation key while focus moves from a
> Neovim split, through its containing Zellij panes, and out into niri.

## What it does

```text
Mod+H/J/K/L (or arrow keys)
              │
              ▼
       Neovim has a split? ── yes ──▶ move in Neovim
              │ no
              ▼
       Zellij has a pane?  ── yes ──▶ move in Zellij
              │ no
              ▼
       use the configured niri window/monitor/workspace action
```

`niri-zvimd` keeps that complete focus graph in memory. Neovim and Zellij push
topology changes to it, and a Niri keybind sends a three-byte navigation
request. Nothing waits for an editor response before deciding where to move,
so held and repeated keys remain pipelined.

- Navigates direct and Zellij-nested Neovim instances.
- Understands Niri tabbed columns and Zellij tabs and panes.
- Supports multiple terminal clients attached to one Zellij session.
- Live-tests Zellij with Ghostty, Alacritty, Kitty, Foot, and WezTerm.
- Includes `doctor`, `status`, configurable Niri fallthrough, and an explicit
  SSH bridge for remote Zellij sessions.

## Requirements

Version 0.3 is an enthusiast release that deliberately follows the current
stack rather than maintaining legacy compatibility.

| Component | Supported version or environment |
| --- | --- |
| Operating system | Linux in a running niri Wayland session |
| niri | 26.04 |
| Zellij | 0.44.3 |
| Neovim | 0.12.4 |
| Zellij terminal | Ghostty, Alacritty, Kitty, Foot/Footclient, or WezTerm |
| Service manager | systemd user services for the complete local install |
| Source build | Rust 1.97.1 or newer with `wasm32-wasip1` |

See [support and compatibility](https://github.com/lindestad/niri-zvim/blob/main/docs/support.md)
for the exact terminal and session-discovery assumptions.

## Quick start

### 1. Install

The recommended installer downloads the latest x86-64 Linux release, verifies
its checksum, and installs everything under your user account. It does not use
`sudo`.

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://raw.githubusercontent.com/lindestad/niri-zvim/main/install.sh | bash
```

To also replace the effective `Mod+H/J/K/L` bindings in your Niri config:

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://raw.githubusercontent.com/lindestad/niri-zvim/main/install.sh | \
  bash -s -- --replace-hjkl-binds
```

This option follows recursive Niri imports, changes the files that actually
own those four bindings, and preserves config and imported-file symlinks. It
validates the config before and after the change and keeps exact originals
under `~/.local/state/niri-zvim/backups`. If any of the four bindings cannot be
located, it changes nothing. It is opt-in because configuration files should
never be rewritten merely by installing a program.

Use `--no-service` to install files without enabling or restarting the user
service. This is useful on the server side of an SSH setup.

### 2. Add Niri bindings

Skip this step if you installed with `--replace-hjkl-binds`. Otherwise, add one
or both variants inside the `binds { ... }` section of
`~/.config/niri/config.kdl` or the file it imports for keybindings.

Vim-style bindings:

```kdl
Mod+H { spawn "niri-zvim" "left"; }
Mod+J { spawn "niri-zvim" "down"; }
Mod+K { spawn "niri-zvim" "up"; }
Mod+L { spawn "niri-zvim" "right"; }
```

Arrow-key bindings:

```kdl
Mod+Left  { spawn "niri-zvim" "left"; }
Mod+Down  { spawn "niri-zvim" "down"; }
Mod+Up    { spawn "niri-zvim" "up"; }
Mod+Right { spawn "niri-zvim" "right"; }
```

These are global compositor bindings. You do not need matching Neovim or
Zellij keymaps: each command routes to the deepest layer that can move and
falls through to Niri at the edge.

### 3. Enable Neovim

Add this to your Neovim configuration:

```lua
require("niri-zvim").setup()
```

The small explicit setup surface is:

```lua
require("niri-zvim").setup({
  enabled = true,
  socket_path = nil,          -- NIRI_ZVIM_SOCKET, then XDG_RUNTIME_DIR
  reconnect_interval_ms = 250,
})
```

`:NiriZvimEnable` and `:NiriZvimDisable` toggle the adapter at runtime.
Loading the installed runtime files without calling `setup()` does not connect
to the daemon.

### 4. Approve Zellij once

From any Zellij session, run:

```bash
zellij action launch-or-focus-plugin --floating \
  file:$HOME/.config/zellij/plugins/niri-zvim.wasm
```

Approve the four requested permissions. The plugin hides itself afterward;
the daemon starts the appropriate background instance automatically when it
discovers a Zellij terminal.

The terminal must keep Zellij's dynamic session-bearing window title. A
visible title bar is not required, but a fixed custom terminal title prevents
discovery.

### 5. Verify

```bash
niri-zvim doctor
niri-zvim status
```

`doctor` checks versions, sockets, the service, adapters, Zellij permissions,
terminal title conflicts, uninstall support, and whether Niri bindings are
installer-managed. `status` asks the daemon for its current Niri, Zellij, and
Neovim graph. Both commands also support JSON output.

## Niri navigation modes

Nested Neovim and Zellij routing is the same in every mode. The mode controls
only what happens after navigation reaches Niri.

| Mode | Left/right at an edge | Up/down at an edge | Intended layout |
| --- | --- | --- | --- |
| `default` | Stays within the current workspace | Stays within the current workspace | Conventional workspace-local navigation |
| `desktop` | Crosses to the adjacent monitor | Crosses to the adjacent vertical workspace | Multi-monitor desktop navigation |

The complete installer selects `desktop`. Without a config file, the daemon
uses `default`. Change `active_mode` in
`~/.config/niri-zvim/config.json`, then inspect the effective result with:

```bash
niri-zvim config check
niri-zvim config show
```

Valid configuration changes are applied without restarting the daemon. In
addition to modes, this file contains the exact Wayland app-ID allowlist and
title separator used for Zellij discovery.

## Installation choices

### Complete release bundle

The bootstrap command in Quick start is the normal installation path. The
release contains the native client and daemon, SSH bridge, Zellij WASM plugin,
Neovim adapter, default configuration, systemd user unit, and uninstaller. Rust
is not required.

### From source

Use this on another architecture or to install the current checkout:

```bash
git clone https://github.com/lindestad/niri-zvim.git
cd niri-zvim
./scripts/install
```

The source installer requires Rust 1.97.1 or newer and `rustup`; Just is only a
convenience wrapper. It accepts the same `--replace-hjkl-binds` and
`--no-service` options.

### Cargo

```bash
cargo install --locked niri-zvim
```

Cargo installs the native executables only. It cannot install the Zellij WASM
plugin, Neovim runtime, configuration, service, or uninstaller, so this is not
the recommended first-time setup.

## SSH-attached Zellij

An SSH client splits the topology across two machines: the Niri window and
daemon are local, while the Zellij session and plugin are remote. The separate
`niri-zvim-zellij-bridge` command joins them over a persistent OpenSSH Unix
socket forward without putting SSH or Zellij commands on the keypress path.

See [Zellij navigation over SSH](https://github.com/lindestad/niri-zvim/blob/main/docs/ssh.md)
for the complete setup and current Neovim limitation.

## Uninstall

```bash
niri-zvim uninstall
```

Uninstall always asks for confirmation. If the installer changed
`Mod+H/J/K/L` and those files are still unchanged, their exact originals are
restored before anything is removed. If a touched file changed afterward,
uninstall stops without removing the installation and prints the retained
backup location rather than overwriting newer user work.

The niri-zvim JSON config and binding backups are preserved. Use `--yes` for
an explicitly non-interactive uninstall and `--no-service` when no systemd user
service should be touched.

## Troubleshooting

- Start with `niri-zvim doctor`; failed required checks produce a non-zero exit
  status.
- Use `niri-zvim status` to confirm that the focused Niri window, Zellij client,
  and Neovim instance appear in the graph.
- If Neovim is absent, confirm that `require("niri-zvim").setup()` runs.
- If Zellij is absent, approve its permissions and remove fixed terminal-title
  settings. `doctor` detects definite conflicts for Ghostty, Alacritty, and
  Foot.
- Each local Zellij client needs its own Wayland toplevel. Terminal-native tabs
  and splits cannot be distinguished by Niri.

More detail lives in the
[support guide](https://github.com/lindestad/niri-zvim/blob/main/docs/support.md),
[architecture](https://github.com/lindestad/niri-zvim/blob/main/docs/architecture.md),
and [benchmark methodology](https://github.com/lindestad/niri-zvim/blob/main/docs/benchmarking.md).

## Development

```bash
just check       # formatting, tests, Clippy, ShellCheck, packages
just test        # automated checks plus live desktop scenarios
just bench       # deterministic routing and client benchmarks
just bench-live  # opt-in live compositor latency
```

The live suite warns before taking desktop control, uses disposable windows and
sessions, restores the previous focus, and treats unexpected user input as an
interrupted run. Its design and individual scenarios are documented in
[live testing](https://github.com/lindestad/niri-zvim/blob/main/docs/live-testing.md).

## License

[MIT](https://github.com/lindestad/niri-zvim/blob/main/LICENSE) © Daniel
Lindestad and niri-zvim contributors.
