#!/usr/bin/env bash
# Collects what Docker Desktop, the o3s containers and VS Code's Dev Containers logged.
set -uo pipefail

out=$1
docker=/Applications/Docker.app/Contents/Resources/bin/docker
mkdir -p "$out"
pgrep -lf -i docker > "$out/processes.txt" 2>&1
cp -R "$HOME/Library/Containers/com.docker.docker/Data/log" "$out/docker-desktop" 2>/dev/null
if [ -x "$docker" ]; then
  "$docker" ps -a > "$out/containers.txt" 2>&1
  for c in $("$docker" ps -a --format '{{.Names}}'); do
    perl -e 'alarm 30; exec @ARGV' "$docker" logs --timestamps "$c" > "$out/container-$c.log" 2>&1
  done
  cage=$("$docker" ps -q --filter label=com.docker.compose.service=cage)
  [ -n "$cage" ] && perl -e 'alarm 30; exec @ARGV' "$docker" exec "$cage" sh -c 'ls -la ~/.vscode-server ~/.vscode-server/bin 2>&1; ps aux' > "$out/cage-inside.txt" 2>&1
fi
cp -R "$HOME/Library/Application Support/Code/logs" "$out/vscode-logs" 2>/dev/null
true
