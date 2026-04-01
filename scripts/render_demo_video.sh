#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

OUTPUT_PATH="${1:-$ROOT_DIR/target/roboclaw-demo-preview.mp4}"
DISPLAY_NAME="${DISPLAY:-:0}"
WINDOW_TITLE="${WINDOW_TITLE:-roboclaw-demo-render}"
WINDOW_GEOMETRY="${WINDOW_GEOMETRY:-120x34+2000+40}"
DEMO_PROFILE="${DEMO_PROFILE:-short}"
START_DELAY="${START_DELAY:-2}"
FPS="${FPS:-30}"
CRF="${CRF:-23}"
PRESET="${PRESET:-veryfast}"
PICK_INSTRUCTION="${PICK_INSTRUCTION:-Use the simulator to pick up the red cube and place it in bin_a.}"
WAVE_INSTRUCTION="${WAVE_INSTRUCTION:-Wave to acknowledge the operator.}"
DEMO_DISABLE_NOTIFICATIONS="${DEMO_DISABLE_NOTIFICATIONS:-1}"
DEMO_DISABLE_LOCKING="${DEMO_DISABLE_LOCKING:-1}"

if ! command -v gnome-terminal >/dev/null 2>&1; then
    echo "gnome-terminal is required but not installed" >&2
    exit 1
fi

if ! command -v ffmpeg >/dev/null 2>&1; then
    echo "ffmpeg is required but not installed" >&2
    exit 1
fi

extract_window_geometry() {
    printf '%s\n' "$1" | grep -o '[0-9]\+x[0-9]\++[0-9]\++[0-9]\+' | tail -n 1
}

extract_window_id() {
    printf '%s\n' "$1" | awk '{print $1}'
}

select_window_line() {
    local title="$1"
    local matches=""
    local match=""
    local geometry=""
    local rest=""
    local xpos=""
    local ypos=""
    local fallback_match=""
    local visible_match=""

    matches="$(xwininfo -tree -root | grep "\"$title\"" || true)"
    [[ -z "$matches" ]] && return 1

    while IFS= read -r match; do
        geometry="$(extract_window_geometry "$match" || true)"
        [[ -z "$geometry" ]] && continue

        fallback_match="$match"
        rest="${geometry#*x}"
        rest="${rest#*+}"
        xpos="${rest%%+*}"
        ypos="${rest#*+}"

        if [[ "$xpos" != "0" || "$ypos" != "0" ]]; then
            visible_match="$match"
        fi
    done <<< "$matches"

    if [[ -n "$visible_match" ]]; then
        printf '%s\n' "$visible_match"
        return 0
    fi

    if [[ -n "$fallback_match" ]]; then
        printf '%s\n' "$fallback_match"
        return 0
    fi

    return 1
}

cleanup() {
    if [[ -n "${FFMPEG_PID:-}" ]] && kill -0 "$FFMPEG_PID" >/dev/null 2>&1; then
        kill -INT "$FFMPEG_PID" >/dev/null 2>&1 || true
        wait "$FFMPEG_PID" || true
    fi
    if [[ -n "${TERM_PID:-}" ]] && kill -0 "$TERM_PID" >/dev/null 2>&1; then
        kill "$TERM_PID" >/dev/null 2>&1 || true
        wait "$TERM_PID" || true
    fi
    if [[ -n "${NOTIFICATION_BANNERS_ORIGINAL:-}" ]]; then
        gsettings set org.gnome.desktop.notifications show-banners "$NOTIFICATION_BANNERS_ORIGINAL" >/dev/null 2>&1 || true
    fi
    if [[ -n "${LOCK_ENABLED_ORIGINAL:-}" ]]; then
        gsettings set org.gnome.desktop.screensaver lock-enabled "$LOCK_ENABLED_ORIGINAL" >/dev/null 2>&1 || true
    fi
    if [[ -n "${IDLE_DELAY_ORIGINAL:-}" ]]; then
        gsettings set org.gnome.desktop.session idle-delay "$IDLE_DELAY_ORIGINAL" >/dev/null 2>&1 || true
    fi
}

trap cleanup EXIT

mkdir -p "$(dirname "$OUTPUT_PATH")"

if [[ "$DEMO_DISABLE_NOTIFICATIONS" == "1" ]] && command -v gsettings >/dev/null 2>&1; then
    NOTIFICATION_BANNERS_ORIGINAL="$(gsettings get org.gnome.desktop.notifications show-banners 2>/dev/null || true)"
    if [[ -n "$NOTIFICATION_BANNERS_ORIGINAL" ]]; then
        gsettings set org.gnome.desktop.notifications show-banners false >/dev/null 2>&1 || true
    fi
fi

if [[ "$DEMO_DISABLE_LOCKING" == "1" ]] && command -v gsettings >/dev/null 2>&1; then
    IDLE_DELAY_ORIGINAL="$(gsettings get org.gnome.desktop.session idle-delay 2>/dev/null || true)"
    LOCK_ENABLED_ORIGINAL="$(gsettings get org.gnome.desktop.screensaver lock-enabled 2>/dev/null || true)"

    [[ -n "$IDLE_DELAY_ORIGINAL" ]] && gsettings set org.gnome.desktop.session idle-delay 0 >/dev/null 2>&1 || true
    [[ -n "$LOCK_ENABLED_ORIGINAL" ]] && gsettings set org.gnome.desktop.screensaver lock-enabled false >/dev/null 2>&1 || true

    if command -v gdbus >/dev/null 2>&1; then
        gdbus call \
          --session \
          --dest org.gnome.ScreenSaver \
          --object-path /org/gnome/ScreenSaver \
          --method org.gnome.ScreenSaver.SetActive false >/dev/null 2>&1 || true

        for _ in $(seq 1 20); do
            if [[ "$(gdbus call --session --dest org.gnome.ScreenSaver --object-path /org/gnome/ScreenSaver --method org.gnome.ScreenSaver.GetActive 2>/dev/null || true)" == "(false,)" ]]; then
                break
            fi
            sleep 0.25
        done
    fi
fi

gnome-terminal \
  --wait \
  --hide-menubar \
  --title="$WINDOW_TITLE" \
  --geometry="$WINDOW_GEOMETRY" \
  -- bash -lc "cd '$ROOT_DIR'; printf '\033]0;$WINDOW_TITLE\a'; sleep '$START_DELAY'; DEMO_PROFILE='$DEMO_PROFILE' ./scripts/demo_terminal.sh \"$PICK_INSTRUCTION\" \"$WAVE_INSTRUCTION\"" &
TERM_PID=$!

window_line=""
for _ in $(seq 1 80); do
    window_line="$(select_window_line "$WINDOW_TITLE" || true)"
    if [[ -n "$window_line" ]]; then
        break
    fi
    sleep 0.25
done

if [[ -z "$window_line" ]]; then
    echo "failed to find demo terminal window '$WINDOW_TITLE'" >&2
    exit 1
fi

window_geometry="$(extract_window_geometry "$window_line" || true)"
if [[ -z "$window_geometry" ]]; then
    echo "failed to parse window geometry from: $window_line" >&2
    exit 1
fi

window_id_hex="$(extract_window_id "$window_line" || true)"
if [[ -z "$window_id_hex" ]]; then
    echo "failed to parse window id from: $window_line" >&2
    exit 1
fi

window_id="$(( window_id_hex ))"

width="${window_geometry%%x*}"
rest="${window_geometry#*x}"
height="${rest%%+*}"
rest="${rest#*+}"
xpos="${rest%%+*}"
ypos="${rest#*+}"

width=$(( width - (width % 2) ))
height=$(( height - (height % 2) ))

echo "recording $OUTPUT_PATH"
echo "window=$WINDOW_TITLE id=${window_id_hex} geometry=${width}x${height}+${xpos}+${ypos}"

ffmpeg \
  -y \
  -framerate "$FPS" \
  -f x11grab \
  -window_id "$window_id" \
  -draw_mouse 0 \
  -i "$DISPLAY_NAME" \
  -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2" \
  -pix_fmt yuv420p \
  -c:v libx264 \
  -preset "$PRESET" \
  -crf "$CRF" \
  "$OUTPUT_PATH" >/tmp/roboclaw-render-demo.log 2>&1 &
FFMPEG_PID=$!

wait "$TERM_PID"
kill -INT "$FFMPEG_PID" >/dev/null 2>&1 || true
wait "$FFMPEG_PID" || true
FFMPEG_PID=""

echo "render complete: $OUTPUT_PATH"
