#!/usr/bin/env bash
set -euo pipefail

if ! command -v kwriteconfig6 >/dev/null 2>&1 || ! command -v qdbus6 >/dev/null 2>&1; then
	echo "Missing required commands: kwriteconfig6 and qdbus6" >&2
	exit 1
fi

kwriteconfig6 \
	--file kwinrc \
	--group Script-screenshot-wayland-trigger \
	--key disableAnimations true
qdbus6 org.kde.KWin /KWin reconfigure >/dev/null
echo "Disabled screenshot overlay transitions."
