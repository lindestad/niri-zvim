set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Build and install the daemon, client, and editor adapters.
install:
    ./scripts/install

# Run checks that do not control the desktop.
check:
    ./scripts/check

# Run all latency benchmarks.
bench:
    ./scripts/bench

# Compare native and daemon navigation on the live desktop.
bench-live:
    ./scripts/bench-live

# Run the complete automated and live desktop test suites.
test:
    ./scripts/test
