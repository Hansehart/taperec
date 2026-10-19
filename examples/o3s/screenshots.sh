#!/bin/sh
# Saves a screenshot and the memory state every 10 s into a directory until the job ends.
mkdir -p "$1"
while true; do
    stamp=$(date +%H%M%S)
    screencapture -x -t jpg "$1/$stamp.jpg"
    {
        echo "== $stamp"
        memory_pressure -Q 2>&1 | tail -1
        vm_stat | grep -E 'Pages (free|active|inactive|wired|occupied by compressor)|Swapouts'
        ps -A -o %cpu=,rss=,comm= | sort -rn | head -5
    } >>"$1/memory.txt"
    sleep 10
done
