# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_consumed_zellij_resize_reflow() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-consumed-reflow"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
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
  terminal_pid_files+=("$left_pid")
  zellij_sessions+=("$upper_session" "$lower_session")

  focus_fresh_workspace
  local left upper lower
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"

  launch_zellij "$upper_session" 5 "$upper_layout"
  upper="$launched_window"
  test_windows+=("$upper")
  move_window_column_last "$upper"
  launch_zellij "$lower_session" 3 "$lower_layout"
  lower="$launched_window"
  test_windows+=("$lower")
  move_window_column_last "$lower"

  local upper_before lower_before upper_after lower_after
  upper_before="$(zellij_layout_signature "$upper_session")"
  lower_before="$(zellij_layout_signature "$lower_session")"
  consume_window_below "$upper" "$lower"
  upper_after="$(zellij_layout_signature "$upper_session")"
  lower_after="$(zellij_layout_signature "$lower_session")"
  [[ "$upper_after" != "$upper_before" && "$lower_after" != "$lower_before" ]] || {
    echo "Zellij did not report the expected resize reflow" >&2
    return 1
  }

  local upper_right lower_left
  upper_right="$(focused_zellij_pane "$upper_session")"
  lower_left="$(focused_zellij_pane "$lower_session")"
  expect_state "resized Zellij fixtures publish their reflowed focus" \
    "$upper" "$upper_session" "$upper_right" - -
  focus_niri_window "$lower"
  expect_state "start in resized lower Zellij" \
    "$lower" "$lower_session" "$lower_left" - -
  navigate_expect "move up after resize reaches upper Zellij" up \
    "$upper" "$upper_session" "$upper_right" - -
  navigate_expect_zellij_side "left follows reflowed upper topology" left \
    "$upper" "$upper_session" left
  navigate_expect_zellij_side "restore reflowed upper right pane" right \
    "$upper" "$upper_session" right

  focus_niri_window "$lower"
  mark_state "sending rapid up-left through the reflowed Zellij topology"
  niri-zvim up
  niri-zvim left
  sleep 1
  expect_zellij_side "rapid up-left uses the reflowed topology" \
    "$upper" "$upper_session" left
)
