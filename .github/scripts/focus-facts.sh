#!/usr/bin/env bash
# Prints what macOS reports about the frontmost process and the named app, for Terminal, VS Code and Docker Desktop.
set -uo pipefail

limited() { perl -e 'alarm shift; exec @ARGV' "$@"; }
facts() {
  echo "== $1 in front"
  limited 20 osascript -e 'on run argv' -e 'set appName to item 1 of argv' \
    -e 'tell application appName to activate' -e 'delay 3' \
    -e 'set r to "path to application: " & (POSIX path of (path to application appName)) & linefeed' \
    -e 'set r to r & "id of application: " & (id of application appName) & linefeed' \
    -e 'tell application "System Events"' \
    -e 'set p to first process whose frontmost is true' \
    -e 'set r to r & "front name: " & (name of p) & linefeed' \
    -e 'set r to r & "front displayed name: " & (displayed name of p) & linefeed' \
    -e 'set r to r & "front bundle id: " & (bundle identifier of p) & linefeed' \
    -e 'set r to r & "front application file: " & (POSIX path of (application file of p)) & linefeed' \
    -e 'set r to r & "front file class: " & (class of (application file of p) as text) & linefeed' \
    -e 'end tell' -e 'return r' -e 'end run' "$1" 2>&1
}
facts Terminal
facts "Visual Studio Code"
facts Docker
