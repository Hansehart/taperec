#!/usr/bin/env bash
# Types the probe tape's lines into a cat in Terminal with the typer, holding each key for the given time.
set -uo pipefail

hold=$1
printf '#!/bin/sh\nexec cat > /tmp/typed.txt\n' > /tmp/cat.command
chmod +x /tmp/cat.command
open -a Terminal /tmp/cat.command
sleep 4
sed -n 's/^Type "\(.*\)"$/\1/p' tutorials/typing-probe.tape | tail -n +2 | while IFS= read -r line; do
  "$RUNNER_TEMP/typer" "$hold" 35 "$line"
done
