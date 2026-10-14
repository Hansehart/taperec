#!/usr/bin/env bash
# Dumps how a running o3s stack routes, filters and logs traffic, for side-by-side comparison.
set -uo pipefail

cage() { docker exec o3s-cage-1 "$@"; }
gateway() { docker exec o3s-gateway-1 "$@"; }
dump() { printf '\n### %s\n' "$*"; "$@" 2>&1; }

dump docker version
dump docker ps -a --format '{{.Names}}\t{{.Status}}\t{{.Networks}}'
dump docker network inspect o3s-cage o3s-egress
dump docker inspect o3s-cage-1 --format '{{json .HostConfig.SecurityOpt}} {{json .HostConfig.CapAdd}} {{json .AppArmorProfile}} {{json .HostConfig.Dns}}'
dump docker logs o3s-cage-1
dump cage ip addr
dump cage ip route
dump cage ip route get 1.1.1.1
dump cage cat /etc/resolv.conf
dump cage curl -sSv --connect-timeout 5 -o /dev/null https://1.1.1.1
dump cage curl -sSv --connect-timeout 5 -o /dev/null http://1.1.1.1
dump cage curl -sSv --connect-timeout 10 -o /dev/null https://github.com
dump cage curl -sSv --connect-timeout 5 -o /dev/null https://example.com
dump gateway ip addr
dump gateway ip route
dump gateway sysctl net.ipv4.ip_forward
dump gateway iptables-save
dump gateway ipset list
dump docker logs o3s-gateway-1
dump docker logs o3s-proxy-1
