# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

test_original_desktop_preserved() {
  local current_windows current_workspaces
  current_windows="$(niri msg --json windows)"
  current_workspaces="$(niri msg --json workspaces)"
  if ! jq -en --argjson before "$original_windows" --argjson after "$current_windows" '
    all($before[]; . as $window |
      any($after[]; .id == $window.id and .workspace_id == $window.workspace_id))
  ' >/dev/null; then
    echo "an original window was closed or moved to another workspace" >&2
    return 1
  fi
  if ! jq -en --argjson before "$original_workspaces" --argjson after "$current_workspaces" '
    all($before[] | select(.active_window_id != null or .name != null); . as $workspace |
      any($after[]; .id == $workspace.id and .name == $workspace.name
        and .output == $workspace.output))
  ' >/dev/null; then
    echo "an original named or occupied workspace was deleted or changed" >&2
    return 1
  fi
  test_pass "all original windows and user workspaces are unchanged" "  "
}
