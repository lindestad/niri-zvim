# Changelog

## 0.2.0

- Version the navigation and adapter wire protocols and reject mismatched
  clients instead of interpreting stale message formats.
- Add human-readable and JSON daemon status with live graph, connection, focus,
  and pending-navigation state.
- Add a read-only doctor command for config, dependency versions, services,
  sockets, adapter files, and Zellij permission approval.
- Make Zellij terminal app IDs and session-title separators configurable, with
  an isolated live test that proves a non-default app ID drives discovery.
- Support several terminal clients attached to one Zellij session with
  client-targeted plugin messages and bidirectional live traversal.
- Make Neovim integration explicitly opt-in through a validated, idempotent
  `setup()` interface with socket/retry options and complete runtime teardown.
- Add `config check` and `config show` commands for validating and inspecting
  the complete effective configuration.
- Reload valid navigation and discovery configuration changes at runtime while
  retaining the last valid configuration after a rejected edit.
- Install an owned Neovim adapter copy independent of the source checkout,
  support prebuilt release payloads, and stop changing Ghostty service state.
- Build, checksum, install-test, and publish complete GitHub release archives
  from finalized version tags while keeping crates.io publication explicit.

## 0.1.1

- Treat adapter focus outside a published topology as an edge instead of
  panicking, including when a Neovim floating window is focused.
- Include the MIT license in both published crates and keep repository-only
  tests and relative documentation links out of their source archives.
- Verify the contents and tests of unpacked crate archives during release
  checks.
- Refuse to replace real Neovim runtime paths with installer symlinks.
- Require a private runtime directory or explicit absolute socket path, set
  user-only socket permissions, and bound connection, frame, and outgoing
  queue resources.

## 0.1.0

Initial developer-preview release.

- Route directional focus across niri columns and tiles, Zellij panes, and
  Neovim windows through one cached navigation graph.
- Apply optimistic transitions so repeated input does not wait for compositor,
  multiplexer, or editor acknowledgements.
- Reconcile predictions against revisioned niri, Zellij, and Neovim state.
- Support direct Neovim, Zellij, nested Neovim inside Zellij, niri tabs,
  floating and tiled Zellij layers, and configurable niri fallthrough actions.
- Add deterministic routing and dispatch benchmarks plus opt-in native desktop
  transition and comparative latency suites.
- Provide a source installer for the documented niri, Ghostty, Zellij,
  Neovim, and systemd environment.
