#!/usr/bin/env bash
# Types the o3s commands key by key, as taperec does, while one kind of load runs, and counts what got lost.
# Usage: keyloss-probe.sh <session|hid|hid-hold> <out dir>
set -uo pipefail

method=$1 out=$2
mkdir -p "$out"
source action/main.sh
set +e
icon_agent_off
HOMEBREW_NO_AUTO_UPDATE=1 brew install cliclick ffmpeg > /dev/null 2>&1
swiftc -O .github/scripts/typer.swift -o "$RUNNER_TEMP/typer"
say() { echo "[$(date +%T)] $*"; }

lines=()
while IFS= read -r line; do lines+=("$line"); done < <(sed -n 's/^Type "\(.*\)"$/\1/p' tutorials/typing-probe.tape | tail -n +2 | sed 's/\\"/"/g')

# Taperec's typing: one cliclick run per character, paced at 35 ms.
type_line() {
  local text=$1 i
  for ((i = 0; i < ${#text}; i++)); do cliclick "t:${text:i:1}" w:20; sleep 0.035; done
  cliclick kp:return w:20
}

open -a Terminal; sleep 10
for _ in $(seq 5); do
  pgrep -x cat > /dev/null && break
  cliclick kd:ctrl t:c ku:ctrl w:200
  type_line "cat > /tmp/typed.txt"
  sleep 2
done
pgrep -x cat > /dev/null || { say "cat never started"; exit 1; }

# Taperec's recording runs throughout, as in a real tape.
case record in
  record) ffmpeg -loglevel error -f avfoundation -capture_cursor 1 -capture_mouse_clicks 1 -pixel_format bgr0 -framerate 30 -i "Capture screen 0" \
            -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -f mpegts -y /tmp/load.ts & ;;
  capture) ffmpeg -loglevel error -f avfoundation -capture_cursor 1 -capture_mouse_clicks 1 -pixel_format bgr0 -framerate 30 -i "Capture screen 0" \
            -f null - & ;;
  encode) ffmpeg -loglevel error -re -f lavfi -i testsrc2=s=1920x1080:r=30 \
            -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -f mpegts -y /tmp/load.ts & ;;
esac
sleep 5
say "$method: typing ${#lines[@]} lines ten times"
expected=$(mktemp)
for _ in $(seq 10); do printf '%s\n' "${lines[@]}" >> "$expected"; done
case $method in
  session) "$RUNNER_TEMP/typer" session 0 35 < "$expected" ;;
  hid) "$RUNNER_TEMP/typer" hid 0 35 < "$expected" ;;
  hid-hold) "$RUNNER_TEMP/typer" hid 15 20 < "$expected" ;;
esac
sleep 3
pkill -x cat; pkill -x ffmpeg
typed=$(wc -c < "$expected"); got=$(wc -c < /tmp/typed.txt)
bad=$(diff "$expected" /tmp/typed.txt | grep -c '^>')
diff "$expected" /tmp/typed.txt > "$out/diff.txt"
say "$method: $bad of $(wc -l < "$expected") lines damaged, $((typed - got)) of $typed characters missing" | tee "$out/summary.txt"
