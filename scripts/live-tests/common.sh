# Shared helpers for live navigation cases. This file is sourced by test-live.
# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

launch_ghostty() {
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    ghostty +new-window \
    --config-default-files=false \
    --config-file="$ghostty_config" \
    -e "$@"
}

mark_state() {
  printf '%s\n' "$1" >"$case_state_file"
}

dump_live_state() {
  local reason="$1"
  local session socket
  printf '  debug: %s\n' "$reason" >&2
  printf '  CPU strain: start=%s current=' "$test_start_cpu_strain" >&2
  cpu_strain_snapshot >&2
  printf '\n' >&2
  if [[ -r "$case_state_file" ]]; then
    printf '  last checkpoint: %s\n' "$(<"$case_state_file")" >&2
  fi
  printf '  Niri focus: ' >&2
  timeout 1s niri msg --json focused-window 2>/dev/null |
    jq -c '{id,title,workspace_id}' >&2 || echo unavailable >&2
  echo "  relevant Niri workspaces:" >&2
  timeout 1s niri msg --json workspaces 2>/dev/null |
    jq -c --arg tag "niri-zvim-live-$tag" \
      '.[] | select(.is_focused or ((.name // "") | contains($tag)))
        | {id,idx,name,output,is_focused,active_window_id}' >&2 || true
  echo "  relevant Niri windows:" >&2
  timeout 1s niri msg --json windows 2>/dev/null |
    jq -c --arg tag "niri-zvim-live-$tag" \
      '.[] | select(.is_focused or (.title | contains($tag)))
        | {id,title,workspace_id,is_focused}' >&2 || true
  while IFS= read -r session; do
    [[ -n "$session" ]] || continue
    printf '  Zellij %s: ' "$session" >&2
    timeout 1s zellij --session "$session" action list-panes --all --json \
      2>/dev/null | jq -c '[.[] | select(.is_plugin | not)
        | {id,is_focused,pane_x,pane_columns}]' >&2 || echo unavailable >&2
  done < <(timeout 1s zellij list-sessions --short --no-formatting 2>/dev/null |
    grep "niri-zvim-live-$tag" || true)
  shopt -s nullglob
  for socket in "$runtime_dir"/niri-zvim-live-"$tag"*.sock; do
    [[ -S "$socket" ]] || continue
    printf '  Neovim %s: ' "$(basename "$socket")" >&2
    timeout 1s nvim --headless --clean --server "$socket" --remote-expr \
      'json_encode(#{focused: win_getid(), windows: map(getwininfo(), {_, v -> v.winid})})' \
      2>/dev/null >&2 || echo unavailable >&2
  done
  shopt -u nullglob
  echo "  relevant processes:" >&2
  # shellcheck disable=SC2009
  ps -eo pid,ppid,etimes,stat,args |
    grep "[n]iri-zvim-live-$tag" >&2 || true
}

cpu_strain_snapshot() {
  local -a load_fields
  local load_1 load_5 load_15 pressure_kind pressure_10 pressure_60 pressure_300 pressure_rest
  read -ra load_fields </proc/loadavg
  load_1="${load_fields[0]}"
  load_5="${load_fields[1]}"
  load_15="${load_fields[2]}"
  read -r pressure_kind pressure_10 pressure_60 pressure_300 pressure_rest \
    </proc/pressure/cpu || true
  [[ "$pressure_kind" == some ]] || {
    pressure_10=unavailable
    pressure_60=unavailable
    pressure_300=unavailable
  }
  printf 'load=%s/%s/%s cpus=%s psi=%s,%s,%s' \
    "$load_1" "$load_5" "$load_15" "$test_cpu_count" \
    "$pressure_10" "$pressure_60" "$pressure_300"
}

wait_for_socket() {
  local socket="$1"
  for _ in {1..150}; do
    [[ -S "$socket" ]] && return 0
    sleep 0.05
  done
  echo "timed out waiting for socket $socket" >&2
  return 1
}

wait_for_file() {
  local file="$1"
  for _ in {1..150}; do
    [[ -s "$file" ]] && return 0
    sleep 0.05
  done
  echo "timed out waiting for file $file" >&2
  return 1
}

wait_for_window() {
  local title="$1"
  local mode="$2"
  local window
  for _ in {1..150}; do
    if [[ "$mode" == exact ]]; then
      window="$(niri msg --json windows | jq -r --arg title "$title" \
        '.[] | select(.title == $title) | .id' | tail -1)"
    else
      window="$(niri msg --json windows | jq -r --arg title "$title" \
        '.[] | select(.title | contains($title)) | .id' | tail -1)"
    fi
    if [[ -n "$window" ]]; then
      printf '%s\n' "$window"
      return 0
    fi
    sleep 0.05
  done
  echo "timed out waiting for window $title" >&2
  return 1
}

wait_for_windows_gone() {
  local current
  for _ in {1..150}; do
    current="$(niri msg --json windows)"
    local found=false
    local window
    for window in "$@"; do
      if jq -e --argjson id "$window" '.[] | select(.id == $id)' \
        <<<"$current" >/dev/null; then
        found=true
        break
      fi
    done
    [[ "$found" == false ]] && return 0
    sleep 0.05
  done
  echo "timed out waiting for test windows to close: $*" >&2
  return 1
}

nvim_remote_expr() {
  local socket="$1"
  local expression="$2"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    nvim --headless --clean --server "$socket" --remote-expr "$expression"
}

nvim_window_ids() {
  local socket="$1"
  nvim_remote_expr "$socket" \
    'json_encode(map(getwininfo(), {_, v -> [v.winid, v.wincol]}))' |
    jq -r 'sort_by(.[1]) | .[][0]'
}

wait_for_nvim_count() {
  local socket="$1"
  local expected="$2"
  local count
  for _ in {1..150}; do
    count="$(nvim_window_ids "$socket" | wc -l)"
    [[ "$count" == "$expected" ]] && return 0
    sleep 0.05
  done
  echo "Neovim did not expose $expected windows" >&2
  return 1
}

focused_zellij_pane() {
  local session="$1"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    zellij --session "$session" action list-clients 2>/dev/null |
    awk 'NR > 1 && $2 ~ /^terminal_[0-9]+$/ {
      sub(/^terminal_/, "", $2)
      print $2
      exit
    }'
}

zellij_pane_ids() {
  local session="$1"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    zellij --session "$session" action list-panes --all --json 2>/dev/null |
    jq -r '[.[] | select((.is_plugin | not) and .is_selectable and (.is_suppressed | not))]
      | sort_by(.pane_x) | .[].id'
}

zellij_layout_signature() {
  local session="$1"
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    zellij --session "$session" action list-panes --all --json 2>/dev/null |
    jq -c '[.[] | select(.is_plugin | not)
      | {id, x: .pane_x, y: .pane_y, rows: .pane_rows, columns: .pane_columns}]
      | sort_by(.id)'
}

wait_for_zellij_session() {
  local session="$1"
  for _ in {1..150}; do
    timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
      zellij list-sessions --short --no-formatting 2>/dev/null |
      grep -qx "$session" && return 0
    sleep 0.05
  done
  echo "timed out waiting for Zellij session $session" >&2
  return 1
}

wait_for_zellij_count() {
  local session="$1"
  local expected="$2"
  local count
  for _ in {1..150}; do
    count="$(zellij_pane_ids "$session" | wc -l)"
    [[ "$count" == "$expected" ]] && return 0
    sleep 0.05
  done
  echo "Zellij did not expose $expected panes in $session" >&2
  return 1
}

focus_nvim_window() {
  local socket="$1"
  local window="$2"
  nvim_remote_expr "$socket" "win_gotoid($window)" >/dev/null
  for _ in {1..150}; do
    [[ "$(nvim_remote_expr "$socket" 'win_getid()')" == "$window" ]] && {
      sleep 0.1
      return 0
    }
    sleep 0.02
  done
  echo "could not focus Neovim window $window" >&2
  return 1
}

wait_for_niri_window() {
  local expected="$1"
  for _ in {1..150}; do
    if [[ "$(niri msg --json focused-window | jq -r '.id // empty')" == "$expected" ]]; then
      sleep 0.05
      return 0
    fi
    sleep 0.02
  done
  echo "Niri focused window did not become $expected" >&2
  return 1
}

focus_niri_window() {
  local window="$1"
  niri msg action focus-window --id "$window" >/dev/null
  wait_for_niri_window "$window"
}

focused_workspace() {
  niri msg --json workspaces | jq -c '.[] | select(.is_focused)'
}

focus_fresh_workspace() {
  mark_state "finding a fresh workspace"
  focus_niri_window "$original_window"
  local output idx
  output="$(focused_workspace | jq -r '.output')"
  idx="$(niri msg --json workspaces | jq -r --arg output "$output" \
    '[.[] | select(.output == $output and .active_window_id == null and .name == null)]
      | max_by(.idx) | .idx // empty')"
  if [[ -z "$idx" ]]; then
    echo "could not find Niri's trailing empty workspace on $output" >&2
    return 1
  fi
  niri msg action focus-workspace "$idx" >/dev/null
  for _ in {1..150}; do
    local current
    current="$(focused_workspace)"
    if [[ "$(jq -r '.output' <<<"$current")" == "$output" &&
    "$(jq -r '.idx' <<<"$current")" == "$idx" &&
    "$(jq -r '.active_window_id' <<<"$current")" == null ]]; then
      return 0
    fi
    sleep 0.02
  done
  echo "could not focus the empty workspace on $output" >&2
  return 1
}

name_focused_workspace() {
  local name="$1"
  niri msg action set-workspace-name "$name" >/dev/null
  for _ in {1..150}; do
    if niri msg --json workspaces | jq -e --arg name "$name" \
      '.[] | select(.name == $name and .is_focused)' >/dev/null; then
      return 0
    fi
    sleep 0.02
  done
  echo "could not name test workspace $name" >&2
  return 1
}

move_window_column_last() {
  local window="$1"
  focus_niri_window "$window"
  niri msg action move-column-to-last >/dev/null
}

consume_window_below() {
  local upper="$1"
  local lower="$2"
  local upper_position lower_position
  focus_niri_window "$upper"
  niri msg action consume-window-into-column >/dev/null
  for _ in {1..150}; do
    upper_position="$(niri msg --json windows | jq -r --argjson id "$upper" \
      '.[] | select(.id == $id) | .layout.pos_in_scrolling_layout | @tsv')"
    lower_position="$(niri msg --json windows | jq -r --argjson id "$lower" \
      '.[] | select(.id == $id) | .layout.pos_in_scrolling_layout | @tsv')"
    if [[ "$upper_position" =~ ^([0-9]+)$'\t'1$ &&
      "$lower_position" == "${BASH_REMATCH[1]}"$'\t'2 ]]; then
      sleep 0.1
      return 0
    fi
    sleep 0.02
  done
  echo "could not consume Niri window $lower below $upper" >&2
  return 1
}

launch_terminal() {
  local title="$1"
  local pid_file="$2"
  mark_state "launching empty terminal $title"
  launch_ghostty "$terminal_fixture" "$title" "$pid_file"
  mark_state "waiting for empty terminal process $title"
  wait_for_file "$pid_file"
  mark_state "waiting for empty terminal Niri window $title"
  launched_window="$(wait_for_window "$title" exact)"
  mark_state "empty terminal $title is ready as Niri window $launched_window"
}

launch_nvim() {
  local title="$1"
  local socket="$2"
  local count="$3"
  local layout='set splitright'
  local split_index
  for ((split_index = 1; split_index < count; split_index++)); do
    layout+=' | vsplit'
  done
  mark_state "launching $count-window Neovim $title"
  launch_ghostty nvim -u "$nvim_fixture" --noplugin --listen "$socket" \
    -c "set title titlestring=$title | $layout"
  mark_state "waiting for Neovim socket $title"
  wait_for_socket "$socket"
  mark_state "waiting for $count Neovim windows in $title"
  wait_for_nvim_count "$socket" "$count"
  mark_state "waiting for Neovim Niri window $title"
  launched_window="$(wait_for_window "$title" exact)"
  mark_state "Neovim $title is ready as Niri window $launched_window"
}

launch_zellij() {
  local session="$1"
  local expected_panes="$2"
  local layout_string="${3:-}"
  local layout_file="$runtime_dir/$session.kdl"
  mark_state "launching Zellij session $session"
  if [[ -n "$layout_string" ]]; then
    printf '%s\n' "$layout_string" >"$layout_file"
    launch_ghostty zellij --session "$session" --new-session-with-layout "$layout_file"
  else
    launch_ghostty zellij --session "$session"
  fi
  mark_state "waiting for Zellij session $session"
  wait_for_zellij_session "$session"
  mark_state "waiting for Zellij Niri window $session"
  launched_window="$(wait_for_window "$session" contains)"
  mark_state "waiting for $expected_panes initial Zellij panes in $session"
  wait_for_zellij_count "$session" "$expected_panes"
  sleep 0.6
  mark_state "Zellij $session is ready as Niri window $launched_window"
}

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

cleanup_case() {
  local status="${1:-$?}"
  local socket session pid_file pid workspace
  set +e
  if ((status != 0)); then
    dump_live_state "case exited with status $status before cleanup" \
      >"$case_debug_file" 2>&1
  fi
  for socket in "${nvim_sockets[@]}"; do
    if [[ -S "$socket" ]]; then
      nvim_remote_expr "$socket" 'execute("qa!")' >/dev/null 2>&1 || true
    fi
  done
  for session in "${zellij_sessions[@]}"; do
    zellij kill-session "$session" >/dev/null 2>&1 || true
    zellij delete-session "$session" --force >/dev/null 2>&1 || true
    pkill -TERM -f -- "^zellij --session $session pipe --name niri-zvim$" \
      >/dev/null 2>&1 || true
  done
  for pid_file in "${terminal_pid_files[@]}"; do
    if [[ -s "$pid_file" ]]; then
      pid="$(<"$pid_file")"
      kill "$pid" >/dev/null 2>&1 || true
    fi
  done
  if ((${#test_windows[@]})); then
    wait_for_windows_gone "${test_windows[@]}" || true
  fi
  for workspace in "${test_workspaces[@]}"; do
    niri msg action unset-workspace-name "$workspace" >/dev/null 2>&1 || true
  done
  for pid_file in "${terminal_pid_files[@]}"; do
    rm -f "$pid_file"
  done
  rm -f "$runtime_dir"/niri-zvim-live-"$tag"-*.kdl
}

cleanup_timed_out_case() {
  local socket session workspace
  while IFS= read -r session; do
    [[ -n "$session" ]] || continue
    zellij kill-session "$session" >/dev/null 2>&1 || true
    zellij delete-session "$session" --force >/dev/null 2>&1 || true
  done < <(timeout 1s zellij list-sessions --short --no-formatting 2>/dev/null |
    grep "niri-zvim-live-$tag" || true)
  for _ in {1..60}; do
    shopt -s nullglob
    for socket in "$runtime_dir"/niri-zvim-live-"$tag"*.sock; do
      [[ -S "$socket" ]] || continue
      nvim_remote_expr "$socket" 'execute("qa!")' >/dev/null 2>&1 || true
    done
    shopt -u nullglob
    pkill -TERM -f -- "niri-zvim-live-$tag" >/dev/null 2>&1 || true
    sleep 0.05
  done
  for workspace in $(niri msg --json workspaces 2>/dev/null |
    jq -r --arg tag "niri-zvim-live-$tag" \
      '.[] | select((.name // "") | contains($tag)) | .name'); do
    niri msg action unset-workspace-name "$workspace" >/dev/null 2>&1 || true
  done
  for _ in {1..150}; do
    if ! niri msg --json windows 2>/dev/null |
      jq -e --arg tag "niri-zvim-live-$tag" \
        '.[] | select(.title | contains($tag))' >/dev/null; then
      break
    fi
    sleep 0.02
  done
  rm -f "$runtime_dir"/niri-zvim-live-"$tag"*.sock \
    "$runtime_dir"/niri-zvim-live-"$tag"*.pid \
    "$runtime_dir"/niri-zvim-live-"$tag"*.kdl
}

begin_case() {
  test_windows=()
  nvim_sockets=()
  zellij_sessions=()
  terminal_pid_files=()
  test_workspaces=()
  launched_window=""
  trap cleanup_case EXIT
  trap 'trap - EXIT INT TERM; cleanup_case 130; exit 130' INT
  trap 'trap - EXIT INT TERM; cleanup_case 143; exit 143' TERM
}

failures=()
run_case() {
  local name="$1"
  local function="$2"
  local status
  printf '\n-- %s --\n' "$name"
  rm -f "$case_debug_file"
  mark_state "starting case: $name"
  timeout --signal=TERM --kill-after=15s "${case_timeout_seconds}s" \
    bash -c "$function"
  status=$?
  if ((status == 0)); then
    test_pass "$name"
  elif ((status == 124)); then
    failures+=("$name (timed out after ${case_timeout_seconds}s)")
    test_timeout "$name exceeded ${case_timeout_seconds}s"
    if [[ ! -s "$case_debug_file" ]]; then
      dump_live_state "case timed out; child diagnostics were unavailable" \
        >"$case_debug_file" 2>&1
    fi
    cleanup_timed_out_case
    if [[ -s "$case_debug_file" ]]; then
      sed 's/^/  /' "$case_debug_file" >&2
    fi
  else
    failures+=("$name")
    test_fail "$name"
    if [[ -s "$case_debug_file" ]]; then
      sed 's/^/  /' "$case_debug_file" >&2
    fi
  fi
}
