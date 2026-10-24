#!/usr/bin/env bash
# Renders a synthetic capture with an edit file on any machine with ffmpeg: a text and effects announced in
# the centre and settled in their corners, a shortcut pill, and the zoom itself, each checked in the frames.
set -euo pipefail

taperec=${1:?usage: render.sh <taperec> <work-dir>}
work=${2:?usage: render.sh <taperec> <work-dir>}
mkdir -p "$work"

# Ten seconds whose first frame carries 100 s on the capture clock.
ffmpeg -v error -y -f lavfi -i testsrc2=size=1920x1080:rate=30 -t 10 -c:v libx264 -pix_fmt yuv420p \
    -movflags use_metadata_tags -metadata com.taperec.start=100 "$work/t.raw.mov"
cat > "$work/t.timeline.yaml" <<'EOF'
video: t.raw.mov
warps: []
marks:
  - { name: start, at: 100 }
  - { name: shown, at: 101 }
  - { name: results, at: 103, x: 400, y: 300 }
  - { name: waiting, at: 104 }
  - { name: waited, at: 108 }
  - { name: end, at: 110 }
keys:
  - { at: 102, labels: ["⇧", "⌘", "P"] }
  - { at: 109, labels: ["⌘", "V"] }
EOF
cat > "$work/t.edit.yaml" <<'EOF'
tracks:
  text:
    - { text: Rendered anywhere, from: shown, until: waiting }
  speed:
    - { label: Waiting, length: 3s, from: waiting, until: waited }
  zoom:
    - { at: results, scale: 2, from: results, until: waiting }
EOF
sed '/^  zoom:/,$d' "$work/t.edit.yaml" > "$work/nozoom.edit.yaml"

"$taperec" render "$work/t.timeline.yaml" --edit "$work/t.edit.yaml" --out "$work/t.mp4"
"$taperec" render "$work/t.timeline.yaml" --edit "$work/nozoom.edit.yaml" --out "$work/nozoom.mp4"

# The mean luma of a region of one frame: `luma <video> <seconds> <w:h:x:y>`.
luma() {
    ffmpeg -v error -ss "$2" -i "$1" -frames:v 1 \
        -vf "crop=$3,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-" -f null - |
        sed -n 's/.*YAVG=//p' | head -n 1
}
fail() { echo "::error::$1"; exit 1; }

# Nine seconds: 0–1 s nothing; the text announced 1–2.2 s, gliding to 2.7 s, top left until 4 s; ⇧⌘P
# 2–3.5 s; the zoom's badge top right 3–4 s; the speed-up 4–7 s, announced to 5.2 s and top right from 5.7 s;
# ⌘V 8 s on.
seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$work/t.mp4")
echo "duration: ${seconds}s"
awk -v s="$seconds" 'BEGIN { exit !(s > 8.8 && s < 9.2) }' || fail "the speed-up left ${seconds}s, expected 9s"

# Each check compares a region with the same region at 0.5 s, before anything shows.
check() {
    local name=$1 at=$2 region=$3 ratio=$4 now before
    now=$(luma "$work/t.mp4" "$at" "$region")
    before=$(luma "$work/t.mp4" 0.5 "$region")
    echo "$name: luma $now at ${at}s, $before before"
    awk -v a="$now" -v b="$before" -v r="$ratio" 'BEGIN { exit !(a < b * r) }' || fail "$name is missing"
}
check "the announced text in the centre" 1.5 300:10:810:504 0.6
check "the settled text top left" 3.5 120:8:40:46 0.6
check "the shortcut pill above the Dock" 2.5 120:4:900:915 0.6
check "the zoom badge top right, without an announcement" 3.2 120:8:1740:46 0.6
check "the speed-up badge top right" 6.5 120:8:1740:46 0.6

# Before its glide, nothing sits top left yet.
early=$(luma "$work/t.mp4" 2.0 120:8:40:46)
before=$(luma "$work/t.mp4" 0.5 120:8:40:46)
echo "top left at 2 s: $early, $before before"
awk -v a="$early" -v b="$before" 'BEGIN { exit !(a > b * 0.9) }' || fail "the text settled before its announcement ended"

# The zoom is fully eased in by 3.7 s, and both renders match outside it.
zoom_diff() {
    ffmpeg -v error -ss "$1" -i "$work/t.mp4" -ss "$1" -i "$work/nozoom.mp4" -frames:v 1 \
        -lavfi "[0:v][1:v]blend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-" \
        -f null - | sed -n 's/.*YAVG=//p' | head -n 1
}
zoomed=$(zoom_diff 3.7)
unzoomed=$(zoom_diff 2)
echo "zoom difference: $zoomed while zoomed, $unzoomed before"
awk -v z="$zoomed" -v u="$unzoomed" 'BEGIN { exit !(z > 10 && u < 2) }' || fail "the zoom is missing or spills over"
echo "render: all checks passed"
