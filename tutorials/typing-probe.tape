# Types the o3s draft's long commands into a file while recording, to count lost characters.
Output typing-probe.mp4
Set MoveDuration 700ms
Set TypingSpeed 35ms

Sleep 1.5s
Click dock:Terminal
WaitWindow "sh" 30s
Sleep 1s
Type "cat > /tmp/typed.txt"
Key return
Sleep 1s
Type "curl -fsSLo /tmp/Docker.dmg https://desktop.docker.com/mac/main/amd64/Docker.dmg"
Key return
Type "sudo hdiutil attach /tmp/Docker.dmg && sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user=$USER && sudo hdiutil detach /Volumes/Docker"
Key return
Type "curl -fsSLo /tmp/Docker.dmg https://desktop.docker.com/mac/main/amd64/Docker.dmg"
Key return
Type "sudo hdiutil attach /tmp/Docker.dmg && sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user=$USER && sudo hdiutil detach /Volumes/Docker"
Key return
Type "curl -fsSLo /tmp/Docker.dmg https://desktop.docker.com/mac/main/amd64/Docker.dmg"
Key return
Type "sudo hdiutil attach /tmp/Docker.dmg && sudo /Volumes/Docker/Docker.app/Contents/MacOS/install --accept-license --user=$USER && sudo hdiutil detach /Volumes/Docker"
Key return
Type "brew install --cask visual-studio-code"
Key return
Type "code --install-extension ms-vscode-remote.remote-containers"
Key return
Type "git clone https://github.com/Hansehart/o3s.git && code o3s"
Key return
Type "curl -sSI --max-time 5 https://example.com"
Key return
Sleep 2s
