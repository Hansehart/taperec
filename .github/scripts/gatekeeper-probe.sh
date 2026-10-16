#!/usr/bin/env bash
# Opens a freshly downloaded VS Code and tries each way of pressing Open on macOS's first-launch prompt.
set -uo pipefail

out=$1
mkdir -p "$out"
source action/main.sh
set +e
icon_agent_off
HOMEBREW_NO_AUTO_UPDATE=1 brew install cliclick displayplacer
HOMEBREW_NO_AUTO_UPDATE=1 brew install --cask visual-studio-code
displayplacer "id:$(displayplacer list | awk '/^Persistent screen id:/ { print $4; exit }') res:1920x1080 scaling:off"

shot() { sleep 3; screencapture -x "$out/$1.png"; osascript .github/scripts/ui-dump.applescript > "$out/$1-ui.txt" 2>&1; }
dialog_open() { osascript -e 'tell application "System Events" to exists window 1 of process "CoreServicesUIAgent"' 2>/dev/null | grep -q true; }
vscode_up() { pgrep -f 'Visual Studio Code.app/Contents/MacOS' > /dev/null; }
report() { if dialog_open; then echo "$1: dialog still open"; else echo "$1: dialog gone, VS Code running: $(vscode_up && echo yes || echo no)"; fi | tee -a "$out/summary.txt"; }

for attempt in mouse ax-press keyboard; do
  pkill -f 'Visual Studio Code.app/Contents/MacOS'; sleep 2
  open -a "Visual Studio Code"
  for _ in $(seq 30); do dialog_open && break; sleep 1; done
  shot "$attempt-before"
  case $attempt in
    mouse)
      pos=$(osascript -e 'tell application "System Events" to tell button "Open" of window 1 of process "CoreServicesUIAgent" to get {position, size}' 2>&1)
      echo "Open button: $pos" | tee -a "$out/summary.txt"
      read -r x y w h <<<"$(echo "$pos" | tr -d ',' )"
      cliclick "m:$((x + w / 2)),$((y + h / 2))" w:300 c:. ;;
    ax-press)
      osascript -e 'tell application "System Events" to click button "Open" of window 1 of process "CoreServicesUIAgent"' 2>&1 | tee -a "$out/summary.txt" ;;
    keyboard)
      osascript -e 'tell application "System Events" to set frontmost of process "CoreServicesUIAgent" to true' 2>&1
      cliclick kp:tab w:300 kp:space w:300 ;;
  esac
  shot "$attempt-after"
  report "$attempt"
done
