#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

DEMO_SLEEP="${DEMO_SLEEP:-1.2}"
DEMO_PAUSE="${DEMO_PAUSE:-2.0}"
DEMO_PROFILE="${DEMO_PROFILE:-full}"
ROBOCLAW_LLM_PROVIDER="${ROBOCLAW_LLM_PROVIDER:-auto}"
ROBOCLAW_OLLAMA_MODEL="${ROBOCLAW_OLLAMA_MODEL:-llama3.2:1b}"
ROBOCLAW_OLLAMA_HOST="${ROBOCLAW_OLLAMA_HOST:-http://127.0.0.1:11434}"
DEMO_REQUIRE_LOCAL="${DEMO_REQUIRE_LOCAL:-0}"

export ROBOCLAW_LLM_PROVIDER
export ROBOCLAW_OLLAMA_MODEL
export ROBOCLAW_OLLAMA_HOST

pick_instruction="${1:-Use the simulator to pick up the red cube and place it in bin_a.}"
wave_instruction="${2:-Wave to acknowledge the operator.}"

section() {
    printf '\n\033[1;36m# %s\033[0m\n' "$1"
    sleep "$DEMO_SLEEP"
}

scene() {
    printf '\n\033[1;35m## %s\033[0m\n' "$1"
    sleep "$DEMO_SLEEP"
}

note() {
    printf '\033[0;37m%s\033[0m\n' "$1"
    sleep "$DEMO_SLEEP"
}

run_step() {
    printf '\n\033[1m$ %s\033[0m\n' "$1"
    sleep "$DEMO_SLEEP"
    eval "$1"
    sleep "$DEMO_PAUSE"
}

local_model_ready() {
    curl -sf "$ROBOCLAW_OLLAMA_HOST/api/tags" | grep -F "\"name\":\"$ROBOCLAW_OLLAMA_MODEL\"" >/dev/null 2>&1
}

if local_model_ready; then
    compare_providers="mock,local"
    execute_provider_prefix=""
elif [[ "$DEMO_REQUIRE_LOCAL" == "1" ]]; then
    echo "local model '$ROBOCLAW_OLLAMA_MODEL' is required but not available at $ROBOCLAW_OLLAMA_HOST" >&2
    exit 1
else
    compare_providers="mock"
    execute_provider_prefix="ROBOCLAW_LLM_PROVIDER=mock "
fi

clear
section "roboclaw-rs demo (${DEMO_PROFILE})"

if [[ "$compare_providers" == "mock" ]]; then
    printf '\n\033[1;33m! local model unavailable, falling back to mock-only demo\033[0m\n'
    sleep "$DEMO_SLEEP"
fi

case "$DEMO_PROFILE" in
    short|30s)
        scene "Planner comparison"
        note "Same instruction, same YAML skill catalog, different planner backends."
        run_step "cargo run --example compare_planners -- --providers $compare_providers \"$pick_instruction\""

        scene "End-to-end execution"
        note "The gateway executes the selected skill through simulator-safe tools."
        run_step "${execute_provider_prefix}cargo run --example pick_and_place -- \"$pick_instruction\""

        scene "Alternate instruction"
        note "A different instruction routes to a different skill."
        run_step "cargo run --example compare_planners -- --providers $compare_providers \"$wave_instruction\""
        ;;
    full|90s)
        scene "Planner comparison"
        note "Mock and local planners target the same YAML skill catalog."
        run_step "cargo run --example compare_planners -- --providers $compare_providers \"$pick_instruction\""

        scene "Skill execution"
        note "The gateway runs detect, move, grasp, and place through one backend interface."
        run_step "${execute_provider_prefix}cargo run --example pick_and_place -- \"$pick_instruction\""

        scene "Second skill execution"
        note "The same execution loop can drive a different skill."
        run_step "ROBOCLAW_LLM_PROVIDER=mock cargo run --example pick_and_place -- \"$wave_instruction\""

        scene "Planner contrast"
        note "Planner behavior stays observable before any hardware hookup."
        run_step "cargo run --example compare_planners -- --providers $compare_providers \"$wave_instruction\""
        ;;
    *)
        echo "Unsupported DEMO_PROFILE='$DEMO_PROFILE'. Use short, 30s, full, or 90s." >&2
        exit 1
        ;;
esac

section "demo complete"
