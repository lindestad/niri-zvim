# Shared live-test helpers. Sourced by scripts/live-tests/common.sh.
# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

cleanup_case() {
  local status="${1:-$?}"
  local file socket session pid_file pid workspace
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
  for file in "${case_files[@]}"; do
    rm -f "$file"
  done
  rm -f "$runtime_dir"/niri-zvim-live-"$tag"-*.kdl
}

cleanup_timed_out_case() {
  local pid pid_file socket session workspace
  while IFS= read -r session; do
    [[ -n "$session" ]] || continue
    zellij kill-session "$session" >/dev/null 2>&1 || true
    zellij delete-session "$session" --force >/dev/null 2>&1 || true
  done < <(timeout 1s zellij list-sessions --short --no-formatting 2>/dev/null |
    grep "niri-zvim-live-$tag" || true)
  for pid_file in "$runtime_dir"/niri-zvim-live-"$tag"-*.pid; do
    [[ -s "$pid_file" ]] || continue
    pid="$(<"$pid_file")"
    [[ "$pid" =~ ^[1-9][0-9]*$ ]] || continue
    kill "$pid" >/dev/null 2>&1 || true
  done
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
  case_files=()
  launched_window=""
  trap cleanup_case EXIT
  trap 'trap - EXIT INT TERM; cleanup_case 130; exit 130' INT
  trap 'trap - EXIT INT TERM; cleanup_case 143; exit 143' TERM
}

failures=()
interruptions=()
run_case() {
  local name="$1"
  local function="$2"
  local status
  ((${#interruptions[@]} == 0)) || return 0
  printf '\n-- %s --\n' "$name"
  rm -f "$case_debug_file" "$case_interruption_file"
  mark_state "starting case: $name"
  timeout --signal=TERM --kill-after=15s "${case_timeout_seconds}s" \
    bash -c "$function"
  status=$?
  if [[ -s "$case_interruption_file" ]]; then
    interruptions+=("$name: $(<"$case_interruption_file")")
    test_interrupted "$name"
  elif ((status == 0)); then
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
