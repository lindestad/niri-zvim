#!/usr/bin/env bash

replace_managed_symlink() {
  local target="$1"
  local destination="$2"

  if [[ -e "$destination" && ! -L "$destination" ]]; then
    echo "refusing to replace existing path: $destination" >&2
    return 1
  fi
  ln -sfnT -- "$target" "$destination"
}
