#!/usr/bin/env bash
# Prepares, off camera, what the parts before the given one show.
# Usage: setup.sh <part>, e.g. setup.sh 3-cage
set -euo pipefail

# App launches stall while the Intel image's icon agent crash-loops; fix it first, as the action does.
source "$(dirname "$0")/../../action/main.sh"
set -euo pipefail
if [ "$(uname -m)" = x86_64 ]; then icon_agent_fix; fi

# Quiet VS Code from its first start: no welcome page, trust prompt or chat panel.
user="$HOME/Library/Application Support/Code/User"
mkdir -p "$user"
cp "$(dirname "$0")/vscode-settings.json" "$user/settings.json"

docker=/Applications/Docker.app/Contents/Resources/bin/docker
code="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
case $1 in
  1-docker) stages=() ;;
  2-vscode) stages=(docker) ;;
  *) stages=(docker vscode clone) ;;
esac
for stage in ${stages[@]+"${stages[@]}"}; do
  case $stage in
    docker)
      # Refreshes the cached image only when Docker publishes a newer one.
      mkdir -p "$HOME/o3s-downloads"
      curl -fsSL -z "$HOME/o3s-downloads/Docker.dmg" -o "$HOME/o3s-downloads/Docker.dmg" https://desktop.docker.com/mac/main/amd64/Docker.dmg
      sudo hdiutil attach -quiet "$HOME/o3s-downloads/Docker.dmg"
      sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user="$USER"
      sudo hdiutil detach -quiet /Volumes/Docker
      # The CLI's start can wrongly report a running Docker Desktop right after installing, so open the app.
      open /Applications/Docker.app
      for _ in $(seq 120); do "$docker" info > /dev/null 2>&1 && break; sleep 5; done
      "$docker" info > /dev/null
      # Hide the welcome window, as part 1 shows.
      osascript -e 'tell application "System Events" to set visible of process "Docker Desktop" to false' ;;
    vscode)
      HOMEBREW_NO_AUTO_UPDATE=1 brew install --cask visual-studio-code
      # Section 2 shows the first-launch prompt; later sections open VS Code without it.
      xattr -dr com.apple.quarantine "/Applications/Visual Studio Code.app"
      # The Marketplace sometimes serves an unreadable signature; a second install succeeds.
      "$code" --install-extension ms-vscode-remote.remote-containers ||
        "$code" --install-extension ms-vscode-remote.remote-containers ;;
    clone)
      git clone --depth 1 https://github.com/Hansehart/o3s.git "$HOME/o3s" ;;
  esac
done
