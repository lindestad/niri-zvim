# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031,SC2034,SC2154

test_terminal_navigation_smoke() (
  set -euo pipefail
  local terminal="$1"
  local app_id
  app_id="$(terminal_app_id "$terminal")"
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-terminal-$terminal"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local right_pid="$runtime_dir/$prefix-right.pid"
  local daemon_pid_file="$runtime_dir/$prefix-daemon.pid"
  local isolated_config="$runtime_dir/$prefix-config.json"
  local isolated_socket="$runtime_dir/$prefix.sock"
  local isolated_log="$runtime_dir/$prefix-daemon.log"
  local session="$prefix-session"
  local layout_file="$runtime_dir/$session.kdl"
  local layout_string='layout {
    pane split_direction="vertical" {
        pane focus=true
        pane
    }
}'
  terminal_pid_files+=("$left_pid" "$right_pid" "$daemon_pid_file")
  zellij_sessions+=("$session")
  case_files+=("$isolated_config" "$isolated_socket" "$isolated_log" "$layout_file")

  jq --arg app_id "$app_id" '
    .zellij = {
      terminal_app_ids: [$app_id],
      session_title_separator: " | "
    }
  ' "$daemon_config" >"$isolated_config"
  env \
    NIRI_ZVIM_CONFIG="$isolated_config" \
    NIRI_ZVIM_SOCKET="$isolated_socket" \
    NIRI_ZVIM_ZELLIJ_PLUGIN="$zellij_plugin" \
    RUST_LOG=info \
    "$HOME/.local/bin/niri-zvimd" >"$isolated_log" 2>&1 &
  printf '%s\n' "$!" >"$daemon_pid_file"
  wait_for_socket "$isolated_socket"
  export NIRI_ZVIM_SOCKET="$isolated_socket"

  focus_fresh_workspace
  local left terminal_window right actual_app_id
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"

  printf '%s\n' "$layout_string" >"$layout_file"
  mark_state "launching $terminal Zellij session $session"
  launch_test_terminal "$terminal" env XDG_CACHE_HOME="$zellij_cache_home" \
    zellij --session "$session" --new-session-with-layout "$layout_file"
  wait_for_zellij_session "$session"
  for _ in {1..150}; do
    terminal_window="$(niri msg --json windows | jq -r \
      --arg app_id "$app_id" --arg title "$session" '
        .[] | select(.app_id == $app_id and (.title | contains($title))) | .id
      ' | tail -1)"
    [[ -n "$terminal_window" ]] && break
    sleep 0.05
  done
  [[ -n "$terminal_window" ]] || {
    echo "timed out waiting for $terminal window with app ID $app_id and title $session" >&2
    return 1
  }
  test_windows+=("$terminal_window")
  wait_for_zellij_count "$session" 2
  move_window_column_last "$terminal_window"
  actual_app_id="$(niri msg --json windows | jq -r --argjson id "$terminal_window" \
    '.[] | select(.id == $id) | .app_id')"
  [[ "$actual_app_id" == "$app_id" ]] || {
    echo "$terminal exposed app ID $actual_app_id, expected $app_id" >&2
    return 1
  }

  for _ in {1..150}; do
    if niri-zvim status --json 2>/dev/null | jq -e --arg session "$session" '
      any(.zellij[]; .session == $session and .connected)
    ' >/dev/null; then
      break
    fi
    sleep 0.02
  done
  niri-zvim status --json | jq -e --arg session "$session" '
    any(.zellij[]; .session == $session and .connected)
  ' >/dev/null || {
    echo "$terminal Zellij session was not discovered" >&2
    return 1
  }

  launch_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"

  local -a panes
  mapfile -t panes < <(zellij_pane_ids "$session")
  focus_niri_window "$left"
  navigate_expect "enter $terminal Zellij client" right \
    "$terminal_window" "$session" "${panes[0]}" - -
  navigate_expect "move inside $terminal Zellij client" right \
    "$terminal_window" "$session" "${panes[1]}" - -
  navigate_expect "leave $terminal Zellij client" right "$right" - - - -
  navigate_expect "re-enter $terminal Zellij client" left \
    "$terminal_window" "$session" "${panes[1]}" - -
  navigate_expect "move back inside $terminal Zellij client" left \
    "$terminal_window" "$session" "${panes[0]}" - -
  navigate_expect "leave $terminal Zellij client to the left" left "$left" - - - -
)
