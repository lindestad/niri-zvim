# Live testing notes

The live suite exists because the hardest failures happen between otherwise
correct components. Unit tests can prove graph routing, but they cannot prove
that Niri focus events, Zellij plugin snapshots, Neovim events, and optimistic
commands converge on the same state under real desktop timing.

`scripts/test-live` performs dependency and service checks and then runs the
scenario files in `scripts/live-tests/`. Every scenario gets a unique prefix,
its own processes, and named temporary workspaces. Cleanup terminates only
those processes, removes only those names, restores the original focus, and
checks that the user's original windows and workspaces did not change.
If focus moves to a window outside the test during an assertion, the harness
stops running further scenarios and reports the run as interrupted rather than
as a product failure.

## Two kinds of transition

Checkpoint transitions send one command, poll authoritative Niri, Zellij, and
Neovim state, and allow a short 100 ms settling period before the next command.
They make failures easy to locate and verify every edge in both directions.

Burst transitions deliberately provide no settling period between commands.
They send a complete path as fast as separate `niri-zvim` clients can connect,
wait one second only after the final command, and read each state once. The
direct test crosses two Neovim instances and three plain terminals. The
Zellij test crosses three panes between two terminals. The nested test crosses
two terminals, three Zellij panes, and three Neovim windows. The Niri-tabs test
enters the active Neovim tab and immediately moves to its second split. These
cases prove both halves of optimistic navigation:

1. The daemon routes each new command against its predicted graph before an
   acknowledgement arrives.
2. Every executor drains the dispatched commands in order and eventually
   reports the predicted state as authoritative.

Polling after every burst command would turn these into ordinary checkpoint
tests and conceal the race they are intended to exercise.

## Problems found while building the suite

Niri's `tile_pos_in_workspace_view` is optional and may be absent even for a
normal tiled window. Building neighbors solely from that field produced an
empty compositor graph. Slow navigation appeared correct because every focus
event repaired the graph before the next keypress, while a burst predicted an
unknown focus after its first Niri move and incorrectly sent every remaining
command to Niri. Tiled Niri windows now use stable `(column, tile)` indices in
the same way as the configured actions: up/down follow tile order and
left/right enter the neighboring column.

Tabbed columns add an identity problem: Niri reports every member's column and
tile index, but does not identify the active member once the column loses
global focus. Predicting the first tile sends the next command to the wrong
layer when another tab contains Neovim or Zellij. The adapter therefore keeps
the last observed focused window for each column and uses it as the horizontal
target. The tabs regression sends `right,right` without a delay: the first move
enters the active Neovim tab and the second must stay in that tab and move to
the next split.

Neovim receives socket data outside the main editor event loop. Navigation is
therefore copied into a FIFO queue and drained in one scheduled callback. Each
move is computed from `nvim_get_current_win()` and a freshly built topology,
not from the last published snapshot. A final snapshot acknowledges the
batch. This prevents `WinEnter`, focus, and publish callbacks from making the
next queued move operate on stale state.

Adapter observations can complete out of order, particularly when Zellij CLI
refreshes overlap plugin events. A prediction reserves its next revision, so
an unacknowledged snapshot with that same revision is ambiguous and cannot
rewind predicted focus. Neovim and the Zellij plugin therefore publish the
latest daemon command sequence reflected by their state. The daemon replaces
the graph with that acknowledged state, removes the covered commands from its
pending queue, and replays any later predictions. Because Zellij may coalesce a
plugin pane event, each pending command also stores its expected target; a CLI
observation of that target implicitly acknowledges the matching queue prefix.
The full live suite caught this when a pre-command right-pane query completed
after the pane had already moved left and otherwise erased the expected nested
transition.

Zellij can report more than one `is_focused` pane when test panes are created
or focused through separate clients. That state is meaningful per client but
ambiguous for a test pretending there is one visible terminal. Assertions read
the visible client's pane from `zellij action list-clients` rather than choosing
the first session-wide focus flag.

Dense consumed-column fixtures need two construction styles. The exact
regression fixture launches one-pane sessions, lets Niri resize both Ghostty
windows into their final consumed column, and then applies complete Zellij
layouts atomically. This produces four panes stacked on the upper left beside
one full-height pane, and two panes stacked on the lower left beside another
full-height pane. It directly covers moving up from the lower left pane and
then left from the upper right pane.

The resize-reflow fixture intentionally does the opposite: it creates those
dense layouts while each Ghostty is full-height and then consumes the windows.
Zellij rearranges panes that no longer satisfy its minimum-size constraints.
The test verifies that the geometry signature changed, then exercises both
checkpointed navigation and a zero-delay `up,left` burst against the reported
post-resize topology. This prevents tests from passing only because their
fixture avoided real Zellij reflow.

Neovim uses the same edge-and-overlap topology rule as the Rust graph. A
full-height split beside a vertical stack has a left/right edge but no up/down
edge; center-based scoring previously trapped `up` inside nested Neovim instead
of falling through to the consumed Niri window above.

Ghostty is a systemd-managed single-instance application in the supported
setup. The suite opens surfaces with `ghostty +new-window` and supplies a
repository config with default config loading disabled. It never asks Niri to
force-close a Ghostty surface: that path can enter Ghostty's close-confirmation
handling and previously crashed the shared process. Cleanup exits the fixture
processes instead.

Test Neovim instances use `tests/fixtures/nvim.lua`, not the user's config.
This avoids plugins, file browsers, mappings, and asynchronous startup work
changing window topology or timing. Zellij and Ghostty are isolated in the
same spirit wherever their interfaces permit it.

## Deadlines and diagnostics

Each external operation has a three-second deadline and each complete scenario
has a 20-second safety cap. A failed case records its last checkpoint and
serializes the relevant Niri workspaces and windows, Zellij panes, Neovim
windows, and processes before cleanup. It also reports the start and current
1/5/15-minute load averages, online CPU count, and Linux CPU pressure averages.
High pressure does not excuse a wrong final state, but it
distinguishes a functional failure from a machine that could not meet a test
deadline.

Run all tests with `just test`, or isolate a live scenario while debugging:

    scripts/test-live direct
    scripts/test-live tabs
    scripts/test-live zellij-three
    scripts/test-live nested
    scripts/test-live consumed-zellij
    scripts/test-live consumed-reflow
    scripts/test-live consumed-nvim

Do not interact with the desktop until the final summary appears.
