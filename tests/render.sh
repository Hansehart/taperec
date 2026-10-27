#!/usr/bin/env bash
# Renders a synthetic capture with an edit file on any machine with ffmpeg: a chapter card, the frosted bar,
# a hold and the zoom, each checked in the frames.
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
# The edit, with and without its zoom.
edit() {
    cat <<'EOF'
tracks:
  chapter:
    - { text: Rendered anywhere, from: start, until: end }
  comment:
    - { text: Typing a command, from: shown, until: waiting }
  speed:
    - { label: Waiting, length: 3s, from: waiting, until: waited }
  hold:
    - { at: results + 500ms, duration: 2s }
EOF
    [ "$1" = zoom ] && printf '  zoom:\n    - { at: results, scale: 2, from: results, until: waiting }\n'
    true
}
edit zoom > "$work/t.edit.yaml"
edit none > "$work/nozoom.edit.yaml"

"$taperec" render "$work/t.timeline.yaml" --edit "$work/t.edit.yaml" --out "$work/t.mp4"
"$taperec" render "$work/t.timeline.yaml" --edit "$work/nozoom.edit.yaml" --out "$work/nozoom.mp4"

# The mean luma of a region of one frame: `luma <video> <seconds> <w:h:x:y>`.
luma() {
    ffmpeg -v error -ss "$2" -i "$1" -frames:v 1 \
        -vf "crop=$3,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-" -f null - |
        sed -n 's/.*YAVG=//p' | head -n 1
}
# The mean luma of the difference of two frames: `difference <video> <seconds> <video> <seconds>`.
difference() {
    ffmpeg -v error -ss "$2" -i "$1" -ss "$4" -i "$3" -frames:v 1 \
        -lavfi "[0:v][1:v]blend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-" \
        -f null - | sed -n 's/.*YAVG=//p' | head -n 1
}
fail() { echo "::error::$1"; exit 1; }
below() { awk -v a="$1" -v b="$2" 'BEGIN { exit !(a < b) }'; }

# 0–2 s the chapter's card; then the bar. The comment 3–8 s, ⇧⌘P 4 s, the zoom 5–8 s with its frame held
# 5.5–7.5 s, the speed-up 8–11 s, ⌘V 12 s; 13 s in all.
seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$work/t.mp4")
echo "duration: ${seconds}s"
awk -v s="$seconds" 'BEGIN { exit !(s > 12.8 && s < 13.2) }' || fail "the video lasts ${seconds}s, expected 13s"

dimmed=$(luma "$work/t.mp4" 1 300:60:100:900)
lit=$(luma "$work/t.mp4" 12.5 300:60:100:900)
echo "card: luma $dimmed under the card, $lit after"
below "$dimmed" "$(awk -v l="$lit" 'BEGIN { print l * 0.6 }')" || fail "the chapter card does not dim the screen"

band=$(luma "$work/t.mp4" 4.5 1920:64:0:0)
under=$(luma "$work/t.mp4" 4.5 1920:64:0:64)
echo "bar: luma $band in the band, $under just below"
below "$band" "$(awk -v l="$under" 'BEGIN { print l * 0.8 }')" || fail "the bar is missing"

still=$(difference "$work/t.mp4" 6 "$work/t.mp4" 7)
moving=$(difference "$work/t.mp4" 9 "$work/t.mp4" 10)
echo "hold: frames 1 s apart differ by $still in the hold, $moving in the speed-up"
below "$still" 1 || fail "the held frame moves"
below 1 "$moving" || fail "the video stays still after the hold"

zoomed=$(difference "$work/t.mp4" 5.3 "$work/nozoom.mp4" 5.3)
unzoomed=$(difference "$work/t.mp4" 3.5 "$work/nozoom.mp4" 3.5)
echo "zoom difference: $zoomed while zoomed, $unzoomed before"
awk -v z="$zoomed" -v u="$unzoomed" 'BEGIN { exit !(z > 10 && u < 2) }' || fail "the zoom is missing or spills over"
echo "render: all checks passed"
