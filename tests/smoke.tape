# Plays Dock clicks, focus, windows, typing, pasting, keys, waits and a warp on a hosted Mac.
Output smoke.mp4
Set MoveDuration 600ms
Set TypingSpeed 40ms

Sleep 1s
Click dock:Terminal
WaitWindow "sh" 30s
Focus "Terminal"
Sleep 1s
# Fails the tape when a lone Return goes missing.
Paste "touch /tmp/taperec-return"
Key return
WaitUntil "test -f /tmp/taperec-return" 10s
# Fails the tape when pasted text goes missing.
Paste "echo the quick brown fox jumps over 0123456789 lazy dogs > /tmp/taperec-typed"
Key return
WaitUntil "grep -qx 'the quick brown fox jumps over 0123456789 lazy dogs' /tmp/taperec-typed" 10s
Type "echo taperec smoke"
Key return
Warp "Waiting" 1s
WaitUntil "sleep 3" 30s
EndWarp
Key cmd+k
Type "echo done"
Key return
Sleep 1s
