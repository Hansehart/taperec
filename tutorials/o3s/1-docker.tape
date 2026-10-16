# Section 1: Docker Desktop from the command line, through its first-run window.
Output o3s-1-docker.mp4
Set MoveDuration 700ms
Set TypingSpeed 35ms

Sleep 1.5s
Click dock:Terminal
WaitWindow "sh" 30s
Sleep 1s
Type "curl -fsSLo /tmp/Docker.dmg https://desktop.docker.com/mac/main/amd64/Docker.dmg"
Key return
Warp "Downloading Docker Desktop" 2s
WaitUntil "test -s /tmp/Docker.dmg && ! pgrep -x curl" 4m
EndWarp
Type "sudo hdiutil attach /tmp/Docker.dmg && sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user=$USER && sudo hdiutil detach /Volumes/Docker"
Key return
Warp "Installing Docker Desktop" 2s
WaitUntil "test -x /Applications/Docker.app/Contents/Resources/bin/docker && ! test -d /Volumes/Docker" 4m
EndWarp
Type "open -a Docker"
Key return
Warp "Starting Docker" 2s
WaitUntil "/Applications/Docker.app/Contents/Resources/bin/docker info" 5m
EndWarp
# Docker Desktop greets first-time users: allow local network access and skip the sign-in.
Sleep 5s
Click 1019,409
Sleep 1.5s
Click 1188,343
Sleep 2s
# Hide Docker Desktop and carry on in Terminal.
Key cmd+h
Sleep 1s
Click 180,300
Sleep 1s
Key cmd+k
Type "/Applications/Docker.app/Contents/Resources/bin/docker info --format '{{.ServerVersion}}'"
Key return
Sleep 3s
