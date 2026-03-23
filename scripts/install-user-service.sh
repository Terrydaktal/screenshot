#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
TEMPLATE_PATH="${PROJECT_ROOT}/deploy/systemd/screenshot-daemon.service"
UNIT_DIR="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
UNIT_PATH="${UNIT_DIR}/screenshot-daemon.service"

if [[ ! -f "${TEMPLATE_PATH}" ]]; then
  echo "Service template not found: ${TEMPLATE_PATH}" >&2
  exit 1
fi

mkdir -p "${UNIT_DIR}"
sed "s|__PROJECT_ROOT__|${PROJECT_ROOT}|g" "${TEMPLATE_PATH}" > "${UNIT_PATH}"

systemctl --user daemon-reload
systemctl --user enable --now screenshot-daemon.service

echo "Installed: ${UNIT_PATH}"
echo "Service status:"
systemctl --user --no-pager --full status screenshot-daemon.service || true
echo
echo "Follow logs:"
echo "journalctl --user -u screenshot-daemon.service -f"
