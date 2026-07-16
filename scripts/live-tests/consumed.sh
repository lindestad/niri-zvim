# Compatibility loader for consumed-column live scenarios.
# shellcheck shell=bash

consumed_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=scripts/live-tests/consumed-zellij.sh
source "$consumed_dir/consumed-zellij.sh"
# shellcheck source=scripts/live-tests/consumed-reflow.sh
source "$consumed_dir/consumed-reflow.sh"
# shellcheck source=scripts/live-tests/consumed-nvim.sh
source "$consumed_dir/consumed-nvim.sh"
