#!/usr/bin/env bash
# Records through ScreenCaptureKit and ffmpeg and reports clock, crash safety, CPU and frames: sck-probe.sh OUT
set -uo pipefail

out=$1
mkdir -p "$out"
HOMEBREW_NO_AUTO_UPDATE=1 brew install ffmpeg cliclick > /dev/null 2>&1
swiftc -swift-version 5 -O .github/scripts/sck-probe.swift -o "$out/sck-probe" || exit 1
# Runs a command for at most $1 seconds.
limit() { perl -e 'alarm shift; exec @ARGV' "$@"; }
now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }

# CPU shares every 2 s of the capture, the encoder service and the compositor.
sample() {
    while true; do
        ps -A -o %cpu=,comm= | awk -v t="$(date +%T)" '
            /sck-probe/ { s += $1 } /ffmpeg/ { f += $1 } /VTEncoderXPCService/ { v += $1 }
            /replayd/ { r += $1 } /WindowServer/ { w += $1 } { a += $1 }
            END { printf "%s sck %.0f ffmpeg %.0f vtencoder %.0f replayd %.0f WindowServer %.0f all %.0f\n", t, s, f, v, r, w, a }'
        sleep 2
    done
}

# Clicks and moves the pointer, so frames change and click rings show.
wiggle() {
    for i in 1 2 3 4 5; do
        cliclick "m:$((300 + i * 150)),400" w:300 c:. w:700
    done
}

report() {
    local file=$1 first=$2
    local start duration frames
    start=$(limit 30 ffprobe -v error -show_entries format=start_time -of csv=p=0 "$file")
    duration=$(limit 30 ffprobe -v error -show_entries format=duration -of csv=p=0 "$file")
    frames=$(limit 60 ffprobe -v error -select_streams v:0 -count_packets -show_entries stream=nb_read_packets -of csv=p=0 "$file")
    echo "$file: start_time $start (first frame logged at $first, diff $(awk -v a="$start" -v b="$first" 'BEGIN{printf "%.3f", a-b}') s), duration $duration, $frames frames"
    limit 60 ffmpeg -v error -i "$file" -f null - 2>&1 | head -3 | sed 's/^/  decode: /'
    echo "  decode exit ${PIPESTATUS[0]}"
    limit 60 ffmpeg -v error -y -i "$file" -c copy -avoid_negative_ts make_zero "${file%.mov}-remux.mp4" 2>&1 | head -2
    limit 120 ffmpeg -v error -y -i "${file%.mov}-remux.mp4" -vf fps=30 -c:v libx264 -preset veryfast "${file%.mov}-cfr.mp4" 2>&1 | head -2
    echo "  remux+cfr frames: $(ffprobe -v error -select_streams v:0 -count_packets -show_entries stream=nb_read_packets -of csv=p=0 "${file%.mov}-cfr.mp4")"
}

echo "== clean stop with q after 20 s"
sample > "$out/cpu-sck.txt" &
sampler=$!
(sleep 20; echo q) | "$out/sck-probe" "$out/clean.mov" 60 2> "$out/clean.log" &
probe=$!
sleep 5; wiggle; sleep 2; screencapture -x "$out/during.png"
wait "$probe"; echo "probe exit $?"
kill "$sampler"
cat "$out/clean.log"
report "$out/clean.mov" "$(awk '/first frame epoch/ { print $4 }' "$out/clean.log" | tr -d ,)"
mkdir -p "$out/frames"
limit 60 ffmpeg -v error -i "$out/clean-remux.mp4" -vf fps=1 "$out/frames/%02d.png"
# Five frames a second while the pointer clicks, to find click rings.
limit 60 ffmpeg -v error -ss 4 -t 6 -i "$out/clean-remux.mp4" -vf fps=5 "$out/frames/click-%02d.png"

echo "== SIGKILL after 10 s"
sleep 60 | "$out/sck-probe" "$out/killed.mov" 60 2> "$out/killed.log" &
probe=$!
sleep 10
kill -9 "$probe"
echo "killed"
cat "$out/killed.log"
ls -l "$out/killed.mov"
report "$out/killed.mov" "$(awk '/first frame epoch/ { print $4 }' "$out/killed.log" | tr -d ,)"

echo "== ffmpeg avfoundation for 20 s, for CPU"
sample > "$out/cpu-ffmpeg.txt" &
sampler=$!
start=$(now)
ffmpeg -hide_banner -loglevel error -y -use_wallclock_as_timestamps 1 \
    -f avfoundation -capture_cursor 1 -capture_mouse_clicks 1 -pixel_format bgr0 -framerate 30 -i "Capture screen 0" \
    -t 20 -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency "$out/ffmpeg.mkv" < /dev/null &
probe=$!
sleep 5; wiggle
wait "$probe"
kill "$sampler"
echo "ffmpeg started at $start"

for f in sck ffmpeg; do
    echo "== CPU during $f capture (mean over samples)"
    awk '{ for (i = 2; i < NF; i += 2) { sum[$i] += $(i + 1) } n++ } END { for (k in sum) printf "  %s %.0f%%\n", k, sum[k] / n }' "$out/cpu-$f.txt" | sort
done
sysctl -n hw.ncpu machdep.cpu.brand_string
