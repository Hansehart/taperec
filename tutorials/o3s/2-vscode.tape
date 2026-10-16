# Section 2: VS Code, the Dev Containers extension and o3s, with Docker already running.
Output o3s-2-vscode.mp4
Set MoveDuration 700ms
Set TypingSpeed 35ms

Sleep 1.5s
Click dock:Terminal
WaitWindow "sh" 30s
Sleep 1s
Type "brew install --cask visual-studio-code"
Key return
Warp "Installing VS Code" 2s
WaitUntil "test -x '/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' && ! pgrep -f 'brew.rb'" 4m
EndWarp
Type "code --install-extension ms-vscode-remote.remote-containers"
Key return
Warp "Installing Dev Containers" 2s
WaitUntil "'/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code' --list-extensions | grep -qi remote-containers" 3m
EndWarp
Key cmd+k
Type "git clone https://github.com/Hansehart/o3s.git && code o3s"
Key return
# macOS asks once before opening an app downloaded from the internet.
WaitUntil "osascript -e 'tell application \"System Events\" to exists window 1 of process \"CoreServicesUIAgent\"' | grep -q true" 2m
Sleep 1.5s
# The first click brings the dialog forward, the second presses Open.
Click 1019,447
Sleep 1s
Click 1019,447
WaitWindow "o3s" 2m
Sleep 3s
