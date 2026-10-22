#!/usr/bin/env bash
# Linux desktop probe: probe/linux.sh desktop|keys|controls|capture|docker OUT
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
        dbus-x11 x11-utils libatk-adaptor > "$out/apt.log" 2>&1
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

controls() {
    local t pid x y w h code
    zenity --question --title "taperec probe" --text "Click Yes" &
    pid=$!
    t=$(now)
    for _ in $(seq 100); do wmctrl -l | grep -q 'taperec probe' && break; sleep 0.1; done
    echo "SUMMARY window listed: $(wmctrl -l | grep -c 'taperec probe') after $(since "$t")"
    wmctrl -a 'taperec probe'; sleep 0.5
    echo "SUMMARY focus: active window is '$(xdotool getactivewindow getwindowname)'"
    t=$(now)
    read -r x y w h < <(python3 - <<'PY'
import pyatspi
def walk(node, depth=0):
    if depth > 12 or node is None:
        return
    try:
        if node.getRole() == pyatspi.ROLE_PUSH_BUTTON and node.name in ("Yes", "_Yes"):
            e = node.queryComponent().getExtents(pyatspi.DESKTOP_COORDS)
            print(e.x, e.y, e.width, e.height)
            raise SystemExit
        for child in node:
            walk(child, depth + 1)
    except SystemExit:
        raise
    except Exception:
        pass
for app in pyatspi.Registry.getDesktop(0):
    walk(app)
PY
)
    echo "SUMMARY AT-SPI Yes button: ${x:-?} ${y:-?} ${w:-?} ${h:-?} after $(since "$t")"
    shot controls
    if [ -n "${x:-}" ]; then
        xdotool mousemove $((x + w / 2)) $((y + h / 2)) click 1
    fi
    for _ in $(seq 50); do kill -0 "$pid" 2> /dev/null || break; sleep 0.1; done
    wait "$pid"; code=$?
    echo "SUMMARY click: zenity exited $code (0 = Yes clicked)"
}

capture() {
    local t start
    xfce4-terminal --disable-server -e "sh -c 'while :; do date; ls -la /usr/bin | head -40; done'" &
    sleep 3
    ( while :; do ps -C ffmpeg -o %cpu= | awk '{ s += $1 } END { print s + 0 }'; sleep 2; done ) > "$out/cpu.txt" &
    local sampler=$!
    t=$(now)
    ffmpeg -v error -y -use_wallclock_as_timestamps 1 -f x11grab -framerate 30 -video_size 1920x1080 -i "$DISPLAY" \
        -t 60 -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency \
        -copyts -flush_packets 1 -f matroska -live 1 -cluster_time_limit 1000 "$out/capture.mkv" < /dev/null
    kill "$sampler"
    start=$(ffprobe -v error -show_entries format=start_time -of csv=p=0 "$out/capture.mkv")
    echo "SUMMARY capture: $(ffprobe -v error -select_streams v:0 -count_packets -show_entries stream=nb_read_packets -of csv=p=0 "$out/capture.mkv") frames in 60 s, start_time $start vs launch $t, ffmpeg CPU $(awk '{ s += $1; n++ } END { printf "%.0f%%", s / n }' "$out/cpu.txt") on $(nproc) cores"
    ffmpeg -v error -y -use_wallclock_as_timestamps 1 -f x11grab -framerate 30 -video_size 1920x1080 -i "$DISPLAY" \
        -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -tune zerolatency \
        -copyts -flush_packets 1 -f matroska -live 1 -cluster_time_limit 1000 "$out/killed.mkv" < /dev/null &
    local pid=$!
    sleep 10; kill -9 "$pid"; wait "$pid" 2> /dev/null
    ffmpeg -v error -i "$out/killed.mkv" -f null - 2> "$out/killed-decode.txt"
    echo "SUMMARY killed capture: $(ffprobe -v error -select_streams v:0 -count_packets -show_entries stream=nb_read_packets -of csv=p=0 "$out/killed.mkv") frames readable after 10 s, decode errors: $(grep -c . "$out/killed-decode.txt")"
}

docker_cage() {
    local t
    t=$(now)
    docker info > /dev/null && echo "SUMMARY docker info: $(since "$t")"
    t=$(now)
    npx -y @devcontainers/cli@latest up --workspace-folder o3s > "$out/cage.log" 2>&1
    echo "SUMMARY cage up: exit $? after $(since "$t")"
    tail -5 "$out/cage.log"
}

case $mode in
    desktop) desktop ;;
    keys) keys ;;
    controls) controls ;;
    capture) capture ;;
    docker) docker_cage ;;
esac
