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

Nested pane topology follows tiled-editor semantics: a candidate must be beyond
the requested edge and overlap on the perpendicular axis. Comparing centers
alone invents vertical neighbors between a full-height pane and a stack beside
it. Zellij can have several candidates along one shared edge and chooses the
most recently active one. The daemon uses a deterministic candidate for
prediction, while the plugin acknowledges any actual move away from the
command's origin so Zellij's MRU choice can authoritatively correct that
prediction.

Niri topology instead mirrors the actions the daemon actually sends. Up and
down follow tile order within a column; left and right enter the adjacent
column's last observed active window. The event adapter retains that active
window per column because Niri tabs share a column while the window snapshot
does not identify the active member of an unfocused tabbed column.

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

Executor reconciliation is deliberately outside the dispatch path. Niri uses
separate persistent action and snapshot sockets, coalescing snapshot requests
after a burst. Zellij plugin acknowledgements cancel a coalesced CLI fallback;
focus and topology metadata changes share that same debounced query worker.
Neovim and the Zellij plugin cache geometry-derived topology until structural
state changes. Benchmark boundaries and interpretation are specified in
[benchmarking.md](benchmarking.md).

Zellij focus is scoped to a connected client, not the session as a whole.
Session-wide pane metadata can mark several panes `is_focused` when diagnostic
or bootstrap clients have different histories. The plugin queries the pane for
its associated client directly. Background CLI snapshots use `list-clients`
and are accepted only when exactly one connected terminal client makes the
mapping unambiguous.

Pane resize and layout changes are normal authoritative topology updates.
Zellij's plugin emits `PaneUpdate`, and a metadata watcher also fingerprints
pane IDs, positions, dimensions, and visibility as a fallback. These snapshots
replace the predicted base and pending commands are replayed over the new
graph. A resize-only update cannot acknowledge navigation because focus did
not leave the command's origin pane.

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
