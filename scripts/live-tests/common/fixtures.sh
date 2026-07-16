# Shared live-test helpers. Sourced by scripts/live-tests/common.sh.
# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

launch_ghostty() {
  timeout --signal=TERM --kill-after=2s "${operation_timeout_seconds}s" \
    ghostty +new-window \
    --config-default-files=false \
    --config-file="$ghostty_config" \
    -e "$@"
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
