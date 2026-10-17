#!/usr/bin/env bash
# Runs each AppleScript statement Focus could use on its own, ten seconds each, to see which ones block.
set -uo pipefail

source action/main.sh
set +e
icon_agent_off
open -a Terminal; open -a TextEdit; sleep 8
try() {
  local started=$SECONDS out
  out=$(perl -e 'alarm 10; exec @ARGV' osascript -e "$2" 2>&1); local code=$?
  printf '%-34s exit %3s after %2ss: %s\n' "$1" "$code" "$((SECONDS - started))" "$out"
}
for app in Terminal TextEdit; do
  echo "== $app"
  try "activate" "tell application \"$app\" to activate"
  try "path to application" "POSIX path of (path to application \"$app\")"
  try "id of application" "id of application \"$app\""
  try "System Events activate (frontmost)" "tell application \"System Events\" to set frontmost of process \"$app\" to true"
  sleep 1
  try "front process name" "tell application \"System Events\" to name of first process whose frontmost is true"
  try "front bundle id" "tell application \"System Events\" to bundle identifier of first process whose frontmost is true"
  try "front application file" "tell application \"System Events\" to POSIX path of (application file of first process whose frontmost is true)"
done
