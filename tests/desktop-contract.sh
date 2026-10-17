#!/usr/bin/env bash
# Drives the fixture window with taperec-desktop and checks what reached it: desktop-contract.sh HELPER FIXTURE
set -euo pipefail
export LC_ALL=en_US.UTF-8

helper=$1
fixture=$2
log=/tmp/taperec-fixture.log

fail() {
    echo "::error::$*" >&2
    echo "--- fixture log" >&2
    cat "$log" >&2 || true
    exit 1
}

# Retries a command for up to $1 seconds.
eventually() {
    local seconds=$1
    shift
    for _ in $(seq "$((seconds * 4))"); do
        "$@" && return
        sleep 0.25
    done
    return 1
}

logged() { grep -qxF -- "$1" "$log"; }
# The last line the fixture logged.
last_logged() { tail -n 1 "$log"; }
# Whether the fixture logged at least $1 entered lines.
entered() { [ "$(grep -c '^entered ' "$log")" -ge "$1" ]; }
in_front() { [ "$("$helper" focus "$1")" = true ]; }
lists_window() { "$helper" windows | grep -qxF "$1"; }

rm -f "$log"
"$fixture" &
fixture_pid=$!
trap 'kill "$fixture_pid" 2>/dev/null || true' EXIT
eventually 20 logged ready || fail "the fixture never started"

echo "windows lists the fixture"
eventually 10 lists_window "taperec fixture" || fail "no fixture window in: $("$helper" windows)"

echo "buttons reports Open inside the window"
open_line=$("$helper" buttons | awk -F '\t' '$3 == "Open"')
[ "$(printf '%s\n' "$open_line" | wc -l)" -eq 1 ] || fail "Open buttons: $open_line"
IFS=$'\t' read -r app frontmost _ x y w h <<<"$open_line"
[ "$app" = taperec-fixture ] || fail "Open belongs to $app"
[ "$frontmost" = true ] || fail "the fixture is not frontmost"
[ "$w" -gt 0 ] && [ "$h" -gt 0 ] || fail "Open has no size: $open_line"

echo "move and click press Open"
"$helper" move "$((x + w / 2))" "$((y + h / 2))"
pointer=$("$helper" pointer)
[ "$pointer" = "$((x + w / 2))"$'\t'"$((y + h / 2))" ] || fail "pointer at $pointer"
"$helper" click
eventually 5 logged "pressed Open" || fail "the click missed Open"

echo "focus brings apps to the front"
open -a Terminal
eventually 20 in_front Terminal || fail "Terminal stayed behind"
eventually 20 in_front taperec-fixture || fail "the fixture stayed behind"

# Every printable ASCII character plus some beyond the keyboard layout, 100 in all.
line=$(printf '%b' "$(printf '\\%03o' $(seq 32 126))")"éüß€x"
[ "${#line}" -eq 100 ] || fail "the test line has ${#line} characters"

echo "type and key return deliver every character"
for round in 1 2 3; do
    for ((i = 0; i < ${#line}; i++)); do
        "$helper" type "${line:i:1}"
    done
    "$helper" key return
    eventually 5 entered "$round" || fail "round $round: return never arrived"
    [ "$(last_logged)" = "entered $line" ] || fail "round $round typed: $(last_logged)"
done

echo "paste delivers the whole line"
for round in 1 2 3 4 5; do
    "$helper" paste "$line"
    "$helper" key return
    eventually 5 entered "$((3 + round))" || fail "round $round: return never arrived"
    [ "$(last_logged)" = "entered $line" ] || fail "round $round pasted: $(last_logged)"
done

echo "dock lists Finder"
"$helper" dock | cut -f 1 | grep -qxF Finder || fail "no Finder in: $("$helper" dock)"

echo "an untypable character exits 3"
status=0
"$helper" type $'\a' 2>/dev/null || status=$?
[ "$status" -eq 3 ] || fail "untypable exited $status"

echo "the desktop contract holds"
