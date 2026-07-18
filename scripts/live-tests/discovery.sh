# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_configured_terminal_discovery() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-discovery"
  local workspace="$prefix-workspace"
  local app_id="dev.niri-zvim.live-test"
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
    "$HOME/.local/bin/niri-zvimd" >"$isolated_log" 2>&1 &
  printf '%s\n' "$!" >"$daemon_pid_file"
  wait_for_socket "$isolated_socket"
  export NIRI_ZVIM_SOCKET="$isolated_socket"

  launch_ghostty_with_app_id() {
    local configured_app_id="$1"
    shift
    ghostty \
      --config-default-files=false \
      --config-file="$ghostty_config" \
      --class="$configured_app_id" \
      --gtk-single-instance=false \
      -e "$@" >/dev/null 2>&1 &
  }

  launch_discovery_terminal() {
    local title="$1"
    local pid_file="$2"
    launch_ghostty_with_app_id "$app_id" "$terminal_fixture" "$title" "$pid_file"
    wait_for_file "$pid_file"
    launched_window="$(wait_for_window "$title" exact)"
  }

  focus_fresh_workspace
  local left zellij_window right actual_app_id
  launch_discovery_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"

  printf '%s\n' "$layout_string" >"$layout_file"
  launch_ghostty_with_app_id "$app_id" env XDG_CACHE_HOME="$zellij_cache_home" \
    zellij --session "$session" --new-session-with-layout "$layout_file"
  wait_for_zellij_session "$session"
  zellij_window="$(wait_for_window "$session" contains)"
  test_windows+=("$zellij_window")
  wait_for_zellij_count "$session" 3
  move_window_column_last "$zellij_window"
  actual_app_id="$(niri msg --json windows | jq -r --argjson id "$zellij_window" \
    '.[] | select(.id == $id) | .app_id')"
  if [[ "$actual_app_id" != "$app_id" ]]; then
    test_fail "custom terminal exposes its configured app ID" "  "
    printf '    expected app_id=%s, actual app_id=%s\n' "$app_id" "$actual_app_id" >&2
    return 1
  fi
  test_pass "custom terminal exposes its configured app ID" "  "

  for _ in {1..150}; do
    if niri-zvim status --json 2>/dev/null | jq -e --arg session "$session" \
      '.zellij[] | select(.session == $session and .connected)' >/dev/null; then
      break
    fi
    sleep 0.02
  done
  niri-zvim status --json | jq -e --arg session "$session" \
    '.zellij[] | select(.session == $session and .connected)' >/dev/null || {
    test_fail "custom discovery connects the isolated Zellij bridge" "  "
    return 1
  }
  test_pass "custom discovery connects the isolated Zellij bridge" "  "

  launch_discovery_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"

  local -a panes
  mapfile -t panes < <(zellij_pane_ids "$session")
  focus_niri_window "$left"
  navigate_expect "custom discovery enters Zellij" right \
    "$zellij_window" "$session" "${panes[0]}" - -
  navigate_expect "custom discovery moves inside Zellij" right \
    "$zellij_window" "$session" "${panes[1]}" - -
  navigate_burst_expect "custom discovery crosses the remaining graph" right 2 \
    "$right" "$session" "${panes[2]}" - -
)
