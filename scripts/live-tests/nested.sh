# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_nested_nvim_in_zellij() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-nested"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local right_pid="$runtime_dir/$prefix-right.pid"
  local session="$prefix-session"
  local socket="$runtime_dir/$prefix-nvim.sock"
  terminal_pid_files+=("$left_pid" "$right_pid")
  zellij_sessions+=("$session")
  nvim_sockets+=("$socket")

  focus_fresh_workspace
  local left zellij_window right
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"
  launch_zellij "$session"
  zellij_window="$launched_window"
  test_windows+=("$zellij_window")
  move_window_column_last "$zellij_window"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    zellij --session "$session" action new-pane --direction right --close-on-exit -- \
    nvim -u "$nvim_fixture" --noplugin --listen "$socket" \
    -c 'set splitright | vsplit | vsplit' >/dev/null
  wait_for_socket "$socket"
  wait_for_nvim_count "$socket" 3
  wait_for_zellij_count "$session" 2
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    zellij --session "$session" action new-pane --direction right >/dev/null
  wait_for_zellij_count "$session" 3
  launch_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"

  local -a panes nvim_ids
  mapfile -t panes < <(zellij_pane_ids "$session")
  mapfile -t nvim_ids < <(nvim_window_ids "$socket")
  focus_nvim_window "$socket" "${nvim_ids[0]}"
  focus_zellij_pane "$session" "${panes[0]}"
  focus_niri_window "$left"
  navigate_expect "enter nested Zellij at left shell" right \
    "$zellij_window" "$session" "${panes[0]}" - -
  navigate_expect "move from shell into Neovim pane" right \
    "$zellij_window" "$session" "${panes[1]}" "$socket" "${nvim_ids[0]}"
  navigate_expect "move to middle nested Neovim window" right \
    "$zellij_window" "$session" "${panes[1]}" "$socket" "${nvim_ids[1]}"
  navigate_expect "move to right nested Neovim window" right \
    "$zellij_window" "$session" "${panes[1]}" "$socket" "${nvim_ids[2]}"
  navigate_expect "leave Neovim for right Zellij shell" right \
    "$zellij_window" "$session" "${panes[2]}" - -
  navigate_expect "leave nested Zellij right" right "$right" - - - -
  navigate_expect "re-enter right Zellij shell" left \
    "$zellij_window" "$session" "${panes[2]}" - -
  navigate_expect "re-enter nested Neovim from right" left \
    "$zellij_window" "$session" "${panes[1]}" "$socket" "${nvim_ids[2]}"
  navigate_expect "move left in nested Neovim" left \
    "$zellij_window" "$session" "${panes[1]}" "$socket" "${nvim_ids[1]}"
  navigate_expect "reach left nested Neovim window" left \
    "$zellij_window" "$session" "${panes[1]}" "$socket" "${nvim_ids[0]}"
  navigate_expect "leave Neovim for left Zellij shell" left \
    "$zellij_window" "$session" "${panes[0]}" - -
  navigate_expect "leave nested Zellij left" left "$left" - - - -
)
