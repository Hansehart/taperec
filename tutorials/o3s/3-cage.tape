# Section 3: reopen o3s in its cage and show which hosts answer, with Docker, VS Code and o3s ready.
Output o3s-3-cage.mp4
Set MoveDuration 700ms
Set TypingSpeed 35ms

Sleep 1.5s
WaitUntil "'/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' $HOME/o3s" 1m
WaitWindow "o3s" 2m
Focus "Visual Studio Code"
Sleep 3s
Key cmd+shift+p
Sleep 800ms
Paste "Dev Containers: Reopen in Container"
Sleep 800ms
Key return
Warp "Building the cage" 5s
# VS Code is connected once its extension host runs inside the cage.
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker exec \"$(/Applications/Docker.app/Contents/Resources/bin/docker ps -q --filter label=com.docker.compose.service=cage)\" pgrep -f -- --type=extensionHost" 30m
EndWarp
Sleep 2s
Key ctrl+`
Sleep 4s
Paste "curl -sI https://github.com | head -1"
Key return
Sleep 2s
Paste "curl -sSI --max-time 5 https://example.com"
Key return
Sleep 4s
