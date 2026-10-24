#!/usr/bin/env bash
# Linux desktop probe: probe/linux.sh desktop|keys|controls|window-at|capture|docker OUT
set -uo pipefail
mode=$1 out=$2
mkdir -p "$out"
now() { date +%s.%N; }
since() { awk -v a="$1" -v b="$(now)" 'BEGIN { printf "%.1f s", b - a }'; }
shot() { ffmpeg -v error -y -f x11grab -video_size 1920x1080 -i "$DISPLAY" -frames:v 1 "$out/$1.png"; }

# Installs and starts Xvfb, a D-Bus session with the accessibility bus, and Xfce.
desktop() {
    local t
    t=$(now)
    sudo apt-get update -qq
    sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends \
        xvfb xfce4 xfce4-terminal xdotool xclip wmctrl at-spi2-core ffmpeg zenity python3-pyatspi \
        dbus-x11 x11-utils libatk-adaptor python3-gi gir1.2-gtk-3.0 > "$out/apt.log" 2>&1
    echo "SUMMARY apt install: $(since "$t")"
    t=$(now)
    export DISPLAY=:99
    Xvfb :99 -screen 0 1920x1080x24 > "$out/xvfb.log" 2>&1 &
    for _ in $(seq 100); do xdpyinfo > /dev/null 2>&1 && break; sleep 0.1; done
    eval "$(dbus-launch --sh-syntax)"
    export NO_AT_BRIDGE=0 GTK_MODULES=gail:atk-bridge
    gsettings set org.gnome.desktop.interface toolkit-accessibility true 2> /dev/null
    startxfce4 > "$out/xfce.log" 2>&1 &
    for _ in $(seq 300); do
        wmctrl -m > /dev/null 2>&1 && xdotool search --class xfce4-panel > /dev/null 2>&1 && break
        sleep 0.1
    done
    echo "SUMMARY desktop ready: $(since "$t") ($(wmctrl -m 2>&1 | head -1))"
    sleep 2
    shot desktop
    {
        echo "DISPLAY=$DISPLAY"
        echo "DBUS_SESSION_BUS_ADDRESS=$DBUS_SESSION_BUS_ADDRESS"
        echo "NO_AT_BRIDGE=0"
        echo "GTK_MODULES=gail:atk-bridge"
    } >> "${GITHUB_ENV:-/dev/null}"
}

# A terminal whose input lands in $1, focused.
terminal() {
    xfce4-terminal --disable-server -T typed -e "sh -c 'cat > $1'" &
    xdotool search --sync --name '^typed$' > /dev/null
    xdotool windowactivate --sync "$(xdotool search --name '^typed$' | head -1)"
    sleep 1
}

# Characters missing from $2 compared with $1, by longest matching blocks.
missing() {
    python3 - "$1" "$2" <<'PY'
import difflib, sys
want, got = (open(p, encoding="utf-8", errors="replace").read() for p in sys.argv[1:])
same = sum(b.size for b in difflib.SequenceMatcher(None, want, got, autojunk=False).get_matching_blocks())
print(f"{len(want) - same} of {len(want)} lost, {len(got) - same} extra")
PY
}

keys() {
    local line i c
    line=$(python3 -c 'print("".join(chr(c) for c in range(33, 127)) + "éüß€")')
    # One xdotool call per line, as a fast typist would.
    terminal /tmp/typed-calls
    for i in $(seq 100); do xdotool type --delay 12 -- "$line"; xdotool key Return; done
    xdotool key ctrl+d; sleep 1
    for i in $(seq 100); do echo "$line"; done > "$out/want-calls"
    echo "SUMMARY type, one call per line: $(missing "$out/want-calls" /tmp/typed-calls)"
    # One process per character, as taperec's helper types.
    terminal /tmp/typed-chars
    for i in $(seq 20); do
        for ((c = 0; c < ${#line}; c++)); do xdotool type -- "${line:c:1}"; done
        xdotool key Return
    done
    xdotool key ctrl+d; sleep 1
    for i in $(seq 20); do echo "$line"; done > "$out/want-chars"
    echo "SUMMARY type, one process per character: $(missing "$out/want-chars" /tmp/typed-chars)"
    # Lone Returns, each ending a numbered line.
    terminal /tmp/typed-returns
    for i in $(seq 500); do xdotool type -- "r$i"; xdotool key Return; done
    xdotool key ctrl+d; sleep 1
    seq 500 | sed 's/^/r/' > "$out/want-returns"
    echo "SUMMARY lone returns: $(grep -c . /tmp/typed-returns) of 500 lines, $(missing "$out/want-returns" /tmp/typed-returns)"
    # Paste through the clipboard.
    terminal /tmp/typed-paste
    for i in $(seq 20); do printf '%s' "$line" | xclip -selection clipboard; xdotool key ctrl+shift+v; xdotool key Return; done
    xdotool key ctrl+d; sleep 1
    for i in $(seq 20); do echo "$line"; done > "$out/want-paste"
    echo "SUMMARY paste: $(missing "$out/want-paste" /tmp/typed-paste)"
}

# A GTK 3 window titled $1 at (300, 200), 400x200, whose Yes button prints "yes" and quits.
gtk3_window() {
    python3 - "$1" <<'PY' &
import sys, gi
gi.require_version("Gtk", "3.0")
from gi.repository import Gtk
window = Gtk.Window(title=sys.argv[1])
window.set_default_size(400, 200)
window.move(300, 200)
button = Gtk.Button(label="Yes")
def clicked(_):
    print("yes", flush=True)
    Gtk.main_quit()
button.connect("clicked", clicked)
window.add(button)
window.show_all()
Gtk.main()
PY
}

# Waits until a window titled $1 is listed, printing its X11 id.
window_id() {
    for _ in $(seq 100); do
        wmctrl -l | awk -v t="$1" '{ id = $1; $1 = $2 = $3 = ""; sub(/^ +/, ""); if ($0 == t) print id }' | grep . && return
        sleep 0.1
    done
    return 1
}

# The absolute upper-left corner and size of X11 window $1: "x y w h".
x11_frame() {
    xwininfo -id "$1" | awk '
        /Absolute upper-left X/ { x = $NF } /Absolute upper-left Y/ { y = $NF }
        /^ *Width/ { w = $NF } /^ *Height/ { h = $NF } END { print x, y, w, h }'
}

# The AT-SPI tree of the app showing a window titled $1, into $2; then the Yes button as
# "dx dy dw dh wx wy": its extents in desktop and in window coordinates.
atspi_yes() {
    python3 - "$1" "$2" <<'PY'
import sys, pyatspi
title, dump = sys.argv[1], open(sys.argv[2], "w")
found = None
def extents(node, kind):
    e = node.queryComponent().getExtents(kind)
    return e.x, e.y, e.width, e.height
def walk(node, depth):
    global found
    if node is None or depth > 12:
        return
    try:
        role, name = node.getRoleName(), node.name
        try:
            desk, win = extents(node, pyatspi.DESKTOP_COORDS), extents(node, pyatspi.WINDOW_COORDS)
        except Exception:
            desk = win = None
        print(f"{'  ' * depth}{role} {name!r} desktop={desk} window={win}", file=dump)
        if found is None and node.getRole() == pyatspi.ROLE_PUSH_BUTTON and name in ("Yes", "_Yes") and desk:
            found = desk + win[:2]
        for child in node:
            walk(child, depth + 1)
    except Exception as error:
        print(f"{'  ' * depth}! {error}", file=dump)
for app in pyatspi.Registry.getDesktop(0):
    if any(getattr(w, "name", "") == title for w in app):
        walk(app, 0)
if found:
    print(*found)
PY
}

# Finds the Yes button of a window titled $1 ($2 names its toolkit) and clicks it by window
# coordinates plus the X11 window's position; prints what each coordinate route gives.
click_yes() {
    local title=$1 kit=$2 id fx fy fw fh dx dy dw dh wx wy t
    id=$(window_id "$title") || { echo "SUMMARY $kit: no window titled $title"; return; }
    read -r fx fy fw fh < <(x11_frame "$id")
    echo "SUMMARY $kit window: id $id at $fx,$fy ${fw}x$fh, frame extents $(xprop -id "$id" _GTK_FRAME_EXTENTS 2>/dev/null | sed 's/.*= //')"
    t=$(now)
    read -r dx dy dw dh wx wy < <(atspi_yes "$title" "$out/atspi-$kit.txt")
    echo "SUMMARY $kit Yes: desktop ${dx:-?},${dy:-?} ${dw:-?}x${dh:-?}; window ${wx:-?},${wy:-?}; after $(since "$t")"
    [ -n "${wx:-}" ] || return
    echo "SUMMARY $kit routes: desktop centre $((dx + dw / 2)),$((dy + dh / 2)); window + X11 $((fx + wx + dw / 2)),$((fy + wy + dh / 2))"
    shot "controls-$kit"
    xdotool mousemove $((fx + wx + dw / 2)) $((fy + wy + dh / 2)) click 1
}

controls() {
    local zenity code
    zenity --question --title "taperec probe" --text "Click Yes" --timeout 30 &
    zenity=$!
    gtk3_window "taperec gtk3" > "$out/gtk3.txt"
    sleep 2
    click_yes "taperec probe" gtk4
    for _ in $(seq 50); do kill -0 "$zenity" 2> /dev/null || break; sleep 0.1; done
    wait "$zenity"; code=$?
    echo "SUMMARY gtk4 click: zenity exited $code (0 = Yes clicked, 5 = timed out)"
    click_yes "taperec gtk3" gtk3
    sleep 1
    echo "SUMMARY gtk3 click: the window printed '$(cat "$out/gtk3.txt")' (yes = clicked)"
}

window_at() {
    local id fx fy fw fh WINDOW X Y SCREEN
    gtk3_window "taperec window" > /dev/null
    id=$(window_id "taperec window") || { echo "SUMMARY window-at: no window"; return; }
    sleep 1
    read -r fx fy fw fh < <(x11_frame "$id")
    eval "$(xdotool mousemove $((fx + fw / 2)) $((fy + fh / 2)) getmouselocation --shell)"
    echo "SUMMARY window-at: under $X,$Y is $WINDOW '$(xdotool getwindowname "$WINDOW" 2>&1)' at $(x11_frame "$WINDOW"); the window is $id at $fx $fy $fw $fh"
    # Over the bare desktop, the window under the pointer is the desktop's.
    eval "$(xdotool mousemove 1800 900 getmouselocation --shell)"
    echo "SUMMARY window-at desktop: under $X,$Y is $WINDOW '$(xdotool getwindowname "$WINDOW" 2>&1)' at $(x11_frame "$WINDOW")"
}

capture() {
    local t start pid code
    xfce4-terminal --disable-server -e "sh -c 'while :; do date; ls -la /usr/bin | head -40; done'" &
    sleep 3
    ( while :; do ps -C ffmpeg -o %cpu= | awk '{ s += $1 } END { print s + 0 }'; sleep 2; done ) > "$out/cpu.txt" &
    local sampler=$!
    # Stopped with `q` on stdin, as taperec stops its captures.
    mkfifo "$out/control"
    t=$(now)
    ffmpeg -v error -y -use_wallclock_as_timestamps 1 -f x11grab -framerate 30 -video_size 1920x1080 -i "$DISPLAY" \
        -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency \
        -copyts -flush_packets 1 -f matroska -live 1 -cluster_time_limit 1000 "$out/capture.mkv" < "$out/control" &
    pid=$!
    exec 3> "$out/control"
    sleep 60
    echo q >&3
    exec 3>&-
    wait "$pid"; code=$?
    kill "$sampler"
    start=$(ffprobe -v error -show_entries format=start_time -of csv=p=0 "$out/capture.mkv")
    local frames seconds
    frames=$(ffprobe -v error -select_streams v:0 -count_packets -show_entries stream=nb_read_packets -of csv=p=0 "$out/capture.mkv")
    seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out/capture.mkv")
    ffmpeg -v error -i "$out/capture.mkv" -f null - 2> "$out/capture-decode.txt"
    echo "SUMMARY capture stopped with q: exit $code, $frames frames in ${seconds}s ($(awk -v f="$frames" -v s="$seconds" 'BEGIN { printf "%.1f", f / s }') fps), first frame $(awk -v a="$start" -v b="$t" 'BEGIN { printf "%+.3f", a - b }') s after launch, decode errors $(grep -c . "$out/capture-decode.txt"), ffmpeg CPU $(awk '{ s += $1; n++ } END { printf "%.0f%%", s / n }' "$out/cpu.txt") on $(nproc) cores"
}

docker_cage() {
    local t
    t=$(now)
    docker info > /dev/null && echo "SUMMARY docker info: $(since "$t")"
    # The tutorial's cage: o3s without its optional features.
    python3 examples/o3s/no-features.py o3s/.devcontainer/devcontainer.json
    t=$(now)
    npx -y @devcontainers/cli@latest up --workspace-folder o3s > "$out/cage.log" 2>&1
    echo "SUMMARY cage up: exit $? after $(since "$t")"
    tail -5 "$out/cage.log"
}

case $mode in
    desktop) desktop ;;
    keys) keys ;;
    controls) controls ;;
    window-at) window_at ;;
    capture) capture ;;
    docker) docker_cage ;;
esac
