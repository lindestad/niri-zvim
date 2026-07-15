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
carry monotonically increasing revisions and replace predictions whenever the
real state differs.

## Identity

Niri windows use compositor window IDs. Zellij clients use session name plus
client ID. Neovim instances use a generated token and declare either a Niri
window or a Zellij client/pane as their parent. No process-tree inference is
used.
