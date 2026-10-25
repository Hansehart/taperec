#!/usr/bin/env bash
# Tests the action script's functions on any machine.
set -euo pipefail

here=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=action/main.sh
source "$here/action/main.sh"

failures=0
check() {
    local name=$1 expected=$2 actual=$3
    if [ "$expected" = "$actual" ]; then
        echo "ok   $name"
    else
        echo "FAIL $name"
        printf '  expected: %q\n  actual:   %q\n' "$expected" "$actual"
        failures=$((failures + 1))
    fi
}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$work/tutorials"
touch "$work/tutorials/b.yaml" "$work/tutorials/a.yaml" "$work/intro.yaml"

check "globs expand in order" \
    "$work/intro.yaml
$work/tutorials/a.yaml
$work/tutorials/b.yaml" \
    "$(tapes_from "$work/intro.yaml
$work/tutorials/*.yaml")"
check "a glob without matches fails" \
    "::error::no tape matches $work/missing/*.yaml" \
    "$( (tapes_from "$work/missing/*.yaml") 2>&1 || true)"
check "blank lines are skipped" "$work/intro.yaml" "$(tapes_from "
$work/intro.yaml
")"

printf '[package]\nname = "demo"\nversion = "1.2.3"\nedition = "2021"\n\n[dependencies]\nother = { version = "9.9.9" }\n' > "$work/Cargo.toml"
check "version comes from the package section" "1.2.3" "$(crate_version "$work/Cargo.toml")"
check "asset name" "taperec-0.1.0-x86_64-apple-darwin.tar.gz" "$(asset_name 0.1.0 x86_64-apple-darwin)"

printf 'video' > "$work/asset"
(cd "$work" && sha256sum asset > asset.sha256)
check "a matching checksum passes" "verified" "$(verify_sha256 "$work/asset" "$work/asset.sha256" && echo verified)"
printf 'tampered' > "$work/asset"
check "a tampered file fails" "::error::checksum mismatch for asset" \
    "$( (verify_sha256 "$work/asset" "$work/asset.sha256") 2>&1 || true)"

check "Linux is refused" "::error::taperec records on macOS runners, not Linux" \
    "$( (RUNNER_OS=Linux require_mac) 2>&1 || true)"
check "macOS passes" "ok" "$(RUNNER_OS=macOS require_mac && echo ok)"
check "Apple silicon target" "aarch64-apple-darwin" "$(target_triple ARM64)"
check "Intel target" "x86_64-apple-darwin" "$(target_triple X64)"
check "other architectures are refused" "::error::taperec builds for ARM64 and X64 Macs, not ARM" \
    "$( (target_triple ARM) 2>&1 || true)"

check "Intel runners fix the icon agent" "yes" "$(crashing_icon_agent x86_64-apple-darwin && echo yes)"
check "Apple silicon keeps the icon agent as it is" "no" "$(crashing_icon_agent aarch64-apple-darwin || echo no)"
mkdir -p "$work/bin"
for tool in launchctl plutil; do
    printf '#!/bin/sh\necho "%s $*" >> "%s/calls"\n' "$tool" "$work" > "$work/bin/$tool"
    chmod +x "$work/bin/$tool"
done
(PATH="$work/bin:$PATH" TMPDIR="$work" icon_agent_fix)
agent=com.apple.iconservices.iconservicesagent
check "the icon agent restarts from a copy with Metal logging its archive error" \
    "plutil -convert xml1 -o $work/taperec-$agent.plist /System/Library/LaunchAgents/$agent.plist
plutil -remove EnvironmentVariables $work/taperec-$agent.plist
plutil -insert EnvironmentVariables -json {\"MTL_ERROR_MODE\":\"nslog\"} $work/taperec-$agent.plist
launchctl bootout gui/$(id -u)/$agent
launchctl enable gui/$(id -u)/$agent
launchctl bootstrap gui/$(id -u) $work/taperec-$agent.plist" \
    "$(cat "$work/calls")"

check "failshots drop the tape extension" "/out/intro-failshot.png" "$(failshot_path /out tutorials/intro.yaml)"
check "failshots keep dots inside the name" "/out/v1.2-failshot.png" "$(failshot_path /out v1.2.yaml)"

check "the default wallpaper is the gradient" \
    "-f lavfi -i gradients=s=1920x1080:c0=0x1e3c72:c1=0x7f53ac:x0=0:y0=0:x1=1920:y1=1080" \
    "$(wallpaper_input "" | tr '\n' ' ' | sed 's/ $//')"
check "a color wallpaper fills the screen" \
    "-f lavfi -i color=c=0x1e1e2e:s=1920x1080" \
    "$(wallpaper_input "#1e1e2e" | tr '\n' ' ' | sed 's/ $//')"
touch "$work/brand.png"
check "an image wallpaper is scaled to fill and cropped" \
    "-i $work/brand.png -vf scale=1920:1080:force_original_aspect_ratio=increase,crop=1920:1080" \
    "$(wallpaper_input "$work/brand.png" | tr '\n' ' ' | sed 's/ $//')"
for bad in "#12345" "#12345g"; do
    check "the wallpaper color $bad is rejected" "::error::the wallpaper color $bad is not #rrggbb" \
        "$( (wallpaper_input "$bad") 2>&1 || true)"
done
check "a missing wallpaper image is rejected" "::error::no wallpaper image at $work/missing.png" \
    "$( (wallpaper_input "$work/missing.png") 2>&1 || true)"

[ "$failures" -eq 0 ] || { echo "$failures failed"; exit 1; }
echo "all passed"
