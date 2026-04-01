#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
cargo run --example planner_experiments -- --write-docs "$@"
