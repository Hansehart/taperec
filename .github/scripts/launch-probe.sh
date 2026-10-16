#!/usr/bin/env bash
# Launches one app in the background and records, for 90 s, whether it comes up and what else stalls.
# Usage: launch-probe.sh <app path> <process name> <open|binary|after-dock> <out dir>
set -uo pipefail

app=$1 proc=$2 mode=$3 out=$4
mkdir -p "$out"

# Runs a command with a hard time limit; macOS has no timeout(1).
limited() { perl -e 'alarm shift; exec @ARGV' "$@"; }

state() {
  printf '\n### %s (+%ss)\n' "$1" "$SECONDS"
  lsappinfo info -only pid,status "$proc" 2>&1 | head -3
  pgrep -lf "$proc" 2>&1 | head -5
  printf 'windows: '; limited 15 osascript -e "tell application \"System Events\" to count windows of (first process whose name is \"$proc\")" 2>&1
  printf 'dock items: '; limited 15 osascript -e 'tell application "System Events" to count UI elements of list 1 of process "Dock"' 2>&1
  screencapture -x "$out/$1.png"
}

sw_vers > "$out/machine.txt"; uname -m >> "$out/machine.txt"
state before
SECONDS=0
case "$mode" in
  open) open -a "$app" & ;;
  binary) "$app/Contents/MacOS/$(defaults read "$app/Contents/Info" CFBundleExecutable)" >/dev/null 2>&1 & ;;
  after-dock) killall Dock; sleep 5; state after-dock-restart; open -a "$app" & ;;
esac
launcher=$!
for t in 10 30 60 90; do
  sleep $((t - SECONDS > 0 ? t - SECONDS : 0))
  state "at-${t}s"
done
if kill -0 "$launcher" 2>/dev/null; then echo "launcher still running after 90 s (pid $launcher)"; fi

pid=$(pgrep -x "$proc" | head -1)
[ -n "$pid" ] && limited 20 sample "$pid" 5 -file "$out/sample.txt" >/dev/null 2>&1
log show --last 4m --style compact \
  --predicate "process == \"$proc\" OR process == \"launchservicesd\" OR process == \"runningboardd\" OR process == \"Dock\"" \
  > "$out/log.txt" 2>&1
grep -E ' (E|F) ' "$out/log.txt" | grep -v XPC_ERROR_CONNECTION_INTERRUPTED | tail -25
