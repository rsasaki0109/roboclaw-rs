#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OUTPUT_PATH="${1:-$ROOT_DIR/target/demo-recording-$(date +%Y%m%d-%H%M%S).mp4}"
DISPLAY_NAME="${DISPLAY:-:0}"
FPS="${FPS:-30}"
CRF="${CRF:-23}"
PRESET="${PRESET:-veryfast}"

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "ffmpeg is required but not installed" >&2
    exit 1
fi

if ! command -v xwininfo >/dev/null 2>&1; then
    echo "xwininfo is required but not installed" >&2
    exit 1
fi

mkdir -p "$(dirname "$OUTPUT_PATH")"

echo "Click the terminal window you want to record."
window_info="$(xwininfo)"

width="$(printf '%s\n' "$window_info" | awk -F: '/Width:/ {gsub(/ /, "", $2); print $2; exit}')"
height="$(printf '%s\n' "$window_info" | awk -F: '/Height:/ {gsub(/ /, "", $2); print $2; exit}')"
xpos="$(printf '%s\n' "$window_info" | awk -F: '/Absolute upper-left X:/ {gsub(/ /, "", $2); print $2; exit}')"
ypos="$(printf '%s\n' "$window_info" | awk -F: '/Absolute upper-left Y:/ {gsub(/ /, "", $2); print $2; exit}')"

if [[ -z "$width" || -z "$height" || -z "$xpos" || -z "$ypos" ]]; then
    echo "failed to parse xwininfo output" >&2
    exit 1
fi

echo "Recording ${width}x${height} at ${xpos},${ypos} on ${DISPLAY_NAME}"
echo "Output: $OUTPUT_PATH"
echo "Stop recording with q in the ffmpeg terminal."

ffmpeg \
  -y \
  -video_size "${width}x${height}" \
  -framerate "$FPS" \
  -f x11grab \
  -i "${DISPLAY_NAME}+${xpos},${ypos}" \
  -pix_fmt yuv420p \
  -c:v libx264 \
  -preset "$PRESET" \
  -crf "$CRF" \
  "$OUTPUT_PATH"
