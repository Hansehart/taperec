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

check "Intel runners switch off the icon agent" "yes" "$(crashing_icon_agent x86_64-apple-darwin && echo yes)"
check "Apple silicon keeps the icon agent" "no" "$(crashing_icon_agent aarch64-apple-darwin || echo no)"
mkdir -p "$work/bin"
printf '#!/bin/sh\necho "launchctl $*" >> "%s/calls"\n' "$work" > "$work/bin/launchctl"
chmod +x "$work/bin/launchctl"
(PATH="$work/bin:$PATH" icon_agent_off)
check "the icon agent is booted out and disabled" \
    "launchctl bootout gui/$(id -u)/com.apple.iconservices.iconservicesagent
launchctl disable gui/$(id -u)/com.apple.iconservices.iconservicesagent" \
    "$(cat "$work/calls")"

check "failshots drop the tape extension" "/out/intro-failshot.png" "$(failshot_path /out tutorials/intro.yaml)"
check "failshots keep dots inside the name" "/out/v1.2-failshot.png" "$(failshot_path /out v1.2.yaml)"

[ "$failures" -eq 0 ] || { echo "$failures failed"; exit 1; }
echo "all passed"
