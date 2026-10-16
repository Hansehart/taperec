#!/usr/bin/env bash
# Compares what reached the file with what the typing probe tape typed, line by line.
set -uo pipefail

expected=$(sed -n 's/^Type "\(.*\)"$/\1/p' tutorials/typing-probe.tape | tail -n +2 | sed 's/\\"/"/g')
diff <(printf '%s\n' "$expected") /tmp/typed.txt && echo "all lines intact" || { echo "::warning::typed lines differ"; exit 1; }
