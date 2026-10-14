#!/usr/bin/env bash
# Reads the bridge firewall setting and tries the cage's egress directly, via the explicit proxy and via the bridge.
set -uo pipefail

cage() { docker exec o3s-cage-1 "$@"; }
dump() { printf '\n### %s\n' "$*"; "$@" 2>&1; }
code() { cage curl -sS --connect-timeout 10 -o /dev/null -w '%{http_code}\n' "$@"; }

# Host-level view of the Docker engine's network namespace (the VM on Docker Desktop).
# shellcheck disable=SC2016 # expands inside the engine's namespace
dump docker run --rm --privileged --pid=host alpine:3 nsenter -t 1 -m -n -- sh -c \
  'echo "bridge-nf-call-iptables=$(cat /proc/sys/net/bridge/bridge-nf-call-iptables 2>&1)"; iptables -L DOCKER-INTERNAL -v -n 2>&1'

dump code https://github.com
dump code https://1.1.1.1
dump code -x http://10.10.0.2:8080 https://github.com
dump code -x http://10.10.0.2:8080 https://example.org
dump code -x http://10.10.0.2:8080 https://1.1.1.1
dump cage ip route replace default via 10.10.0.1
dump code https://1.1.1.1
dump code -x http://10.10.0.2:8080 https://github.com
dump cage ip route replace default via 10.10.0.2
dump docker logs --tail 40 o3s-proxy-1
