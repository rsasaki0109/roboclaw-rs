#!/usr/bin/env bash
set -euo pipefail

ROBOCLAW_OLLAMA_MODEL="${ROBOCLAW_OLLAMA_MODEL:-llama3.2:1b}"

if ! command -v ollama >/dev/null 2>&1; then
    echo "ollama is required but not installed" >&2
    exit 1
fi

if ollama list | awk 'NR > 1 {print $1}' | grep -Fx "$ROBOCLAW_OLLAMA_MODEL" >/dev/null 2>&1; then
    echo "model already present: $ROBOCLAW_OLLAMA_MODEL"
else
    echo "pulling model: $ROBOCLAW_OLLAMA_MODEL"
    ollama pull "$ROBOCLAW_OLLAMA_MODEL"
fi

echo "warming model: $ROBOCLAW_OLLAMA_MODEL"
curl -sf http://127.0.0.1:11434/api/generate \
  -d "{\"model\":\"$ROBOCLAW_OLLAMA_MODEL\",\"prompt\":\"Return JSON: {\\\"skill\\\":\\\"pick_and_place\\\"}\",\"stream\":false}" \
  >/dev/null

echo "demo model ready"
