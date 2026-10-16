#!/usr/bin/env bash
# Prepares, off camera, what earlier o3s sections show: docker, vscode, clone, in that order.
# Usage: o3s-setup.sh "<stage> ..."
set -euo pipefail

# App launches stall while the Intel image's icon agent crash-loops; switch it off first, as the action does.
source action/main.sh
set -euo pipefail
if [ "$(uname -m)" = x86_64 ]; then icon_agent_off; fi

docker=/Applications/Docker.app/Contents/Resources/bin/docker
code="/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
read -ra stages <<<"$1"
for stage in "${stages[@]}"; do
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
      # Dismiss the first-run window as section 1 does, on the screen size the action sets.
      HOMEBREW_NO_AUTO_UPDATE=1 brew install cliclick displayplacer
      displayplacer "id:$(displayplacer list | awk '/^Persistent screen id:/ { print $4; exit }') res:1920x1080 scaling:off"
      sleep 5
      cliclick c:1019,409 w:1500 c:1188,343 w:2000
      osascript -e 'tell application "System Events" to set visible of process "Docker Desktop" to false' ;;
    vscode)
      HOMEBREW_NO_AUTO_UPDATE=1 brew install --cask visual-studio-code
      # Section 2 shows the first-launch prompt; later sections open VS Code without it.
      xattr -dr com.apple.quarantine "/Applications/Visual Studio Code.app"
      "$code" --install-extension ms-vscode-remote.remote-containers ;;
    clone)
      git clone --depth 1 https://github.com/Hansehart/o3s.git "$HOME/o3s" ;;
  esac
done
