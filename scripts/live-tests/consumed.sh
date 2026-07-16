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
