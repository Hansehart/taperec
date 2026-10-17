#!/usr/bin/env bash
# Ends wall-clock captures with SIGKILL and SIGINT, for MPEG-TS with a fixed clock base and live Matroska, and reports what survived.
set -uo pipefail

out=$1
mkdir -p "$out"
HOMEBREW_NO_AUTO_UPDATE=1 brew install ffmpeg > /dev/null 2>&1
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }
for variant in ts-base mkv-live; do
  for signal in KILL INT; do
    base=$(now)
    case $variant in
      ts-base) file="$out/$variant-$signal.ts"; tail=(-copyts -output_ts_offset "-$base" -f mpegts "$file") ;;
      mkv-live) file="$out/$variant-$signal.mkv"; tail=(-copyts -flush_packets 1 -live 1 -cluster_time_limit 1000 -f matroska "$file") ;;
    esac
    ffmpeg -hide_banner -loglevel error -y -stats_period 0.05 -use_wallclock_as_timestamps 1 \
      -f avfoundation -capture_cursor 1 -capture_mouse_clicks 1 -pixel_format bgr0 -framerate 30 -i "Capture screen 0" \
      -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency "${tail[@]}" < /dev/null &
    pid=$!
    sleep 20
    stopped=$(now)
    kill "-$signal" "$pid"; wait "$pid" 2>/dev/null
    pts=$(ffprobe -v error -select_streams v:0 -show_entries packet=pts_time -of csv=p=0 "$file" 2>&1 | tr -d , | sort -n)
    first=$(echo "$pts" | head -1); last=$(echo "$pts" | tail -1); frames=$(echo "$pts" | grep -c '^[0-9]')
    # On the wall clock a frame's time is its pts plus the base for ts-base, the pts itself for Matroska.
    offset=$([ "$variant" = ts-base ] && echo "$base" || echo 0)
    echo "$variant SIG$signal: $frames frames, first at +$(awk -v a="$first" -v o="$offset" -v b="$base" 'BEGIN{printf "%.2f", a+o-b}') s, last $(awk -v a="$last" -v o="$offset" -v s="$stopped" 'BEGIN{printf "%.2f", s-(a+o)}') s before the stop"
    ffmpeg -hide_banner -loglevel error -y -i "$file" -c copy -avoid_negative_ts make_zero "$out/remux-$variant-$signal.mp4" 2>&1 | head -2 && echo "$variant SIG$signal: remux exit ${PIPESTATUS[0]}"
  done
done
