#!/usr/bin/env bash
# Dumps the Docker engine's kernel, sysctls, daemon config and firewall, as seen from its own namespaces.
set -uo pipefail

engine() { printf '\n### engine: %s\n' "$1"; docker run --rm --privileged --pid=host alpine:3 nsenter -t 1 -m -u -n -i -- sh -c "$1" 2>&1; }

engine 'uname -a'
# shellcheck disable=SC2016 # expands inside the engine's namespace
engine 'for k in net/ipv4/ip_forward net/bridge/bridge-nf-call-iptables net/bridge/bridge-nf-call-ip6tables net/bridge/bridge-nf-call-arptables; do printf "%s=%s\n" "$k" "$(cat /proc/sys/$k 2>&1)"; done'
engine 'cat /etc/docker/daemon.json 2>&1; ls /etc/docker 2>&1'
engine 'lsmod 2>/dev/null | grep -E "br_netfilter|bridge|nf_" || echo "lsmod unavailable"'
engine 'iptables-save 2>&1'
engine 'nft list ruleset 2>&1 | head -200'
printf '\n### docker info\n'; docker info 2>&1
