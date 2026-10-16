#!/usr/bin/env bash
# Launches one app after an IconServices treatment and records, for 90 s, whether it and the Dock come up.
# Usage: launch-probe.sh <app path> <process name> <plain|kickstart-agent|asuser|wait-5m> <out dir>
set -uo pipefail

app=$1 proc=$2 mode=$3 out=$4
mkdir -p "$out"

# Runs a command with a hard time limit; macOS has no timeout(1).
limited() { perl -e 'alarm shift; exec @ARGV' "$@"; }

state() {
  printf '\n### %s (+%ss)\n' "$1" "$SECONDS"
  lsappinfo info -only pid,status "$proc" 2>&1 | head -3
  pgrep -lf 'iconservices' 2>&1
  printf 'windows: '; limited 15 osascript -e "tell application \"System Events\" to count windows of (first process whose name is \"$proc\")" 2>&1
  printf 'dock items: '; limited 15 osascript -e 'tell application "System Events" to count UI elements of list 1 of process "Dock"' 2>&1
  screencapture -x "$out/$1.png"
}

# Samples every IconServices daemon and the app, the ends of the stalled XPC call.
samples() {
  for p in iconservicesagent iconservicesd "$proc"; do
    pid=$(pgrep -x "$p" | head -1)
    [ -n "$pid" ] && sudo perl -e 'alarm 20; exec @ARGV' sample "$pid" 3 -file "$out/sample-$p-$1.txt" >/dev/null 2>&1
  done
}

{ sw_vers; uname -m; uptime; } > "$out/machine.txt"
# Which launchd session this job and the user's icon agent live in.
{
  uid=$(id -u)
  echo "uid $uid, manager $(launchctl managername)"
  ps -axo pid,ppid,user,command | grep -E 'Runner\.(Listener|Worker)|loginwindow|iconservices|Dock$' | grep -v grep
  grep -l -i iconservices /System/Library/LaunchAgents/* /System/Library/LaunchDaemons/* 2>/dev/null
  plutil -p /System/Library/LaunchAgents/com.apple.iconservices.iconservicesagent.plist 2>&1
  launchctl print "gui/$uid" 2>&1 | grep -iE 'icon|session|type|state' | head -20
  launchctl print "gui/$uid/com.apple.iconservices.iconservicesagent" 2>&1 | head -40
  launchctl print "user/$uid" 2>&1 | grep -iE 'icon|type' | head -10
} > "$out/session.txt" 2>&1
sudo ls -la /Library/Caches/com.apple.iconservices.store > "$out/icon-store.txt" 2>&1
state before
samples before
case "$mode" in
  plain) ;;
  kickstart-agent) launchctl kickstart -k "gui/$(id -u)/com.apple.iconservices.iconservicesagent"; sleep 5 ;;
  wait-5m) sleep 300 ;;
esac
state treated
SECONDS=0
if [ "$mode" = asuser ]; then sudo launchctl asuser "$(id -u)" sudo -u "$USER" open -a "$app" & else open -a "$app" & fi
launcher=$!
for t in 10 30 60 90; do
  sleep $((t - SECONDS > 0 ? t - SECONDS : 0))
  state "at-${t}s"
done
if kill -0 "$launcher" 2>/dev/null; then echo "launcher still running after 90 s (pid $launcher)"; fi
samples after

sudo log show --last 10m --style compact \
  --predicate "subsystem == \"com.apple.iconservices\" OR process BEGINSWITH \"iconservices\" OR process == \"$proc\" OR process == \"Dock\"" \
  > "$out/log.txt" 2>&1
grep -E ' (E|F) ' "$out/log.txt" | grep -v XPC_ERROR_CONNECTION_INTERRUPTED | tail -25
