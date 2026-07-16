# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_direct_terminal_matrix() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-direct"
  local workspace="$prefix-workspace"
  local empty_left_pid="$runtime_dir/$prefix-empty-left.pid"
  local empty_middle_pid="$runtime_dir/$prefix-empty-middle.pid"
  local empty_right_pid="$runtime_dir/$prefix-empty-right.pid"
  local nvim_two_socket="$runtime_dir/$prefix-nvim-two.sock"
  local nvim_three_socket="$runtime_dir/$prefix-nvim-three.sock"
  terminal_pid_files+=("$empty_left_pid" "$empty_middle_pid" "$empty_right_pid")
  nvim_sockets+=("$nvim_two_socket" "$nvim_three_socket")

  focus_fresh_workspace
  local empty_left nvim_two empty_middle nvim_three empty_right
  launch_terminal "$prefix-empty-left" "$empty_left_pid"
  empty_left="$launched_window"
  test_windows+=("$empty_left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$empty_left"

  launch_nvim "$prefix-nvim-two" "$nvim_two_socket" 2
  nvim_two="$launched_window"
  test_windows+=("$nvim_two")
  move_window_column_last "$nvim_two"
  launch_terminal "$prefix-empty-middle" "$empty_middle_pid"
  empty_middle="$launched_window"
  test_windows+=("$empty_middle")
  move_window_column_last "$empty_middle"
  launch_nvim "$prefix-nvim-three" "$nvim_three_socket" 3
  nvim_three="$launched_window"
  test_windows+=("$nvim_three")
  move_window_column_last "$nvim_three"
  launch_terminal "$prefix-empty-right" "$empty_right_pid"
  empty_right="$launched_window"
  test_windows+=("$empty_right")
  move_window_column_last "$empty_right"

  local -a nvim_two_ids nvim_three_ids
  mapfile -t nvim_two_ids < <(nvim_window_ids "$nvim_two_socket")
  mapfile -t nvim_three_ids < <(nvim_window_ids "$nvim_three_socket")
  focus_nvim_window "$nvim_two_socket" "${nvim_two_ids[0]}"
  focus_nvim_window "$nvim_three_socket" "${nvim_three_ids[0]}"
  focus_niri_window "$empty_left"
  expect_state "start in left empty terminal" "$empty_left" - - - -

  navigate_expect "enter two-window Neovim" right \
    "$nvim_two" - - "$nvim_two_socket" "${nvim_two_ids[0]}"
  navigate_expect "move inside two-window Neovim" right \
    "$nvim_two" - - "$nvim_two_socket" "${nvim_two_ids[1]}"
  navigate_expect "leave Neovim for middle empty terminal" right \
    "$empty_middle" - - - -
  navigate_expect "enter three-window Neovim" right \
    "$nvim_three" - - "$nvim_three_socket" "${nvim_three_ids[0]}"
  navigate_expect "move to middle Neovim window" right \
    "$nvim_three" - - "$nvim_three_socket" "${nvim_three_ids[1]}"
  navigate_expect "move to right Neovim window" right \
    "$nvim_three" - - "$nvim_three_socket" "${nvim_three_ids[2]}"
  navigate_expect "leave Neovim for right empty terminal" right \
    "$empty_right" - - - -
  navigate_expect "re-enter three-window Neovim from right" left \
    "$nvim_three" - - "$nvim_three_socket" "${nvim_three_ids[2]}"
  navigate_expect "move left inside three-window Neovim" left \
    "$nvim_three" - - "$nvim_three_socket" "${nvim_three_ids[1]}"
  navigate_expect "reach left Neovim window" left \
    "$nvim_three" - - "$nvim_three_socket" "${nvim_three_ids[0]}"
  navigate_expect "leave for middle empty terminal" left \
    "$empty_middle" - - - -
  navigate_expect "re-enter two-window Neovim from right" left \
    "$nvim_two" - - "$nvim_two_socket" "${nvim_two_ids[1]}"
  navigate_expect "move left inside two-window Neovim" left \
    "$nvim_two" - - "$nvim_two_socket" "${nvim_two_ids[0]}"
  navigate_expect "return to left empty terminal" left \
    "$empty_left" - - - -
)
