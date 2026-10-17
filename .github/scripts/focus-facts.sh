#!/usr/bin/env bash
# Runs taperec's Focus script for Terminal, TextEdit and Docker, polled as taperec does.
set -uo pipefail
source action/main.sh
set +e
icon_agent_off
open -a Terminal; open -a TextEdit; sleep 8
cat > /tmp/focus.applescript <<'SCRIPT'
on run argv
set appName to item 1 of argv
tell application appName to activate
set wanted to id of application appName
tell application "System Events"
set p to first process whose frontmost is true
set {frontId, frontPath} to {bundle identifier of p, POSIX path of (application file of p)}
end tell
return wanted is frontId or frontPath contains "/" & appName & ".app/"
end run
SCRIPT
# Taperec polls the script, since activation completes a moment later.
for app in Terminal Docker "Docker Desktop" Terminal "Docker Desktop"; do
  for attempt in 1 2 3 4 5; do
    started=$SECONDS
    out=$(perl -e "alarm 15; exec @ARGV" osascript /tmp/focus.applescript "$app" 2>&1); code=$?
    echo "focus $app attempt $attempt: exit $code after $((SECONDS - started))s: $out"
    [ "$out" = true ] && break
    perl -e "alarm 10; exec @ARGV" osascript -e 'tell application "System Events" to tell (first process whose frontmost is true) to return (name as text) & " | " & (bundle identifier as text) & " | " & (POSIX path of (application file as alias))' 2>&1 | sed 's/^/  front: /'
    sleep 1
  done
done
