#!/usr/bin/env bash
# Renders a synthetic capture with an edit file on any machine with ffmpeg: bar text, a speed-up with its
# caption and a zoom, each checked in the frames.
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
EOF
cat > "$work/t.edit.yaml" <<'EOF'
tracks:
  text:
    - { text: Rendered anywhere, from: shown, until: waiting }
  speed:
    - { label: Waiting, length: 1s, from: waiting, until: waited }
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
darker() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a < b * 0.6) }'; }

# Seven seconds: one before the text, three with it, four sped up into one, two after.
seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$work/t.mp4")
echo "duration: ${seconds}s"
awk -v s="$seconds" 'BEGIN { exit !(s > 6.8 && s < 7.2) }' || fail "the speed-up left ${seconds}s, expected 7s"

bar_shown=$(luma "$work/t.mp4" 2 1920:64:0:0)
bar_gone=$(luma "$work/t.mp4" 6 1920:64:0:0)
echo "bar strip luma: $bar_shown with the text, $bar_gone after"
darker "$bar_shown" "$bar_gone" || fail "the bar is missing or never goes"

# The caption box's top padding, above its text.
caption_shown=$(luma "$work/t.mp4" 4.5 200:12:860:846)
caption_gone=$(luma "$work/t.mp4" 6 200:12:860:846)
echo "caption luma: $caption_shown in the speed-up, $caption_gone after"
darker "$caption_shown" "$caption_gone" || fail "the speed-up's caption is missing"

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
