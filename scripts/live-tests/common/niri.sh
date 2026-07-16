# Shared live-test helpers. Sourced by scripts/live-tests/common.sh.
# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

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
