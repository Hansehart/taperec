#!/bin/sh
# Runs cargo in the dev toolchain image, with --locked for resolving subcommands.
set -e
docker build -q -t taperec-dev -f dev.Dockerfile . >/dev/null
[ $# -gt 0 ] || set -- test
command=$1
shift
case $command in
    build | check | clippy | doc | run | test) set -- "$command" --locked "$@" ;;
    *) set -- "$command" "$@" ;;
esac
# Doc warnings fail like code warnings.
RUSTDOCFLAGS=${RUSTDOCFLAGS:--D warnings}
export RUSTDOCFLAGS
exec docker run --rm -e RUSTDOCFLAGS -v "$PWD":/src -v taperec-target:/src/target -w /src \
    taperec-dev cargo "$@"
