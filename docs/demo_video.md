# Demo Video Pack

This file is the recording pack for `roboclaw-rs`.

## Goal

The demo should show three points fast:

1. `roboclaw-rs` is agent-first, not just a low-level driver.
2. planners can swap while targeting the same YAML skill catalog.
3. simulator execution already follows the same backend abstraction we can reuse for hardware.

The terminal script now prints scene headers and one-line guidance before each block so the viewer can follow the story without extra editing.

## Fast Recording Setup

Use two terminals.

Terminal A:

```bash
./scripts/record_demo.sh target/roboclaw-demo.mp4
```

Terminal B:

```bash
./scripts/prepare_demo.sh
DEMO_PROFILE=short ./scripts/demo_terminal.sh
```

For the longer version:

```bash
./scripts/prepare_demo.sh
DEMO_PROFILE=full ./scripts/demo_terminal.sh
```

If no local model is available, `demo_terminal.sh` automatically falls back to a mock-only recording path. If you want the recording to fail instead of falling back, set:

```bash
DEMO_REQUIRE_LOCAL=1 DEMO_PROFILE=short ./scripts/demo_terminal.sh
```

## 30s Shot List

Target length: 25-35 seconds

Shot 1:
Show `compare_planners` on pick-and-place.
Duration: 6-8s
Narration: "roboclaw-rs is a Rust robotics agent stack with pluggable planners."

Shot 2:
Show end-to-end `pick_and_place`.
Duration: 10-12s
Narration: "The planner selects a YAML skill, then the gateway executes simulator-safe tools through one interface."

Shot 3:
Show `compare_planners` on wave.
Duration: 6-8s
Narration: "The same catalog can choose a different skill for a different instruction."

Shot 4:
Hold on `demo complete`.
Duration: 2-3s
Narration: "That gives us a sim-first base for ROS2 and real robot backends."

## 90s Shot List

Target length: 75-100 seconds

Shot 1:
Open on terminal header.
Duration: 3-4s
Narration: "This is roboclaw-rs, an agent-first robotics workspace in Rust."

Shot 2:
Show planner comparison for pick-and-place.
Duration: 10-12s
Narration: "The planner layer is provider-agnostic, so mock and local planning can target the same skill catalog."

Shot 3:
Show full pick-and-place execution.
Duration: 18-22s
Narration: "Once selected, the gateway runs detect, move, grasp, and place through a shared simulator backend."

Shot 4:
Pause on ROS2-like action and state lines.
Duration: 8-10s
Narration: "The gateway also emits action and state messages, which is the seam for ROS2 integration."

Shot 5:
Show wave execution with mock planning.
Duration: 12-15s
Narration: "A different instruction routes to a different skill without changing the execution loop."

Shot 6:
Show planner comparison for wave.
Duration: 10-12s
Narration: "That makes planner behavior visible and testable before connecting to a real robot."

Shot 7:
Close on the last lines.
Duration: 4-6s
Narration: "Next steps are typed ROS2 messages, richer skills, and real hardware backends."

## Voiceover Script

### 30s Version

"roboclaw-rs is a Rust robotics agent stack with pluggable planners. The planner selects a YAML skill, then the gateway executes simulator-safe tools through one interface. The same catalog can choose a different skill for a different instruction. That gives us a sim-first base for ROS2 and real robot backends."

### 90s Version

"This is roboclaw-rs, an agent-first robotics workspace in Rust. The planner layer is provider-agnostic, so mock, local, and cloud planners can target the same skill catalog. Here the system resolves pick and place, then executes detect, move, grasp, and place through a shared simulator backend. The gateway also emits action and state updates, which is the seam for ROS2 integration. When the instruction changes, the planner can route to a different skill such as wave_arm without changing the execution loop. That gives us a clean base for richer skills, typed ROS2 interfaces, and real hardware backends."

## Subtitle-Friendly Lines

- "roboclaw-rs is an agent-first robotics workspace in Rust."
- "Planners select from the same YAML skill catalog."
- "The gateway executes each step through a unified tool interface."
- "Simulator and hardware stay behind the same backend abstraction."
- "Different instructions select different skills without changing the loop."

## Framing Notes

- Use a large terminal font before recording.
- Keep the terminal around 120 columns wide.
- Use `DEMO_PROFILE=short` for social clips.
- Use `DEMO_PROFILE=full` for README, YouTube, or conference demos.
- Run `./scripts/prepare_demo.sh` before recording if you want the local planner path.
