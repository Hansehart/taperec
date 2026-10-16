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
      curl -fsSLo "$RUNNER_TEMP/Docker.dmg" https://desktop.docker.com/mac/main/amd64/Docker.dmg
      sudo hdiutil attach -quiet "$RUNNER_TEMP/Docker.dmg"
      sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user="$USER"
      sudo hdiutil detach -quiet /Volumes/Docker
      # The CLI's start can wrongly report a running Docker Desktop right after installing, so open the app.
      open /Applications/Docker.app
      for _ in $(seq 120); do "$docker" info > /dev/null 2>&1 && break; sleep 5; done
      "$docker" info > /dev/null ;;
    vscode)
      HOMEBREW_NO_AUTO_UPDATE=1 brew install --cask visual-studio-code
      # Section 2 shows the first-launch prompt; later sections open VS Code without it.
      xattr -dr com.apple.quarantine "/Applications/Visual Studio Code.app"
      "$code" --install-extension ms-vscode-remote.remote-containers ;;
    clone)
      git clone --depth 1 https://github.com/Hansehart/o3s.git "$HOME/o3s" ;;
  esac
done
