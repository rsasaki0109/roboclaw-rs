#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"
mkdir -p target

# Use deterministic providers and disable injected failures for this smoke test.
ROBOCLAW_LLM_PROVIDER=mock \
ROBOCLAW_ROS2_BRIDGE=mock \
ROBOCLAW_SENSOR_FAIL_COUNT=0 \
ROBOCLAW_MOTOR_FAIL_COUNT=0 \
cargo run --locked --example pick_and_place -- \
  "Pick up the red cube and place it in bin_a." | tee target/mock-demo.log

# The example can exit successfully with an incomplete report; check behavior too.
grep -Fxq 'planner_provider=mock' target/mock-demo.log
grep -Fxq 'ros2_transport=mock' target/mock-demo.log
grep -Fxq 'selected_skill=pick_and_place' target/mock-demo.log
grep -Fxq 'completed=true' target/mock-demo.log
grep -Fxq 'steps_executed=4' target/mock-demo.log
grep -Fq 'last_action: Some("place"), last_pose: "bin_a", held_object: None' target/mock-demo.log
