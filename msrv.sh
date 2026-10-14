#!/bin/sh
# Compiles everything with the minimum Rust from Cargo.toml's rust-version.
set -eu
msrv=$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml)
[ -n "$msrv" ] || { echo "msrv: no rust-version in Cargo.toml" >&2; exit 1; }
# Uses the image's own toolchain, offline.
exec docker run --rm -e RUSTUP_AUTO_INSTALL=0 -e CARGO_TARGET_DIR=/tmp/target \
    -v "$PWD":/src:ro -w /src "rust:$msrv-slim-bookworm" sh -euc '
        RUSTUP_TOOLCHAIN=$(rustup default | cut -d" " -f1) cargo check --locked --all-targets
        echo "msrv: compiles with $(rustc +"$(rustup default | cut -d" " -f1)" --version)"
    '
