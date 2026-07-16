# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_workspace_navigation() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-workspaces"
  local top_name="$prefix-top"
  local middle_name="$prefix-middle"
  local bottom_name="$prefix-bottom"
  local top_pid="$runtime_dir/$top_name.pid"
  local middle_pid="$runtime_dir/$middle_name.pid"
  local bottom_pid="$runtime_dir/$bottom_name.pid"
  terminal_pid_files+=("$top_pid" "$middle_pid" "$bottom_pid")
  test_workspaces+=("$top_name" "$middle_name" "$bottom_name")

  focus_fresh_workspace
  local top middle bottom
  launch_terminal "$top_name" "$top_pid"
  top="$launched_window"
  test_windows+=("$top")
  name_focused_workspace "$top_name"

  focus_fresh_workspace
  launch_terminal "$middle_name" "$middle_pid"
  middle="$launched_window"
  test_windows+=("$middle")
  name_focused_workspace "$middle_name"

  focus_fresh_workspace
  launch_terminal "$bottom_name" "$bottom_pid"
  bottom="$launched_window"
  test_windows+=("$bottom")
  name_focused_workspace "$bottom_name"

  local top_idx middle_idx bottom_idx
  top_idx="$(niri msg --json workspaces | jq -r --arg name "$top_name" '.[] | select(.name == $name) | .idx')"
  middle_idx="$(niri msg --json workspaces | jq -r --arg name "$middle_name" '.[] | select(.name == $name) | .idx')"
  bottom_idx="$(niri msg --json workspaces | jq -r --arg name "$bottom_name" '.[] | select(.name == $name) | .idx')"
  [[ "$middle_idx" -eq $((top_idx + 1)) && "$bottom_idx" -eq $((middle_idx + 1)) ]] || {
    echo "test workspaces are not consecutive: $top_idx, $middle_idx, $bottom_idx" >&2
    return 1
  }

  focus_niri_window "$top"
  expect_state "start on top test workspace" "$top" - - - -
  navigate_expect "workspace-aware down reaches middle" down "$middle" - - - -
  navigate_expect "workspace-aware down reaches bottom" down "$bottom" - - - -
  navigate_expect "workspace-aware up returns to middle" up "$middle" - - - -
  navigate_expect "workspace-aware up returns to top" up "$top" - - - -
)
