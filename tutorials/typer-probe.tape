# Records while the typer types the probe lines, so typing runs under a real recording.
Output typer-probe.mp4
Sleep 1s
WaitUntil ".github/scripts/typer-probe.sh $TYPER_HOLD" 10m
Sleep 2s
