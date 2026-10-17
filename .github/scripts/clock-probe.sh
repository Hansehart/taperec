#!/usr/bin/env bash
# Records a minute with wall-clock frame timestamps into MPEG-TS and Matroska, and compares them with the clock at start.
set -uo pipefail

out=$1
mkdir -p "$out"
HOMEBREW_NO_AUTO_UPDATE=1 brew install ffmpeg > /dev/null 2>&1
for container in mpegts matroska; do
  file="$out/capture.$container"
  started=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
  ffmpeg -hide_banner -loglevel error -y -use_wallclock_as_timestamps 1 \
    -f avfoundation -capture_cursor 1 -capture_mouse_clicks 1 -pixel_format bgr0 -framerate 30 -i "Capture screen 0" \
    -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency -copyts -f "$container" "$file" < /dev/null &
  pid=$!
  sleep 60
  kill -INT "$pid"; wait "$pid"
  first=$(ffprobe -v error -select_streams v:0 -show_entries packet=pts_time -of csv=p=0 "$file" | sort -n | head -1 | tr -d ,)
  last=$(ffprobe -v error -select_streams v:0 -show_entries packet=pts_time -of csv=p=0 "$file" | sort -n | tail -1 | tr -d ,)
  echo "$container: ffmpeg started at $started, first frame pts $first, last $last, first frame $(awk -v a="$first" -v b="$started" 'BEGIN { printf "%.3f", a - b }') s after start"
done
