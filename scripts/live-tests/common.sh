# Shared helpers for live navigation cases. This file is sourced by test-live.
# shellcheck shell=bash
# shellcheck disable=SC2030,SC2031

common_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/common" && pwd)"

# shellcheck source=scripts/live-tests/common/terminals.sh
source "$common_dir/terminals.sh"
# shellcheck source=scripts/live-tests/common/diagnostics.sh
source "$common_dir/diagnostics.sh"
# shellcheck source=scripts/live-tests/common/fixtures.sh
source "$common_dir/fixtures.sh"
# shellcheck source=scripts/live-tests/common/niri.sh
source "$common_dir/niri.sh"
# shellcheck source=scripts/live-tests/common/assertions.sh
source "$common_dir/assertions.sh"
# shellcheck source=scripts/live-tests/common/harness.sh
source "$common_dir/harness.sh"
