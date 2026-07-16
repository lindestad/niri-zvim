# Architecture

The daemon owns the navigation graph. Adapters publish topology; they do not
need to be queried during normal key handling.

    niri event stream -----------------+
    Zellij plugin <-> persistent pipe -+-> niri-zvimd <- one-byte nav client
    Neovim Lua <-> Unix socket --------+        |
                                               +-> exactly one executor

For a focused Niri window containing Zellij and Neovim, routing checks the
cached graph from the inside out:

1. Move Neovim if its focused split has a directional neighbor.
2. Otherwise move Zellij if its focused pane has a directional neighbor.
3. Otherwise ask niri to move focus.

Every command has a predicted transition. The daemon applies that transition
before dispatch, permitting key-repeat to remain pipelined. Adapter snapshots
carry monotonically increasing revisions. A snapshot must advance beyond the
current revision to replace it; an equal-revision observation may have been
captured before the predicted command and cannot rewind that prediction.
Neovim and Zellij snapshots also report the latest daemon command sequence
they have applied. An acknowledging snapshot becomes the authoritative base,
and predictions for any later pending commands are replayed on top of it. If
an adapter event is coalesced, an observation matching a stored expected target
implicitly acknowledges the corresponding prefix of the pending queue.

This means acknowledgements are not a queue barrier. If two requests arrive
before the first focus event, the second request is routed against the first
request's predicted state. Niri focus events and explicitly acknowledged
adapter snapshots are authoritative observations; later pending predictions
are replayed on top of them.

The keypress path performs no process discovery and invokes no Zellij or
Neovim command-line client. It is one short Unix-socket write followed by an
in-memory graph transition. The daemon keeps a persistent Niri action socket,
a persistent pipe per observed Zellij session, and a persistent socket per
Neovim instance.

## Niri modes

Named modes map each direction to a Niri IPC action. The internal default is
workspace-local (`FocusColumnLeft/Right` and `FocusWindowUp/Down`). The
installed desktop mode uses `FocusColumnOrMonitorLeft/Right` and
`FocusWindowOrWorkspaceUp/Down`, matching a multi-monitor vertical-workspace
layout. These actions are the final fallthrough after Neovim and Zellij have
no neighbor; they do not alter nested routing.

## Live transition tests

The opt-in `scripts/test-live` harness launches disposable Ghostty surfaces for
direct Neovim and Zellij. Each scenario records the initial nested focus,
sends the normal one-byte navigation client message, queries authoritative
Neovim RPC or Zellij JSON state, and compares the resulting nested and Niri
focus with the expected transition. Burst scenarios also send complete direct,
Zellij, and nested paths without waiting between commands, then verify every
layer after one final convergence wait. Pure graph tests cover the same routing
invariants without requiring a compositor. See [live-testing.md](live-testing.md)
for fixture design, race regressions, and diagnostics.

## Identity

Niri windows use compositor window IDs. Zellij clients use session name plus
client ID. Neovim instances use a generated token and declare either a Niri
window or a Zellij client/pane as their parent. A direct Neovim instance may
claim the focused Niri window only while its terminal reports `FocusGained`;
this keeps multiple Ghostty surfaces distinct even though GTK single-instance
mode gives them the same process ID. No process-tree inference is used.
