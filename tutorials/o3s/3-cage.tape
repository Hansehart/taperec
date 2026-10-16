# Section 3: reopen o3s in its cage and show which hosts answer, with Docker, VS Code and o3s ready.
Output o3s-3-cage.mp4
Set MoveDuration 700ms
Set TypingSpeed 35ms

Sleep 1.5s
WaitUntil "'/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' $HOME/o3s" 1m
WaitWindow "o3s" 2m
Sleep 3s
Key cmd+shift+p
Sleep 800ms
Type "Dev Containers: Reopen in Container"
Sleep 800ms
Key return
Warp "Building the cage" 5s
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker top \"$(/Applications/Docker.app/Contents/Resources/bin/docker ps -q --filter label=com.docker.compose.service=cage)\" | grep -q vscode-server" 30m
# VS Code finishes connecting a while after its server starts in the cage.
Sleep 3m
EndWarp
Sleep 2s
Key ctrl+`
Sleep 4s
Type "curl -sI https://github.com | head -1"
Key return
Sleep 2s
Type "curl -sSI --max-time 5 https://example.com"
Key return
Sleep 4s
