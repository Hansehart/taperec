# Getting started with o3s on a fresh Mac: Docker Desktop, VS Code, and the cage.
Output o3s.mp4
Set MoveDuration 700ms
Set TypingSpeed 35ms

Sleep 1.5s
Click dock:Terminal
WaitWindow "sh" 30s
Sleep 1s

# Docker Desktop, installed from the command line as Docker documents it.
Type "curl -fsSLo /tmp/Docker.dmg https://desktop.docker.com/mac/main/amd64/Docker.dmg"
Key return
Warp "Downloading Docker Desktop" 2s
WaitUntil "test -s /tmp/Docker.dmg && ! pgrep -x curl" 10m
EndWarp
Type "sudo hdiutil attach /tmp/Docker.dmg && sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user=$USER && sudo hdiutil detach /Volumes/Docker"
Key return
Warp "Installing Docker Desktop" 2s
WaitUntil "test -x /Applications/Docker.app/Contents/Resources/bin/docker && ! test -d /Volumes/Docker" 15m
EndWarp
Type "/Applications/Docker.app/Contents/Resources/bin/docker desktop start"
Key return
Warp "Starting Docker" 2s
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker info" 10m
EndWarp
Key cmd+k

# VS Code and the Dev Containers extension.
Type "brew install --cask visual-studio-code"
Key return
Warp "Installing VS Code" 2s
WaitUntil "test -x '/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' && ! pgrep -f 'brew.rb'" 15m
EndWarp
Type "code --install-extension ms-vscode-remote.remote-containers"
Key return
Warp "Installing Dev Containers" 2s
WaitUntil "'/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' --list-extensions | grep -qi remote-containers" 5m
EndWarp
Key cmd+k

# o3s itself.
Type "git clone https://github.com/Hansehart/o3s.git && code o3s"
Key return
WaitWindow "o3s" 3m
Sleep 3s

# Reopen in the cage; the build is long, so it plays fast-forwarded.
Key cmd+shift+p
Sleep 800ms
Type "Dev Containers: Reopen in Container"
Sleep 800ms
Key return
Warp "Building the cage" 5s
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker top \"$(/Applications/Docker.app/Contents/Resources/bin/docker ps -q --filter label=com.docker.compose.service=cage)\" | grep -q vscode-server" 45m
Sleep 15s
EndWarp
Sleep 2s

# Inside the cage only allow-listed hosts answer.
Key ctrl+`
Sleep 4s
Type "curl -sI https://github.com | head -1"
Key return
Sleep 2s
Type "curl -sSI --max-time 5 https://example.com"
Key return
Sleep 4s
