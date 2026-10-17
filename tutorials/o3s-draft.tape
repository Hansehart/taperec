# Getting started with o3s on a fresh Mac: Docker Desktop, VS Code, and the cage.
Output o3s.mp4
Set MoveDuration 700ms
Set TypingSpeed 35ms

Sleep 1.5s
Click dock:Terminal
WaitWindow "sh" 30s
Focus "Terminal"
Sleep 1s

# Docker Desktop, installed from the command line as Docker documents it.
Paste "curl -fsSLo /tmp/Docker.dmg https://desktop.docker.com/mac/main/amd64/Docker.dmg"
Key return
Warp "Downloading Docker Desktop" 2s
WaitUntil "test -s /tmp/Docker.dmg && ! pgrep -x curl" 10m
EndWarp
Paste "sudo hdiutil attach /tmp/Docker.dmg && sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user=$USER && sudo hdiutil detach /Volumes/Docker"
Key return
Warp "Installing Docker Desktop" 2s
WaitUntil "test -x /Applications/Docker.app/Contents/Resources/bin/docker && ! test -d /Volumes/Docker" 10m
EndWarp
Paste "open /Applications/Docker.app"
Key return
Warp "Starting Docker" 2s
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker info" 10m
EndWarp
# Docker Desktop asks to find devices on the local network.
Click button:"Allow" 2m
Sleep 1.5s
# Hide its welcome window and carry on in Terminal.
Focus "Docker Desktop"
Key cmd+h
Focus "Terminal"
Sleep 1s
Key cmd+k

# VS Code and the Dev Containers extension.
Paste "brew install --cask visual-studio-code"
Key return
Warp "Installing VS Code" 2s
WaitUntil "test -x '/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' && ! pgrep -f 'brew.rb'" 10m
EndWarp
Paste "code --install-extension ms-vscode-remote.remote-containers"
Key return
Warp "Installing Dev Containers" 2s
WaitUntil "'/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' --list-extensions | grep -qi remote-containers" 5m
EndWarp
Key cmd+k

# o3s itself; macOS asks once before opening an app downloaded from the internet.
Paste "git clone https://github.com/Hansehart/o3s.git && code o3s"
Key return
Click button:"Open" 3m
WaitWindow "o3s" 3m
Focus "Visual Studio Code"
Sleep 3s

# Reopen in the cage; the build is long, so it plays fast-forwarded.
Key cmd+shift+p
Sleep 800ms
Paste "Dev Containers: Reopen in Container"
Sleep 800ms
Key return
Warp "Building the cage" 5s
# VS Code is connected once its extension host runs inside the cage.
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker exec \"$(/Applications/Docker.app/Contents/Resources/bin/docker ps -q --filter label=com.docker.compose.service=cage)\" pgrep -f -- --type=extensionHost" 50m
EndWarp
Sleep 2s

# Inside the cage only allow-listed hosts answer.
Key ctrl+`
# The terminal is ready once its shell runs inside the cage.
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker exec \"$(/Applications/Docker.app/Contents/Resources/bin/docker ps -q --filter label=com.docker.compose.service=cage)\" pgrep -f -- '--init-file .*shellIntegration'" 2m
Sleep 1s
Paste "curl -sI https://github.com | head -1"
Key return
Sleep 2s
Paste "curl -sSI --max-time 5 https://example.com"
Key return
Sleep 4s
