#!/usr/bin/env bash
# Records a static screen for three minutes with taperec's exact capture settings and reports its timestamps.
set -uo pipefail

out=$1
mkdir -p "$out"
HOMEBREW_NO_AUTO_UPDATE=1 brew install ffmpeg > /dev/null 2>&1
boot=$(sysctl -n kern.boottime | sed -n 's/.*sec = \([0-9]*\).*/\1/p')
echo "uptime at start: $(( $(date +%s) - boot )) s; MPEG-TS timestamps wrap every 95443 s"

# The arguments of recorder::ffmpeg_args at 30 fps.
ffmpeg -hide_banner -loglevel error -y -stats_period 0.05 \
  -f avfoundation -capture_cursor 1 -capture_mouse_clicks 1 -pixel_format bgr0 -framerate 30 -i "Capture screen 0" \
  -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency -f mpegts "$out/capture.ts" < /dev/null &
pid=$!
sleep 180
kill -INT "$pid"; wait "$pid"

ffprobe -v error -show_entries format=start_time,duration -of default=nw=1 "$out/capture.ts"
ffprobe -v error -select_streams v:0 -show_entries packet=pts_time -of csv=p=0 "$out/capture.ts" > "$out/pts.txt"
echo "packets: $(wc -l < "$out/pts.txt"), first pts: $(head -1 "$out/pts.txt"), last pts: $(tail -1 "$out/pts.txt")"
sort -n "$out/pts.txt" | awk 'NR > 1 { gap = $1 - prev; if (gap > max) { max = gap; at = prev } } { prev = $1 } END { printf "largest gap between frames: %.3f s after pts %s\n", max, at }'
# The remux and probe taperec's finish() does.
ffmpeg -hide_banner -loglevel error -y -i "$out/capture.ts" -c copy -avoid_negative_ts make_zero -movflags +faststart "$out/zeroed.mp4"
echo "remuxed duration: $(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out/zeroed.mp4") s for 180 s of wall time"
