#!/usr/bin/env bash
# Records with taperec-desktop and checks the movie: record-contract.sh HELPER DIR
set -euo pipefail

helper=$1
dir=$2
movie=$dir/record.mov
mkdir -p "$dir"

fail() {
    echo "::error::$*" >&2
    cat "$dir/recorder.err" >&2 || true
    exit 1
}

now() { perl -MTime::HiRes=time -e 'printf "%.3f", time'; }

# The fifo stays open for writing, so the recorder reads `q` only when sent.
mkfifo "$dir/control"
exec 3<>"$dir/control"
before=$(now)
"$helper" record "$movie" <&3 >"$dir/recorder.out" 2>"$dir/recorder.err" &
recorder=$!
for _ in $(seq 120); do
    grep -qx recording "$dir/recorder.out" && break
    sleep 0.25
done
grep -qx recording "$dir/recorder.out" || fail "the recorder never reported recording"
# Changes after still seconds follow the frames that re-show a still screen.
for x in 300 500 700 900 1100; do
    sleep 1.3
    "$helper" move "$x" 400
done
# A still screen lasts until the stop, not just to its last change.
sleep 2
echo q >&3
status=0
wait "$recorder" || status=$?
stopped=$(now)
exec 3>&-
[ "$status" -eq 0 ] || fail "the recorder exited with $status"

start=$(ffprobe -v error -show_entries format_tags=com.taperec.start -of csv=p=0 "$movie")
duration=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$movie")
echo "recorded from $start for $duration s, between $before and $stopped"
awk -v s="$start" -v b="$before" -v e="$stopped" 'BEGIN { exit !(s >= b && s <= e) }' ||
    fail "the start tag $start lies outside the recording"
awk -v d="$duration" 'BEGIN { exit !(d >= 8.4) }' || fail "the movie lasts $duration s"
ffmpeg -v error -i "$movie" -f null - || fail "the movie does not decode"
echo "the recording contract holds"
