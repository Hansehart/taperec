#!/usr/bin/env bash
# Types one line 15 times per method into Terminal while the machine is busy, and counts lines that arrive damaged.
set -uo pipefail

out=$1
mkdir -p "$out"
line="the quick brown fox jumps over 0123456789 lazy dogs curl amd64 --accept-license"
trials=15

source action/main.sh
if [ "$(uname -m)" = x86_64 ]; then icon_agent_off; fi
swiftc -O .github/scripts/typer.swift -o "$RUNNER_TEMP/typer"

# Busy every core and the network the way an install does.
for _ in $(seq "$(sysctl -n hw.ncpu)"); do yes > /dev/null & done
( while true; do curl -s -o /dev/null https://desktop.docker.com/mac/main/amd64/Docker.dmg; done ) &

open -a Terminal
sleep 15

type_line() {
  local method=$1 i c
  case $method in
    per-char-w20)
      for ((i = 0; i < ${#line}; i++)); do cliclick "t:${line:i:1}" w:20; sleep 0.035; done
      cliclick kp:return w:20 ;;
    per-char-w100)
      for ((i = 0; i < ${#line}; i++)); do cliclick "t:${line:i:1}" w:100; sleep 0.035; done
      cliclick kp:return w:100 ;;
    one-call)
      cliclick "t:$line" w:100 kp:return w:100 ;;
    osascript)
      osascript -e 'on run argv' -e 'tell application "System Events" to keystroke (item 1 of argv)' \
        -e 'tell application "System Events" to key code 36' -e 'end run' "$line" ;;
    hid-tap)
      "$RUNNER_TEMP/typer" "$line" ;;
  esac
}

for method in per-char-w20 per-char-w100 one-call osascript hid-tap; do
  file="$out/typed-$method.txt"
  osascript -e "tell application \"Terminal\" to do script \"cat > $file\" in window 1" >/dev/null
  osascript -e 'tell application "Terminal" to activate'
  sleep 3
  SECONDS=0
  for _ in $(seq "$trials"); do type_line "$method"; sleep 0.5; done
  sleep 2
  pkill -x cat
  bad=$(grep -cvxF "$line" "$file")
  printf '%-15s %2d of %d lines damaged, %3ss\n' "$method" "$bad" "$trials" "$SECONDS" | tee -a "$out/summary.txt"
  sleep 1
done
screencapture -x "$out/screen.png"
