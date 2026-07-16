# Shared live-test helpers. Sourced by scripts/live-tests/common.sh.
# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

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
