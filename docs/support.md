# Support and compatibility

## Release status

Version 0.2 is under development. Version 0.1 is a developer preview of the
current architecture, not yet a general terminal-navigation package. It is
supported as a complete integration on the stack below. Other combinations may
work, but have not been validated and should not be presented as supported.

| Component | 0.1 support |
| --- | --- |
| Operating system | Linux in a running niri Wayland session |
| niri | 26.04, using `niri-ipc` 26.4.0 |
| Zellij | 0.44.3 |
| Neovim | 0.12.4 |
| Terminal for direct Neovim | Any terminal that delivers terminal-focus events to Neovim |
| Terminal for Zellij | Ghostty GTK 1.3 is tested; 0.2 accepts configured Wayland app IDs and title separators |
| Rust source build | Rust 1.97.1 or newer with `wasm32-wasip1` available through rustup |
| Service manager | systemd user services through the 0.1 installer |

This project intentionally follows the latest stable niri, Zellij, Neovim, and
Rust releases rather than maintaining a broad compatibility range. Rust 1.97.1
is the floor because it contains the fix for an LLVM miscompilation present in
earlier toolchains. The release versions above are rechecked when each
niri-zvim release is prepared.

The daemon only relies on niri IPC and a Unix socket. Systemd and Ghostty are
requirements of the current complete installation and discovery path, not of
the routing graph itself. The daemon and adapters require either the standard
`XDG_RUNTIME_DIR` environment or an explicit absolute `NIRI_ZVIM_SOCKET`;
they do not create a socket in the shared temporary directory.

`niri-zvim doctor` checks these runtime and installation assumptions directly;
`niri-zvim doctor --json` exposes the same pass, warning, and failure records to
scripts. It is read-only and does not grant plugin permissions or modify the
installation.

## Zellij discovery assumptions

The daemon currently associates a niri window with a Zellij session by
inference. All of the following must be true:

- niri reports a window app ID included in `zellij.terminal_app_ids`;
- niri reports a Wayland window title containing the session name, optionally
  followed by `zellij.session_title_separator` and terminal content;
- a same-named Zellij session socket exists;
- the session has one unambiguous connected terminal client;
- Zellij session metadata is available for the supported fallback refresh
  path; and
- the user has approved the plugin's requested Zellij permissions.

The defaults are Ghostty's `com.mitchellh.ghostty` app ID and Zellij's ` | `
title separator. These are compositor metadata, not requirements to show a
decorated title bar. Ghostty's window decorations may remain hidden. A static
custom terminal title, however, removes the session identity used by
discovery.

The plugin asks for `ReadApplicationState`, `ChangeApplicationState`,
`ReadCliPipes`, and `ReadSessionEnvironmentVariables`. It hides itself after
permission is granted. Normal navigation uses the persistent plugin pipe;
session metadata and Zellij CLI queries are reconciliation fallbacks rather
than work performed for every keypress.

One daemon bridge is currently created per session name and is bound to the
first matching niri window. Multiple Ghostty windows or multiple attached
terminal clients for the same Zellij session are therefore unsupported.
Renaming a running session after discovery is also unsupported.

## Neovim assumptions

The Neovim adapter supports normal windows in the current tab. Floating windows
are intentionally excluded from its directional graph.

For Neovim running directly in a terminal, `FocusGained` and `FocusLost` tell
the adapter whether it may claim the focused niri window. This makes direct
Neovim independent of Ghostty in principle, but the terminal and its settings
must deliver focus events.

Inside Zellij, the adapter uses `ZELLIJ_SESSION_NAME` and `ZELLIJ_PANE_ID` to
attach its window graph beneath the containing pane. This path inherits the
Zellij discovery restrictions above.

The 0.1 installer loads the adapter automatically for every Neovim instance
and retries the daemon socket while Neovim remains open. An explicit setup and
opt-out interface is planned for 0.2.

## Known unsupported configurations

- Vim rather than Neovim;
- terminals that do not expose a stable app ID and session-bearing title;
- more than one niri window attached to the same Zellij session;
- ambiguous multi-client Zellij sessions;
- renamed Zellij sessions; and
- non-systemd installation through the 0.1 installer.

Configurable terminal discovery is complete for 0.2. The next identity work
removes the one-window-per-session restriction.
