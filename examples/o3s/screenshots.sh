#!/bin/sh
# Saves a screenshot every 10 s into a directory until the job ends, showing what happened between steps.
mkdir -p "$1"
while true; do
    screencapture -x -t jpg "$1/$(date +%H%M%S).jpg"
    sleep 10
done
