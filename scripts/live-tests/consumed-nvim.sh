# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_consumed_nvim_columns() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-consumed-nvim"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local upper_socket="$runtime_dir/$prefix-upper.sock"
  local nested_socket="$runtime_dir/$prefix-nested.sock"
  local session="$prefix-session"
  local nvim_layout='set splitright splitbelow | vsplit | wincmd h | split | split | wincmd l'
  local quoted_nvim_fixture quoted_socket quoted_command zellij_layout
  quoted_nvim_fixture="$(jq -Rn --arg value "$nvim_fixture" '$value')"
  quoted_socket="$(jq -Rn --arg value "$nested_socket" '$value')"
  quoted_command="$(jq -Rn --arg value "$nvim_layout" '$value')"
  printf -v zellij_layout 'layout {\n    pane split_direction="vertical" {\n        pane\n        pane command="nvim" close_on_exit=true focus=true {\n            args "-u" %s "--noplugin" "--listen" %s "-c" %s\n        }\n    }\n}' \
    "$quoted_nvim_fixture" "$quoted_socket" "$quoted_command"
  terminal_pid_files+=("$left_pid")
  nvim_sockets+=("$upper_socket" "$nested_socket")
  zellij_sessions+=("$session")

  focus_fresh_workspace
  local left upper lower
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"

  local title="$prefix-upper"
  mark_state "launching irregular direct Neovim $title"
  launch_ghostty nvim -u "$nvim_fixture" --noplugin --listen "$upper_socket" \
    -c "set title titlestring=$title | $nvim_layout"
  wait_for_socket "$upper_socket"
  wait_for_nvim_count "$upper_socket" 4
  upper="$(wait_for_window "$title" exact)"
  test_windows+=("$upper")
  move_window_column_last "$upper"

  launch_zellij "$session" 2 "$zellij_layout"
  lower="$launched_window"
  test_windows+=("$lower")
  move_window_column_last "$lower"
  wait_for_socket "$nested_socket"
  wait_for_nvim_count "$nested_socket" 4
  consume_window_below "$upper" "$lower"

  local -a upper_ids nested_ids panes
  mapfile -t upper_ids < <(nvim_window_ids "$upper_socket")
  mapfile -t nested_ids < <(nvim_window_ids "$nested_socket")
  mapfile -t panes < <(zellij_pane_ids "$session")
  local upper_right="${upper_ids[3]}"
  local nested_left="${nested_ids[0]}"
  local nested_right="${nested_ids[3]}"
  local nested_pane="${panes[1]}"
  focus_nvim_window "$upper_socket" "$upper_right"
  focus_nvim_window "$nested_socket" "$nested_right"
  focus_niri_window "$lower"

  navigate_expect "move up from nested Neovim to direct Neovim" up \
    "$upper" - - "$upper_socket" "$upper_right"
  navigate_expect_nvim_changed "left stays inside irregular direct Neovim" left \
    "$upper" "$upper_socket" "$upper_right"
  focus_nvim_window "$upper_socket" "$upper_right"
  navigate_expect "move down into nested Neovim" down \
    "$lower" "$session" "$nested_pane" "$nested_socket" "$nested_right"
  navigate_expect_nvim_changed "left stays inside Neovim nested in Zellij" left \
    "$lower" "$nested_socket" "$nested_right" "$session" "$nested_pane"
  focus_nvim_window "$nested_socket" "$nested_left"
  navigate_expect "nested Neovim edge falls through to Zellij pane" left \
    "$lower" "$session" "${panes[0]}" "$nested_socket" "$nested_left"
  navigate_expect "nested Zellij edge falls through to adjacent Niri column" left \
    "$left" - - - -
)
