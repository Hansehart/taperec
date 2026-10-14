#!/usr/bin/env bash
# Logs, once a second from the cage's first moment, its default route and whether 1.1.1.1 answers.
set -uo pipefail

until="$1"
end=$((SECONDS + until))
while [ "$SECONDS" -lt "$end" ]; do
  id=$(docker ps -q --filter label=com.docker.compose.project=o3s --filter label=com.docker.compose.service=cage)
  if [ -n "$id" ]; then
    route=$(docker exec "$id" ip route show default 2>&1 | head -1)
    code=$(docker exec "$id" curl -s --connect-timeout 2 -o /dev/null -w '%{http_code}' https://1.1.1.1 2>&1)
    printf '%s\troute=%s\t1.1.1.1=%s\n' "$(date -u +%H:%M:%S)" "$route" "$code"
  fi
  sleep 1
done
