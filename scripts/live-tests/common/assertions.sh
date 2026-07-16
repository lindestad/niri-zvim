# Shared live-test helpers. Sourced by scripts/live-tests/common.sh.
# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

expect_state() {
  local label="$1"
  local expected_niri="$2"
  local session="$3"
  local expected_pane="$4"
  local socket="$5"
  local expected_nvim="$6"
  local actual_niri actual_pane actual_nvim
  for _ in {1..150}; do
    actual_niri="$(niri msg --json focused-window | jq -r '.id // empty')"
    actual_pane=-
    actual_nvim=-
    if [[ "$session" != - ]]; then
      actual_pane="$(focused_zellij_pane "$session")"
    fi
    if [[ "$socket" != - ]]; then
      actual_nvim="$(nvim_remote_expr "$socket" 'win_getid()')"
    fi
    if [[ "$actual_niri" == "$expected_niri" &&
      "$actual_pane" == "$expected_pane" &&
      "$actual_nvim" == "$expected_nvim" ]]; then
      test_pass "$label" "  "
      return 0
    fi
    sleep 0.02
  done
  test_fail "$label" "  "
  printf '    expected niri=%s pane=%s nvim=%s\n' \
    "$expected_niri" "$expected_pane" "$expected_nvim" >&2
  printf '    actual   niri=%s pane=%s nvim=%s\n' \
    "$actual_niri" "$actual_pane" "$actual_nvim" >&2
  return 1
}

assert_state_now() {
  local label="$1"
  local expected_niri="$2"
  local session="$3"
  local expected_pane="$4"
  local socket="$5"
  local expected_nvim="$6"
  local actual_niri actual_pane=- actual_nvim=-
  actual_niri="$(niri msg --json focused-window | jq -r '.id // empty')"
  if [[ "$session" != - ]]; then
    actual_pane="$(focused_zellij_pane "$session")"
  fi
  if [[ "$socket" != - ]]; then
    actual_nvim="$(nvim_remote_expr "$socket" 'win_getid()')"
  fi
  if [[ "$actual_niri" == "$expected_niri" &&
    "$actual_pane" == "$expected_pane" &&
    "$actual_nvim" == "$expected_nvim" ]]; then
    test_pass "$label" "  "
    return 0
  fi
  test_fail "$label" "  "
  printf '    expected niri=%s pane=%s nvim=%s\n' \
    "$expected_niri" "$expected_pane" "$expected_nvim" >&2
  printf '    actual   niri=%s pane=%s nvim=%s\n' \
    "$actual_niri" "$actual_pane" "$actual_nvim" >&2
  return 1
}

assert_nvim_now() {
  local label="$1"
  local socket="$2"
  local expected="$3"
  local actual
  actual="$(nvim_remote_expr "$socket" 'win_getid()')"
  if [[ "$actual" == "$expected" ]]; then
    test_pass "$label" "  "
    return 0
  fi
  test_fail "$label (expected nvim=$expected, actual nvim=$actual)" "  "
  return 1
}

expect_zellij_side() {
  local label="$1"
  local expected_niri="$2"
  local session="$3"
  local side="$4"
  local focused pane_x max_x
  for _ in {1..150}; do
    focused="$(niri msg --json focused-window | jq -r '.id // empty')"
    local focused_pane
    focused_pane="$(focused_zellij_pane "$session")"
    read -r pane_x max_x < <(zellij --session "$session" action list-panes --all --json |
      jq -r --argjson focused "$focused_pane" '[.[] | select(.is_plugin | not)] as $panes
        | ($panes | map(.pane_x) | max) as $max
        | ($panes[] | select(.id == $focused) | [.pane_x, $max] | @tsv)')
    if [[ "$focused" == "$expected_niri" &&
      (("$side" == left && "$pane_x" -lt "$max_x") ||
        ("$side" == right && "$pane_x" -eq "$max_x")) ]]; then
      test_pass "$label" "  "
      return 0
    fi
    sleep 0.02
  done
  test_fail "$label" "  "
  printf '    expected niri=%s zellij-side=%s\n' "$expected_niri" "$side" >&2
  printf '    actual   niri=%s pane-x=%s max-x=%s\n' \
    "$focused" "${pane_x:-unknown}" "${max_x:-unknown}" >&2
  return 1
}

navigate_expect_zellij_side() {
  local label="$1"
  local direction="$2"
  shift 2
  mark_state "navigating $direction: $label"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    niri-zvim "$direction"
  expect_zellij_side "$label" "$@"
  sleep 0.1
  mark_state "completed: $label"
}

expect_nvim_changed() {
  local label="$1"
  local expected_niri="$2"
  local socket="$3"
  local previous="$4"
  local session="${5:--}"
  local expected_pane="${6:--}"
  local focused pane current
  for _ in {1..150}; do
    focused="$(niri msg --json focused-window | jq -r '.id // empty')"
    current="$(nvim_remote_expr "$socket" 'win_getid()')"
    pane=-
    if [[ "$session" != - ]]; then
      pane="$(focused_zellij_pane "$session")"
    fi
    if [[ "$focused" == "$expected_niri" && "$current" != "$previous" &&
      "$pane" == "$expected_pane" ]]; then
      test_pass "$label" "  "
      return 0
    fi
    sleep 0.02
  done
  test_fail "$label" "  "
  printf '    expected niri=%s nvim!=%s pane=%s\n' \
    "$expected_niri" "$previous" "$expected_pane" >&2
  printf '    actual   niri=%s nvim=%s pane=%s\n' \
    "$focused" "$current" "$pane" >&2
  return 1
}

navigate_expect_nvim_changed() {
  local label="$1"
  local direction="$2"
  shift 2
  mark_state "navigating $direction: $label"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    niri-zvim "$direction"
  expect_nvim_changed "$label" "$@"
  sleep 0.1
  mark_state "completed: $label"
}

navigate_expect() {
  local label="$1"
  local direction="$2"
  shift 2
  mark_state "navigating $direction: $label"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    niri-zvim "$direction"
  expect_state "$label" "$@"
  sleep 0.1
  mark_state "completed: $label"
}

navigate_burst_expect() {
  local label="$1"
  local direction="$2"
  local count="$3"
  shift 3
  local keypress
  mark_state "sending $count rapid $direction commands: $label"
  for ((keypress = 0; keypress < count; keypress++)); do
    timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
      niri-zvim "$direction"
  done
  sleep 1
  assert_state_now "$label" "$@"
  mark_state "completed rapid burst: $label"
}
