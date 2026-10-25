#!/usr/bin/env bash
# Records tapes on a macOS runner; inputs arrive as INPUT_* variables.
set -euo pipefail

readonly REPO=Hansehart/taperec

# Reports an error to the workflow and stops.
bail() {
    echo "::error::$*" >&2
    exit 1
}

# Accepts macOS, the platform taperec drives.
require_mac() {
    [ "${RUNNER_OS:-}" = macOS ] || bail "taperec records on macOS runners, not ${RUNNER_OS:-unknown}"
}

# Prints the Rust target triple for a runner architecture, e.g. ARM64.
target_triple() {
    case "${1:-}" in
        ARM64) echo aarch64-apple-darwin ;;
        X64) echo x86_64-apple-darwin ;;
        *) bail "taperec builds for ARM64 and X64 Macs, not ${1:-unknown}" ;;
    esac
}

# Runs a command, stopping it after a number of seconds.
limited() {
    perl -e 'alarm shift; exec @ARGV' "$@"
}

# Prints the files each newline-separated glob matches, in glob order.
tapes_from() {
    local pattern matches
    while IFS= read -r pattern; do
        [ -n "$pattern" ] || continue
        matches=$(compgen -G "$pattern" | sort) || bail "no tape matches $pattern"
        printf '%s\n' "$matches"
    done <<<"$1"
}

# Prints the package version from a Cargo manifest.
crate_version() {
    sed -n 's/^version = "\(.*\)"$/\1/p' "$1" | head -n 1
}

# Prints the release asset name for a version and target triple.
asset_name() {
    printf 'taperec-%s-%s.tar.gz\n' "$1" "$2"
}

# Prints a file's SHA-256 with whichever tool the system has.
sha256_of() {
    if command -v shasum >/dev/null; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        sha256sum "$1" | cut -d ' ' -f 1
    fi
}

# Checks a file against the hash in a `<hash>  <name>` checksum file.
verify_sha256() {
    local expected
    expected=$(cut -d ' ' -f 1 <"$2")
    [ "$(sha256_of "$1")" = "$expected" ] || bail "checksum mismatch for $(basename "$1")"
}

# Installs the tools taperec drives the desktop and encodes video with.
install_tools() {
    HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install ffmpeg displayplacer
    brew list --versions ffmpeg displayplacer
}

# Downloads and verifies the release binaries for a version and target into a directory.
fetch_binaries() {
    local version=$1 target=$2 bin=$3 asset url
    asset=$(asset_name "$version" "$target")
    url="https://github.com/$REPO/releases/download/v$version"
    for file in "$asset" "$asset.sha256"; do
        curl --proto '=https' --tlsv1.2 -fsSL --retry 5 -o "$RUNNER_TEMP/$file" "$url/$file"
    done
    verify_sha256 "$RUNNER_TEMP/$asset" "$RUNNER_TEMP/$asset.sha256"
    # Whatever the release holds, so an older release brings the tools its binary calls.
    tar -xzf "$RUNNER_TEMP/$asset" -C "$bin"
}

# Builds the binaries for a target from the action's own source into a directory.
build_from_source() {
    local source=$1 target=$2 bin=$3 arch
    (cd "$source" && rustup target add "$target")
    cargo build --release --locked --target "$target" --manifest-path "$source/Cargo.toml"
    cp "$source/target/$target/release/taperec" "$bin/"
    arch=${target%%-*}
    [ "$arch" != aarch64 ] || arch=arm64
    local desktop=(-c release --package-path "$source/tools/desktop" --arch "$arch")
    swift build "${desktop[@]}"
    cp "$(swift build "${desktop[@]}" --show-bin-path)/taperec-desktop" "$bin/"
}

# Prints the path of an installed app by name, e.g. Terminal.
app_path() {
    local dir
    for dir in /Applications /System/Applications /System/Applications/Utilities; do
        if [ -d "$dir/$1.app" ]; then
            printf '%s\n' "$dir/$1.app"
            return
        fi
    done
    bail "no app named $1"
}

# Tells whether a target's runners need the icon agent fixed.
crashing_icon_agent() {
    [ "$1" = x86_64-apple-darwin ]
}

# Restarts the per-user icon agent with Metal logging, rather than aborting on, the missing slice of its shader
# archive for the runner's paravirtualized GPU; RenderBox then compiles the shader and app icons render
# (actions/runner-images#14751).
icon_agent_fix() {
    local name=com.apple.iconservices.iconservicesagent plist label
    plist="${TMPDIR:-/tmp}/taperec-$name.plist"
    label="gui/$(id -u)/$name"
    plutil -convert xml1 -o "$plist" "/System/Library/LaunchAgents/$name.plist" ||
        bail "could not copy the icon agent's definition"
    plutil -remove EnvironmentVariables "$plist" 2> /dev/null || true
    plutil -insert EnvironmentVariables -json '{"MTL_ERROR_MODE":"nslog"}' "$plist" ||
        bail "could not set the icon agent's environment"
    limited 30 launchctl bootout "$label" || true
    limited 30 launchctl enable "$label" || bail "could not enable the icon agent"
    limited 30 launchctl bootstrap "gui/$(id -u)" "$plist" || bail "could not restart the icon agent"
}

# Waits up to a minute for a Dock other than the given pid to list its items, keeping a screenshot when none does.
wait_for_dock() {
    local out=$1 old=$2 _
    for _ in $(seq 60); do
        if [ "$(pgrep -x Dock)" != "$old" ] && limited 10 taperec-desktop dock >/dev/null 2>&1; then
            return
        fi
        sleep 1
    done
    mkdir -p "$out"
    screencapture -x "$out/dock-failshot.png" || true
    bail "the Dock did not come back after restarting"
}

# Sets a 1920x1080 screen, a gradient wallpaper and a Dock holding exactly the given apps.
# Prints, one per line, the ffmpeg input arguments for a wallpaper: the default gradient, a #rrggbb color or an image.
wallpaper_input() {
    local value=$1
    case $value in
        "")
            printf '%s\n' -f lavfi -i "gradients=s=1920x1080:c0=0x1e3c72:c1=0x7f53ac:x0=0:y0=0:x1=1920:y1=1080" ;;
        \#*)
            [[ $value =~ ^#[0-9a-fA-F]{6}$ ]] || bail "the wallpaper color $value is not #rrggbb"
            printf '%s\n' -f lavfi -i "color=c=0x${value#\#}:s=1920x1080" ;;
        *)
            [ -f "$value" ] || bail "no wallpaper image at $value"
            # Any size fills the screen, cropped around its centre.
            printf '%s\n' -i "$value" -vf "scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080" ;;
    esac
}

prepare_desktop() {
    local dock=$1 target=$2 out=$3 background=$4 id bounds wallpaper app old arg
    local -a input=()
    if crashing_icon_agent "$target"; then
        icon_agent_fix
    fi
    id=$(displayplacer list | awk '/^Persistent screen id:/ { print $4; exit }')
    displayplacer "id:$id res:1920x1080 scaling:off"
    bounds=$(osascript -e 'tell application "Finder" to get bounds of window of desktop')
    [ "$bounds" = "0, 0, 1920, 1080" ] || bail "the screen is $bounds, expected 0, 0, 1920, 1080"

    wallpaper="$RUNNER_TEMP/taperec-wallpaper.png"
    while IFS= read -r arg; do input+=("$arg"); done < <(wallpaper_input "$background")
    ffmpeg -loglevel error -y "${input[@]}" -frames:v 1 "$wallpaper"
    osascript -e 'on run argv' \
        -e 'tell application "System Events" to tell every desktop to set picture to POSIX file (item 1 of argv)' \
        -e 'end run' "$wallpaper"

    # A click on the wallpaper selects it instead of sweeping every window aside.
    defaults write com.apple.WindowManager EnableStandardClickToShowDesktop -bool false
    killall WindowManager || true

    defaults write com.apple.dock persistent-apps -array
    defaults write com.apple.dock show-recents -bool false
    while IFS= read -r app; do
        [ -n "$app" ] || continue
        defaults write com.apple.dock persistent-apps -array-add \
            "<dict><key>tile-data</key><dict><key>file-data</key><dict><key>_CFURLString</key><string>$(app_path "$app")</string><key>_CFURLStringType</key><integer>0</integer></dict></dict></dict>"
    done <<<"$dock"
    old=$(pgrep -x Dock)
    killall Dock
    wait_for_dock "$out" "$old"
}

# Records each tape into a directory and lists the videos as the action's output.
# Prints where a tape's failure screenshot goes: its file name without the extension.
failshot_path() {
    local name
    name=$(basename "$2")
    printf '%s/%s-failshot.png\n' "$1" "${name%.*}"
}

render() {
    local out=$1 tape
    mkdir -p "$out"
    while IFS= read -r tape; do
        taperec "$tape" --out-dir "$out" --failshot "$(failshot_path "$out" "$tape")" \
            --recorder "${INPUT_RECORDER:-auto}"
    done <<<"$2"
    {
        echo "videos<<TAPEREC_VIDEOS"
        find "$out" -maxdepth 1 -name '*.mp4' | sort
        echo "TAPEREC_VIDEOS"
    } >>"$GITHUB_OUTPUT"
}

main() {
    require_mac
    [ -n "${INPUT_TAPES:-}" ] || bail "the tapes input is required"
    local tapes target bin tape
    tapes=$(tapes_from "$INPUT_TAPES")
    target=$(target_triple "${RUNNER_ARCH:-}")
    install_tools
    bin="$RUNNER_TEMP/taperec-bin"
    mkdir -p "$bin"
    if [ "${INPUT_FROM_SOURCE:-false}" = true ]; then
        build_from_source "$GITHUB_ACTION_PATH" "$target" "$bin"
    else
        fetch_binaries "$(crate_version "$GITHUB_ACTION_PATH/Cargo.toml")" "$target" "$bin"
    fi
    export PATH="$bin:$PATH"
    taperec --version
    # Every tape is valid before the desktop is touched.
    local -a list=()
    while IFS= read -r tape; do list+=("$tape"); done <<<"$tapes"
    taperec check "${list[@]}" || bail "a tape is invalid"
    wallpaper_input "${INPUT_WALLPAPER:-}" > /dev/null || exit 1
    prepare_desktop "${INPUT_DOCK:-}" "$target" "${INPUT_OUT_DIR:-taperec}" "${INPUT_WALLPAPER:-}"
    render "${INPUT_OUT_DIR:-taperec}" "$tapes"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
