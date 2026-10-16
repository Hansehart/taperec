# Presses Open on the re-armed first-launch prompt in five ways, recording each outcome.
Output gatekeeper-probe.mp4
Set MoveDuration 700ms

WaitUntil ".github/scripts/gk.sh arm" 2m
Click 1019,447
WaitUntil ".github/scripts/gk.sh result taperec-click" 30s

WaitUntil ".github/scripts/gk.sh arm" 2m
WaitUntil "cliclick m:1019,447 && cliclick c:." 10s
WaitUntil ".github/scripts/gk.sh result move-then-click-separately" 30s

WaitUntil ".github/scripts/gk.sh arm" 2m
WaitUntil "cliclick m:1019,447 c:." 10s
WaitUntil ".github/scripts/gk.sh result move-and-click-at-once" 30s

WaitUntil ".github/scripts/gk.sh arm" 2m
WaitUntil "cliclick m:1019,447 w:300 c:." 10s
WaitUntil ".github/scripts/gk.sh result move-pause-click" 30s

WaitUntil ".github/scripts/gk.sh arm" 2m
WaitUntil "cliclick m:1019,447 && sleep 0.3 && cliclick c:. w:50" 10s
WaitUntil ".github/scripts/gk.sh result pause-click-settle" 30s
