#!/bin/sh
# Writes lucide.ttf: the icons taperec draws, cut from a Lucide font release with fonttools' pyftsubset.
set -eu
version=1.61.0
# fast-forward and zoom-in; codepoints from the release's lucide-font/codepoints.json.
unicodes=U+E0BD,U+E1B6
cd "$(dirname "$0")"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl --proto '=https' --tlsv1.2 -fsSL -o "$work/lucide.zip" \
    "https://github.com/lucide-icons/lucide/releases/download/$version/lucide-font-$version.zip"
unzip -q -j "$work/lucide.zip" lucide-font/lucide.ttf -d "$work"
# The font goes in on stdin and the subset comes back on stdout, so the container needs no mounts.
docker run --rm -i python:3.13-slim@sha256:57b29a7c050c4e679e4bea572e14b3965cf4b5839de2da49d55e49ffc4024f39 \
    sh -c "pip install -q --disable-pip-version-check --root-user-action=ignore fonttools==4.67.0 >&2 &&
        cat > /tmp/lucide.ttf &&
        pyftsubset /tmp/lucide.ttf --unicodes=$unicodes --output-file=/tmp/subset.ttf >&2 &&
        cat /tmp/subset.ttf" < "$work/lucide.ttf" > "$work/subset.ttf"
cp "$work/subset.ttf" lucide.ttf
