#!/usr/bin/env bash
# Arms VS Code's first-launch prompt and reports whether a press got through.
# Usage: gk.sh arm | gk.sh result <variant>
set -uo pipefail

app="/Applications/Visual Studio Code.app"
limited() { perl -e 'alarm shift; exec @ARGV' "$@"; }
prompt_open() { limited 10 osascript -e 'tell application "System Events" to exists window 1 of process "CoreServicesUIAgent"' 2>/dev/null | grep -q true; }

case $1 in
  arm)
    pkill -f "$app/Contents/MacOS"; sleep 2
    # A fresh quarantine flag without the approved bit brings the prompt back.
    xattr -w com.apple.quarantine "0081;$(printf %x "$(date +%s)");Homebrew Cask;" "$app"
    limited 30 open -a "Visual Studio Code"
    for _ in $(seq 30); do prompt_open && { sleep 2; exit 0; }; sleep 1; done
    echo "prompt did not appear" >> /tmp/gk-summary.txt
    exit 0 ;;
  result)
    sleep 3
    if prompt_open; then r="prompt still open"; else r="prompt gone, VS Code $(pgrep -f "$app/Contents/MacOS" > /dev/null && echo running || echo not running)"; fi
    echo "$2: $r" | tee -a /tmp/gk-summary.txt
    screencapture -x "/tmp/gk-$2.png" ;;
esac
