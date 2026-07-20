# Zellij navigation over SSH

An SSH-attached Zellij client is split across two machines: its Niri window and
navigation daemon are on the SSH client, while the Zellij session, plugin, and
session socket are on the server. Installing the plugin on both machines does
not connect those two graphs.

`niri-zvim-zellij-bridge` runs on the SSH server and joins them through an
OpenSSH-forwarded Unix socket. It binds only the Zellij client that starts the
bridge to the currently focused Niri window. Other clients attached to the same
remote session keep independent bindings.

## Setup

Install the same niri-zvim build, including the Zellij plugin, on both
machines. The Niri-side daemon must be running on the SSH client. The SSH
server must permit Unix-socket forwarding.

Forward a server-side socket to the client daemon when opening SSH. Substitute
the actual runtime paths when the users have different numeric IDs:

```console
ssh -o ExitOnForwardFailure=yes -o StreamLocalBindUnlink=yes \
  -R /run/user/REMOTE_UID/niri-zvim-from-client.sock:/run/user/LOCAL_UID/niri-zvim.sock \
  server
```

Attach to Zellij on the server. While that terminal window is focused in Niri,
start the bridge inside the attached Zellij client:

```console
NIRI_ZVIM_SOCKET=/run/user/REMOTE_UID/niri-zvim-from-client.sock \
  niri-zvim-zellij-bridge
```

The bridge stays in the foreground so its lifetime and errors are visible. It
may instead run as a supervised or background process. It exits when the SSH
forward, daemon, Zellij session, or plugin pipe disappears. The daemon removes
the disconnected client's graph state; starting the command again establishes
a fresh binding.

`NIRI_ZVIM_SOCKET` is mandatory for this command. This prevents an SSH server
that also runs its own niri-zvim daemon from silently binding the remote
Zellij client to the wrong compositor.

On the Niri machine, `niri-zvim status` shows the forwarded client with a
session key beginning with `ssh:`. Normal local clients retain their ordinary
session names.

## Performance model

The status lookup, plugin targeting, and SSH setup happen once when the bridge
starts. The bridge is a separate executable so its async runtime and Zellij
control code are not linked into the latency-sensitive `niri-zvim` keypress
client. Navigation still takes the normal three-byte client write and cached
in-memory graph transition. Dispatch uses the existing bounded, nonblocking
adapter channel and the already-open SSH stream; it does not start `ssh`,
`zellij`, or a discovery process for a keypress. Native Zellij discovery and
dispatch are unchanged when no forwarded bridge is connected.

## Current limits

- One bridge process is required for each SSH-attached Zellij client.
- The correct Niri terminal must be focused when the bridge starts.
- A dropped SSH connection requires starting the bridge again after reconnect.
- This path currently forwards Zellij pane navigation. A Neovim instance
  nested inside the remote Zellij session is not attached to the Niri-side
  graph.
- The bridge is explicit rather than inferred from terminal process trees or
  SSH command lines.
