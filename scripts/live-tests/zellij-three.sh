# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_zellij_three_panes() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-zellij-three"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local right_pid="$runtime_dir/$prefix-right.pid"
  local session="$prefix-session"
  local layout_string='layout {
    pane split_direction="vertical" {
        pane focus=true
        pane
        pane
    }
}'
  terminal_pid_files+=("$left_pid" "$right_pid")
  zellij_sessions+=("$session")

  focus_fresh_workspace
  local left zellij_window right
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"
  launch_zellij "$session" 3 "$layout_string"
  zellij_window="$launched_window"
  test_windows+=("$zellij_window")
  move_window_column_last "$zellij_window"
  launch_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"

  local -a panes
  mapfile -t panes < <(zellij_pane_ids "$session")
  focus_niri_window "$left"
  navigate_expect "enter left Zellij pane" right \
    "$zellij_window" "$session" "${panes[0]}" - -
  navigate_expect "move to middle Zellij pane" right \
    "$zellij_window" "$session" "${panes[1]}" - -
  navigate_expect "move to right Zellij pane" right \
    "$zellij_window" "$session" "${panes[2]}" - -
  navigate_expect "leave three-pane Zellij right" right "$right" - - - -
  navigate_expect "re-enter right Zellij pane" left \
    "$zellij_window" "$session" "${panes[2]}" - -
  navigate_expect "move back to middle Zellij pane" left \
    "$zellij_window" "$session" "${panes[1]}" - -
  navigate_expect "move back to left Zellij pane" left \
    "$zellij_window" "$session" "${panes[0]}" - -
  navigate_expect "leave three-pane Zellij left" left "$left" - - - -
)
