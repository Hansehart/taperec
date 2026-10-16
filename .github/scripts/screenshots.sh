#!/bin/sh
# Saves a screenshot every 10 s, and every 30 s what accessibility sees of the windows, until the job ends.
mkdir -p "$1"
i=0
while true; do
  stamp=$(date +%H%M%S)
  screencapture -x -t jpg "$1/$stamp.jpg"
  if [ $((i % 3)) -eq 0 ]; then
    perl -e 'alarm 25; exec @ARGV' osascript .github/scripts/ui-dump.applescript > "$1/$stamp-ui.txt" 2>&1
  fi
  i=$((i + 1))
  sleep 10
done
