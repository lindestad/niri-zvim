# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

zellij_client_pane() {
  local session="$1"
  local client_id="$2"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    zellij --session "$session" action list-clients 2>/dev/null |
    awk -v client_id="$client_id" 'NR > 1 && $1 == client_id && $2 ~ /^terminal_[0-9]+$/ {
      sub(/^terminal_/, "", $2)
      print $2
      exit
    }'
}

wait_for_zellij_clients() {
  local session="$1"
  local expected="$2"
  local count
  for _ in {1..150}; do
    count="$(zellij --session "$session" action list-clients 2>/dev/null |
      awk 'NR > 1 && $2 ~ /^terminal_[0-9]+$/ { count++ } END { print count + 0 }')"
    [[ "$count" == "$expected" ]] && return 0
    sleep 0.05
  done
  echo "Zellij did not expose $expected clients in $session" >&2
  return 1
}

expect_zellij_client_state() {
  local label="$1"
  local expected_niri="$2"
  local session="$3"
  local client_id="$4"
  local expected_pane="$5"
  local actual_niri actual_pane
  for _ in {1..150}; do
    actual_niri="$(niri msg --json focused-window | jq -r '.id // empty')"
    actual_pane="$(zellij_client_pane "$session" "$client_id")"
    if [[ "$actual_niri" == "$expected_niri" && "$actual_pane" == "$expected_pane" ]]; then
      test_pass "$label" "  "
      return 0
    fi
    if abort_on_external_focus "$expected_niri" "$actual_niri"; then
      return 1
    fi
    sleep 0.02
  done
  test_fail "$label" "  "
  printf '    expected niri=%s client=%s pane=%s\n' \
    "$expected_niri" "$client_id" "$expected_pane" >&2
  printf '    actual   niri=%s client=%s pane=%s\n' \
    "$actual_niri" "$client_id" "$actual_pane" >&2
  return 1
}

navigate_expect_zellij_client() {
  local label="$1"
  local direction="$2"
  shift 2
  mark_state "navigating $direction: $label"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    niri-zvim "$direction"
  expect_zellij_client_state "$label" "$@"
  sleep 0.1
  mark_state "completed: $label"
}

assert_zellij_client_pane_now() {
  local label="$1"
  local session="$2"
  local client_id="$3"
  local expected_pane="$4"
  local actual_pane
  actual_pane="$(zellij_client_pane "$session" "$client_id")"
  if [[ "$actual_pane" == "$expected_pane" ]]; then
    test_pass "$label" "  "
    return 0
  fi
  test_fail "$label (expected client=$client_id pane=$expected_pane, actual pane=$actual_pane)" "  "
  return 1
}

test_zellij_multiple_clients() (
  set -euo pipefail
  local -a test_windows nvim_sockets zellij_sessions terminal_pid_files test_workspaces
  begin_case
  local prefix="niri-zvim-live-$tag-zellij-multi"
  local workspace="$prefix-workspace"
  local left_pid="$runtime_dir/$prefix-left.pid"
  local right_pid="$runtime_dir/$prefix-right.pid"
  local session="$prefix-session"
  local layout_string='layout {
    pane split_direction="vertical" {
        pane focus=true
        pane
        pane
    }
}'
  terminal_pid_files+=("$left_pid" "$right_pid")
  zellij_sessions+=("$session")

  focus_fresh_workspace
  local left first second right
  launch_terminal "$prefix-left" "$left_pid"
  left="$launched_window"
  test_windows+=("$left")
  name_focused_workspace "$workspace"
  test_workspaces+=("$workspace")
  move_window_column_last "$left"

  launch_zellij "$session" 3 "$layout_string"
  first="$launched_window"
  test_windows+=("$first")
  move_window_column_last "$first"

  mark_state "attaching a second terminal client to $session"
  launch_ghostty "$project_dir/tests/fixtures/zellij-attach" \
    "$zellij_cache_home" "$session"
  wait_for_zellij_clients "$session" 2
  for _ in {1..150}; do
    second="$(niri msg --json windows | jq -r --arg title "$session" --argjson first "$first" \
      '.[] | select(.id != $first and (.title | contains($title))) | .id' | tail -1)"
    [[ -n "$second" ]] && break
    sleep 0.05
  done
  [[ -n "$second" ]] || {
    echo "timed out waiting for the second Zellij window" >&2
    return 1
  }
  test_windows+=("$second")
  move_window_column_last "$second"

  launch_terminal "$prefix-right" "$right_pid"
  right="$launched_window"
  test_windows+=("$right")
  move_window_column_last "$right"

  local status first_client second_client
  for _ in {1..150}; do
    status="$(niri-zvim status --json)"
    first_client="$(jq -r --arg session "$session" --argjson window "$first" \
      '.zellij[] | select(.session == $session and .niri_window_id == $window) | .client_id' \
      <<<"$status")"
    second_client="$(jq -r --arg session "$session" --argjson window "$second" \
      '.zellij[] | select(.session == $session and .niri_window_id == $window) | .client_id' \
      <<<"$status")"
    if [[ -n "$first_client" && -n "$second_client" && "$first_client" != "$second_client" ]]; then
      break
    fi
    sleep 0.02
  done
  if [[ -z "$first_client" || -z "$second_client" || "$first_client" == "$second_client" ]]; then
    test_fail "one graph client is bound to each attached Niri window" "  "
    jq --arg session "$session" '.zellij | map(select(.session == $session))' <<<"$status" >&2
    return 1
  fi
  test_pass "one graph client is bound to each attached Niri window" "  "

  local -a panes
  mapfile -t panes < <(zellij_pane_ids "$session")
  focus_niri_window "$left"
  navigate_expect_zellij_client "enter the first attached client" right \
    "$first" "$session" "$first_client" "${panes[0]}"
  navigate_expect_zellij_client "move only the first attached client" right \
    "$first" "$session" "$first_client" "${panes[1]}"
  assert_zellij_client_pane_now "the second client did not move" \
    "$session" "$second_client" "${panes[0]}"
  navigate_expect_zellij_client "move first attached client to its edge" right \
    "$first" "$session" "$first_client" "${panes[2]}"
  navigate_expect_zellij_client "cross into the second attached client" right \
    "$second" "$session" "$second_client" "${panes[0]}"
  navigate_expect_zellij_client "move only the second attached client" right \
    "$second" "$session" "$second_client" "${panes[1]}"
  assert_zellij_client_pane_now "the first client remained at its edge" \
    "$session" "$first_client" "${panes[2]}"
  navigate_expect_zellij_client "move second attached client to its edge" right \
    "$second" "$session" "$second_client" "${panes[2]}"
  navigate_expect "leave both attached clients" right "$right" - - - -

  navigate_expect_zellij_client "re-enter the second attached client" left \
    "$second" "$session" "$second_client" "${panes[2]}"
  navigate_expect_zellij_client "move second attached client back" left \
    "$second" "$session" "$second_client" "${panes[1]}"
  assert_zellij_client_pane_now "reverse movement still leaves the first client alone" \
    "$session" "$first_client" "${panes[2]}"
  navigate_expect_zellij_client "move second attached client to its left edge" left \
    "$second" "$session" "$second_client" "${panes[0]}"
  navigate_expect_zellij_client "cross back into the first attached client" left \
    "$first" "$session" "$first_client" "${panes[2]}"
  navigate_expect_zellij_client "move first attached client back" left \
    "$first" "$session" "$first_client" "${panes[1]}"
  assert_zellij_client_pane_now "reverse movement leaves the second client at its edge" \
    "$session" "$second_client" "${panes[0]}"
  navigate_expect_zellij_client "move first attached client to its left edge" left \
    "$first" "$session" "$first_client" "${panes[0]}"
  navigate_expect "leave both attached clients to the left" left "$left" - - - -
)
