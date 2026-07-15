# niri-zvim

State-synchronized directional navigation across niri windows, Zellij panes,
and Neovim splits.

niri-zvimd keeps a live graph of focus and topology. Neovim and a background
Zellij plugin push changes to it; the niri-zvim command sends one-byte
navigation requests from niri key bindings. A request is routed directly to
the deepest layer that has a neighbor in that direction, otherwise it falls
through to niri.

Focus changes are applied optimistically. Repeated requests route against the
predicted graph without waiting for acknowledgements; authoritative snapshots
then confirm or reconcile the prediction.

The project is under active development. See docs/architecture.md for the
protocol and invariants.
