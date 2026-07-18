# Changelog

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
