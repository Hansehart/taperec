#!/usr/bin/env bash
# Shows what runs in the cage, who may change its route, and where the bridge gateway leads.
set -uo pipefail

cage() { docker exec o3s-cage-1 "$@"; }
dump() { printf '\n### %s\n' "$*"; "$@" 2>&1; }

dump docker inspect o3s-cage-1 --format '{{json .Path}} {{json .Args}} {{json .Config.Cmd}} {{json .Config.Entrypoint}}'
dump docker network inspect o3s-cage --format '{{json .IPAM.Config}} {{json .Options}}'
# shellcheck disable=SC2016 # expands inside the cage
dump cage sh -c 'for p in /proc/[0-9]*; do printf "%s: " "${p#/proc/}"; tr "\0" " " < "$p/cmdline"; echo; done'
dump docker exec -u ubuntu o3s-cage-1 sudo -n true
dump docker exec -u ubuntu o3s-cage-1 grep -E '^Cap(Inh|Prm|Eff|Bnd|Amb)' /proc/self/status
dump cage grep -E '^Cap(Inh|Prm|Eff|Bnd|Amb)' /proc/self/status

bridge=$(docker network inspect o3s-cage --format '{{range .IPAM.Config}}{{.Gateway}}{{end}}')
bridge=${bridge:-10.10.0.1}
dump cage ip route
dump cage curl -sS --connect-timeout 5 -o /dev/null -w '%{http_code}\n' https://1.1.1.1
dump cage ip route replace default via "$bridge"
dump cage ip route
dump cage curl -sS --connect-timeout 5 -o /dev/null -w '%{http_code}\n' https://1.1.1.1
dump cage curl -sS --connect-timeout 5 -o /dev/null -w '%{http_code}\n' https://example.org
dump cage ip route replace default via 10.10.0.2
dump cage ip route
