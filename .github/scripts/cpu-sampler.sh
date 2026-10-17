#!/bin/sh
# Samples the CPU share of ffmpeg, WindowServer and everything every 5 s until the job ends.
while true; do
  ps -A -o %cpu=,comm= | awk -v t="$(date +%T)" '
    /ffmpeg/ { f += $1 } /WindowServer/ { w += $1 } { a += $1 }
    END { printf "%s ffmpeg %.0f WindowServer %.0f all %.0f\n", t, f, w, a }' >> "$1"
  sleep 5
done
