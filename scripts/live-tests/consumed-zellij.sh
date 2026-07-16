# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_consumed_zellij_columns() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-consumed-zellij"
  local workspace="$prefix-workspace"
  local below_workspace="$prefix-below-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local below_pid="$runtime_dir/$prefix-below.pid"
  local upper_session="$prefix-upper-session"
  local lower_session="$prefix-lower-session"
  local upper_layout='layout {
    pane split_direction="vertical" {
        pane split_direction="horizontal" {
            pane
            pane
            pane
            pane
        }
        pane focus=true
    }
}'
  local lower_layout='layout {
    pane split_direction="vertical" {
        pane split_direction="horizontal" {
            pane focus=true
            pane
        }
        pane
    }
}'
  terminal_pid_files+=("$left_pid" "$below_pid")
  zellij_sessions+=("$upper_session" "$lower_session")

  focus_fresh_workspace
  local left upper lower below
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"

  launch_zellij "$upper_session" 1
  upper="$launched_window"
  test_windows+=("$upper")
  move_window_column_last "$upper"
  launch_zellij "$lower_session" 1
  lower="$launched_window"
  test_windows+=("$lower")
  move_window_column_last "$lower"
  consume_window_below "$upper" "$lower"
  mark_state "applying dense Zellij layouts after Niri consumption"
  zellij --session "$upper_session" action override-layout \
    --layout-string "$upper_layout"
  zellij --session "$lower_session" action override-layout \
    --layout-string "$lower_layout"
  wait_for_zellij_count "$upper_session" 5
  wait_for_zellij_count "$lower_session" 3
  sleep 0.6

  local upper_right lower_left
  upper_right="$(zellij --session "$upper_session" action list-panes --all --json |
    jq -r '[.[] | select(.is_plugin | not)] | max_by(.pane_x) | .id')"
  lower_left="$(zellij --session "$lower_session" action list-panes --all --json |
    jq -r '[.[] | select((.is_plugin | not) and .pane_x == 0)] | min_by(.pane_y) | .id')"
  expect_state "consumed Zellij fixtures preserve their initial panes" \
    "$upper" "$upper_session" "$upper_right" - -

  focus_niri_window "$lower"
  expect_state "start in lower consumed Zellij" \
    "$lower" "$lower_session" "$lower_left" - -
  navigate_expect "move up to upper consumed Zellij" up \
    "$upper" "$upper_session" "$upper_right" - -
  navigate_expect_zellij_side "left stays inside upper consumed Zellij" left \
    "$upper" "$upper_session" left
  navigate_expect_zellij_side "right returns to upper Zellij edge" right \
    "$upper" "$upper_session" right
  navigate_expect_zellij_side "left changes upper Zellij focus history" left \
    "$upper" "$upper_session" left
  navigate_expect_zellij_side "right restores the upper Zellij edge" right \
    "$upper" "$upper_session" right
  focus_niri_window "$lower"
  expect_state "restore lower consumed Zellij without changing upper pane" \
    "$lower" "$lower_session" "$lower_left" - -
  navigate_expect "repeat up after vertical focus history" up \
    "$upper" "$upper_session" "$upper_right" - -
  navigate_expect_zellij_side "repeat left still targets upper Zellij" left \
    "$upper" "$upper_session" left
  navigate_expect_zellij_side "restore upper right pane before workspace hop" right \
    "$upper" "$upper_session" right

  focus_fresh_workspace
  launch_terminal "$prefix-below" "$below_pid"
  below="$launched_window"
  test_windows+=("$below")
  name_focused_workspace "$below_workspace"
  test_workspaces+=("$below_workspace")
  local workspace_idx below_idx
  workspace_idx="$(niri msg --json workspaces | jq -r --arg name "$workspace" \
    '.[] | select(.name == $name) | .idx')"
  below_idx="$(niri msg --json workspaces | jq -r --arg name "$below_workspace" \
    '.[] | select(.name == $name) | .idx')"
  [[ "$below_idx" -eq $((workspace_idx + 1)) ]] || {
    echo "consumed test workspaces are not consecutive: $workspace_idx, $below_idx" >&2
    return 1
  }
  navigate_expect "workspace up restores upper consumed Zellij" up \
    "$upper" "$upper_session" "$upper_right" - -
  navigate_expect_zellij_side "left after workspace hop stays in Zellij" left \
    "$upper" "$upper_session" left
)
