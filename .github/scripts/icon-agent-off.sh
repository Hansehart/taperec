#!/usr/bin/env bash
# Switches off the crash-looping per-user icon agent so icon requests fail fast (actions/runner-images#14751).
set -uo pipefail

limited() { perl -e 'alarm shift; exec @ARGV' "$@"; }
label="gui/$(id -u)/com.apple.iconservices.iconservicesagent"
SECONDS=0
limited 30 launchctl bootout "$label"; echo "bootout exit $? after ${SECONDS}s"
limited 30 launchctl disable "$label"; echo "disable exit $? after ${SECONDS}s"
limited 15 launchctl print "$label" 2>&1 | grep -E 'state|runs|crashes' || echo "the agent is not loaded"
# A file icon now returns at once instead of waiting on the agent.
SECONDS=0
limited 60 osascript -l JavaScript -e 'ObjC.import("AppKit"); $.NSWorkspace.sharedWorkspace.iconForFile("/Applications").size.width' \
    && echo "icon request returned after ${SECONDS}s" || echo "icon request still blocked after ${SECONDS}s"
