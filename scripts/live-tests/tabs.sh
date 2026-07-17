# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_niri_tabbed_column() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-tabs"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local plain_pid="$runtime_dir/$prefix-plain-tab.pid"
  local right_pid="$runtime_dir/$prefix-right.pid"
  local nvim_socket="$runtime_dir/$prefix-nvim-tab.sock"
  terminal_pid_files+=("$left_pid" "$plain_pid" "$right_pid")
  nvim_sockets+=("$nvim_socket")

  focus_fresh_workspace
  local left plain_tab nvim_tab right
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"

  launch_terminal "$prefix-plain-tab" "$plain_pid"
  plain_tab="$launched_window"
  test_windows+=("$plain_tab")
  move_window_column_last "$plain_tab"
  launch_nvim "$prefix-nvim-tab" "$nvim_socket" 2
  nvim_tab="$launched_window"
  test_windows+=("$nvim_tab")
  move_window_column_last "$nvim_tab"
  launch_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"

  consume_window_below "$plain_tab" "$nvim_tab"
  mark_state "switching the middle Niri column to tabbed display"
  niri msg action set-column-display tabbed >/dev/null
  local -a nvim_ids
  mapfile -t nvim_ids < <(nvim_window_ids "$nvim_socket")
  focus_nvim_window "$nvim_socket" "${nvim_ids[0]}"
  focus_niri_window "$nvim_tab"
  expect_state "Neovim is the active tab" \
    "$nvim_tab" - - "$nvim_socket" "${nvim_ids[0]}"

  focus_niri_window "$left"
  navigate_burst_expect "rapid right enters the active tab and moves inside Neovim" right 2 \
    "$nvim_tab" - - "$nvim_socket" "${nvim_ids[1]}"
  navigate_expect "Neovim edge leaves the tabbed column" right \
    "$right" - - "$nvim_socket" "${nvim_ids[1]}"
  navigate_expect "left restores the active Neovim tab" left \
    "$nvim_tab" - - "$nvim_socket" "${nvim_ids[1]}"
  navigate_expect "left moves inside the restored Neovim tab" left \
    "$nvim_tab" - - "$nvim_socket" "${nvim_ids[0]}"
  navigate_expect "up selects the previous Niri tab" up \
    "$plain_tab" - - "$nvim_socket" "${nvim_ids[0]}"
  navigate_expect "down returns to the Neovim tab" down \
    "$nvim_tab" - - "$nvim_socket" "${nvim_ids[0]}"
)
