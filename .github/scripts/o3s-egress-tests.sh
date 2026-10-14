#!/usr/bin/env bash
# Tries the cage's egress directly and through the bridge, including a static neighbour pinned to the bridge's MAC.
set -uo pipefail

cage() { docker exec o3s-cage-1 "$@"; }
dump() { printf '\n### %s\n' "$*"; "$@" 2>&1; }
code() { cage curl -sS --connect-timeout 10 -o /dev/null -w '%{http_code}\n' "$@"; }

bridge="br-$(docker network inspect o3s-cage --format '{{.Id}}' | cut -c1-12)"
mac=$(docker run --rm --privileged --pid=host alpine:3 nsenter -t 1 -n -- ip -o link show "$bridge" | grep -oE 'link/ether [0-9a-f:]+' | cut -d' ' -f2)
dump echo "bridge=$bridge mac=$mac"
dump docker network inspect o3s-cage --format '{{json .Internal}} {{json .Options}} {{json .IPAM.Config}}'
dump docker run --rm --privileged --pid=host alpine:3 nsenter -t 1 -n -- ip addr show "$bridge"

dump cage ip route
dump code https://github.com
dump code https://1.1.1.1

dump cage ip route replace default via 10.10.0.1
dump code https://1.1.1.1

dump cage ip neigh replace 10.10.0.1 lladdr "$mac" dev eth0 nud permanent
dump cage ip neigh
dump code https://1.1.1.1
dump code https://example.org

dump cage ip neigh del 10.10.0.1 dev eth0
dump cage ip route replace default via 10.10.0.2
dump code https://github.com
