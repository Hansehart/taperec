#!/usr/bin/env bash
# Checks a recorded video: 1920x1080, longer than a second, opening on a lit frame.
set -euo pipefail

video=${1:?usage: verify-video.sh <video.mp4>}
size=$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0 "$video")
seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$video")
luma=$(ffmpeg -v error -i "$video" -frames:v 1 \
    -vf signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=- -f null - |
    sed -n 's/.*YAVG=//p' | head -n 1)
echo "$video: $size, ${seconds}s, first-frame luma $luma"
[ "$size" = "1920,1080" ] || { echo "::error::$video is $size, expected 1920,1080"; exit 1; }
awk -v s="$seconds" 'BEGIN { exit !(s > 1) }' || { echo "::error::$video lasts ${seconds}s"; exit 1; }
awk -v l="${luma:-0}" 'BEGIN { exit !(l > 30) }' || { echo "::error::$video opens on a dark frame"; exit 1; }
