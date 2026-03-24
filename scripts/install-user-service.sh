#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
TEMPLATE_PATH="${PROJECT_ROOT}/deploy/systemd/screenshot-daemon.service"
UNIT_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
UNIT_PATH="${UNIT_DIR}/screenshot-daemon.service"
DAEMON_PATTERN="[/]screenshot-daemon"

if [[ ! -f "${TEMPLATE_PATH}" ]]; then
	echo "Service template not found: ${TEMPLATE_PATH}" >&2
	exit 1
fi

mkdir -p "${UNIT_DIR}"
sed "s|__PROJECT_ROOT__|${PROJECT_ROOT}|g" "${TEMPLATE_PATH}" >"${UNIT_PATH}"

kill_existing_daemons() {
	local attempts=0

	while ((attempts < 10)); do
		if ! pgrep -f "${DAEMON_PATTERN}" >/dev/null 2>&1; then
			return 0
		fi

		pkill -TERM -f "${DAEMON_PATTERN}" 2>/dev/null || true
		sleep 0.15
		((attempts += 1))
	done

	pkill -KILL -f "${DAEMON_PATTERN}" 2>/dev/null || true
	sleep 0.1
}

echo "Stopping existing screenshot daemon instances..."
# Stop managed service first (if present/running).
systemctl --user stop screenshot-daemon.service 2>/dev/null || true
systemctl --user kill screenshot-daemon.service 2>/dev/null || true

# Kill any manually started daemons so reruns are idempotent.
kill_existing_daemons

systemctl --user daemon-reload
systemctl --user enable --now screenshot-daemon.service

sleep 0.2
running_count="$(pgrep -f "${DAEMON_PATTERN}" | wc -l | tr -d ' ')"
if [[ "${running_count}" != "1" ]]; then
	echo "WARNING: expected 1 running screenshot-daemon, found ${running_count}." >&2
	echo "Active matching processes:" >&2
	pgrep -a -f "${DAEMON_PATTERN}" >&2 || true
	echo "If one is root-owned, stop it with: sudo pkill -f screenshot-daemon" >&2
fi

echo "Installed: ${UNIT_PATH}"
echo "Service status:"
systemctl --user --no-pager --full status screenshot-daemon.service || true
echo
echo "Follow logs:"
echo "journalctl --user -u screenshot-daemon.service -f"
