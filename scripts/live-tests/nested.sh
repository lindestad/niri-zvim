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
  local quoted_nvim_fixture quoted_socket quoted_command layout_string
  quoted_nvim_fixture="$(jq -Rn --arg value "$nvim_fixture" '$value')"
  quoted_socket="$(jq -Rn --arg value "$socket" '$value')"
  quoted_command="$(jq -Rn --arg value 'set splitright | vsplit | vsplit' '$value')"
  printf -v layout_string 'layout {\n    pane split_direction="vertical" {\n        pane focus=true\n        pane command="nvim" close_on_exit=true {\n            args "-u" %s "--noplugin" "--listen" %s "-c" %s\n        }\n        pane\n    }\n}' \
    "$quoted_nvim_fixture" "$quoted_socket" "$quoted_command"
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
  launch_zellij "$session" 3 "$layout_string"
  zellij_window="$launched_window"
  test_windows+=("$zellij_window")
  move_window_column_last "$zellij_window"
  wait_for_socket "$socket"
  wait_for_nvim_count "$socket" 3
  launch_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"

  local -a panes nvim_ids
  mapfile -t panes < <(zellij_pane_ids "$session")
  mapfile -t nvim_ids < <(nvim_window_ids "$socket")
  focus_nvim_window "$socket" "${nvim_ids[0]}"
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

  navigate_burst_expect "rapid right burst crosses nested Neovim and Zellij" right 6 \
    "$right" "$session" "${panes[2]}" "$socket" "${nvim_ids[2]}"
  navigate_burst_expect "rapid left burst crosses nested Neovim and Zellij" left 6 \
    "$left" "$session" "${panes[0]}" "$socket" "${nvim_ids[0]}"
)
