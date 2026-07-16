set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Build and install the daemon, client, and editor adapters.
install:
    ./scripts/install

# Run all latency benchmarks.
bench:
    ./scripts/bench

# Run the complete automated and live desktop test suites.
test:
    @source ./scripts/test-output; test_input_warning
    @sleep 2
    ./scripts/test
