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
    HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_NO_INSTALL_CLEANUP=1 brew install ffmpeg cliclick displayplacer
    brew list --versions ffmpeg cliclick displayplacer
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
    tar -xzf "$RUNNER_TEMP/$asset" -C "$bin" taperec taperec-caption
}

# Builds the binaries for a target from the action's own source into a directory.
build_from_source() {
    local source=$1 target=$2 bin=$3 arch
    (cd "$source" && rustup target add "$target")
    cargo build --release --locked --target "$target" --manifest-path "$source/Cargo.toml"
    cp "$source/target/$target/release/taperec" "$bin/"
    arch=${target%%-*}
    [ "$arch" != aarch64 ] || arch=arm64
    swiftc -O -target "$arch-apple-macos$(sw_vers -productVersion | cut -d . -f 1)" \
        "$source/tools/caption.swift" -o "$bin/taperec-caption"
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

# Sets a 1920x1080 screen, a gradient wallpaper and a Dock holding exactly the given apps.
prepare_desktop() {
    local id bounds wallpaper app
    id=$(displayplacer list | awk '/^Persistent screen id:/ { print $4; exit }')
    displayplacer "id:$id res:1920x1080 scaling:off"
    bounds=$(osascript -e 'tell application "Finder" to get bounds of window of desktop')
    [ "$bounds" = "0, 0, 1920, 1080" ] || bail "the screen is $bounds, expected 0, 0, 1920, 1080"

    wallpaper="$RUNNER_TEMP/taperec-wallpaper.png"
    ffmpeg -loglevel error -y -f lavfi \
        -i "gradients=s=1920x1080:c0=0x1e3c72:c1=0x7f53ac:x0=0:y0=0:x1=1920:y1=1080" \
        -frames:v 1 "$wallpaper"
    osascript -e 'on run argv' \
        -e 'tell application "System Events" to tell every desktop to set picture to POSIX file (item 1 of argv)' \
        -e 'end run' "$wallpaper"

    defaults write com.apple.dock persistent-apps -array
    defaults write com.apple.dock show-recents -bool false
    while IFS= read -r app; do
        [ -n "$app" ] || continue
        defaults write com.apple.dock persistent-apps -array-add \
            "<dict><key>tile-data</key><dict><key>file-data</key><dict><key>_CFURLString</key><string>$(app_path "$app")</string><key>_CFURLStringType</key><integer>0</integer></dict></dict></dict>"
    done <<<"$1"
    killall Dock
    # The Dock redraws its items within a few seconds of restarting.
    sleep 3
}

# Records each tape into a directory and lists the videos as the action's output.
render() {
    local out=$1 tape
    mkdir -p "$out"
    while IFS= read -r tape; do
        taperec "$tape" --out-dir "$out" --failshot "$out/$(basename "$tape" .tape)-failshot.png"
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
    local tapes target bin
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
    prepare_desktop "${INPUT_DOCK:-}"
    render "${INPUT_OUT_DIR:-taperec}" "$tapes"
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
    main "$@"
fi
