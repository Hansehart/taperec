#!/usr/bin/env bash
# Clicks the empty desktop with a window open, before and after switching off click-to-show-desktop.
set -uo pipefail

out=$1
mkdir -p "$out"
HOMEBREW_NO_AUTO_UPDATE=1 brew install cliclick > /dev/null
source action/main.sh
set +e
icon_agent_off
open -a TextEdit; sleep 8
visible() { perl -e 'alarm 10; exec @ARGV' osascript -e 'tell application "System Events" to get position of window 1 of process "TextEdit"' 2>&1; }
for setting in default off; do
  if [ "$setting" = off ]; then
    defaults write com.apple.WindowManager EnableStandardClickToShowDesktop -bool false
    killall WindowManager 2>/dev/null; sleep 3
  fi
  echo "$setting: window before $(visible)" | tee -a "$out/summary.txt"
  cliclick c:1700,900; sleep 3
  screencapture -x "$out/$setting.png"
  echo "$setting: window after $(visible)" | tee -a "$out/summary.txt"
  # Bring the window back for the next round.
  cliclick c:1700,900; sleep 2; open -a TextEdit; sleep 2
done
