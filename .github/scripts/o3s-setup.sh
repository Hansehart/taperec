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
      "$docker" desktop start --timeout 600 ;;
    vscode)
      HOMEBREW_NO_AUTO_UPDATE=1 brew install --cask --no-quarantine visual-studio-code
      "$code" --install-extension ms-vscode-remote.remote-containers ;;
    clone)
      git clone --depth 1 https://github.com/Hansehart/o3s.git "$HOME/o3s" ;;
  esac
done
