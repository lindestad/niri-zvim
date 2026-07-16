# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_zellij_single_pane() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-zellij-one"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local right_pid="$runtime_dir/$prefix-right.pid"
  local session="$prefix-session"
  terminal_pid_files+=("$left_pid" "$right_pid")
  zellij_sessions+=("$session")

  focus_fresh_workspace
  local left zellij_window right pane
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"
  launch_zellij "$session" 1
  zellij_window="$launched_window"
  test_windows+=("$zellij_window")
  move_window_column_last "$zellij_window"
  launch_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"
  pane="$(zellij_pane_ids "$session")"
  focus_niri_window "$left"

  navigate_expect "enter single-pane Zellij" right \
    "$zellij_window" "$session" "$pane" - -
  navigate_expect "single pane falls through right" right "$right" - - - -
  navigate_expect "re-enter single-pane Zellij" left \
    "$zellij_window" "$session" "$pane" - -
  navigate_expect "single pane falls through left" left "$left" - - - -
)
