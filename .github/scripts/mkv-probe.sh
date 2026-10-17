#!/usr/bin/env bash
# Records into Matroska with wall-clock timestamps, ends one capture with SIGKILL and one with SIGINT, and reports what survived.
set -uo pipefail

out=$1
mkdir -p "$out"
HOMEBREW_NO_AUTO_UPDATE=1 brew install ffmpeg > /dev/null 2>&1
for signal in KILL INT; do
  file="$out/capture-$signal.mkv"
  started=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
  ffmpeg -hide_banner -loglevel error -y -stats_period 0.05 -use_wallclock_as_timestamps 1 \
    -f avfoundation -capture_cursor 1 -capture_mouse_clicks 1 -pixel_format bgr0 -framerate 30 -i "Capture screen 0" \
    -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency -copyts -f matroska "$file" < /dev/null &
  pid=$!
  sleep 20
  stopped=$(perl -MTime::HiRes=time -e 'printf "%.3f", time')
  kill "-$signal" "$pid"; wait "$pid" 2>/dev/null
  pts=$(ffprobe -v error -select_streams v:0 -show_entries packet=pts_time -of csv=p=0 "$file" 2>&1 | tr -d , | sort -n)
  first=$(echo "$pts" | head -1); last=$(echo "$pts" | tail -1); frames=$(echo "$pts" | grep -c .)
  echo "SIG$signal: $frames frames, first $(awk -v a="$first" -v b="$started" 'BEGIN{printf "%.2f", a-b}') s after start, last $(awk -v a="$last" -v b="$stopped" 'BEGIN{printf "%.2f", b-a}') s before the stop, $(ffprobe -v error -show_entries format=duration -of csv=p=0 "$file" 2>&1) s duration"
  ffmpeg -hide_banner -loglevel error -y -i "$file" -c copy -avoid_negative_ts make_zero "$out/remux-$signal.mp4" && echo "SIG$signal: remuxes cleanly" || echo "SIG$signal: remux failed"
done
