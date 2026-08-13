#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "${script_dir}/.." && pwd)"
unit_dir="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
application_dir="${XDG_DATA_HOME:-${HOME}/.local/share}/applications"
state_dir="${XDG_STATE_HOME:-${HOME}/.local/state}/screenshot"
shortcut_backup="${state_dir}/print-shortcuts.json"
shortcut_helper="${project_root}/target/release/screenshot-shortcut-setup"
kwin_plugin_id="screenshot-wayland-trigger"
daemon_pattern='[/]screenshot-daemon'

required_commands=(cargo kbuildsycoca6 kpackagetool6 kwriteconfig6 systemctl)
for command_name in "${required_commands[@]}"; do
	if ! command -v "${command_name}" >/dev/null 2>&1; then
		echo "Missing required command: ${command_name}" >&2
		exit 1
	fi
done

echo "Stopping screenshot daemon instances..."
systemctl --user disable --now screenshot-daemon.service 2>/dev/null || true
pkill -TERM -f "${daemon_pattern}" 2>/dev/null || true

kwriteconfig6 --file kwinrc --group Plugins --key "${kwin_plugin_id}Enabled" false
if command -v qdbus6 >/dev/null 2>&1; then
	qdbus6 org.kde.KWin /KWin reconfigure >/dev/null 2>&1 || true
fi

if [[ ! -x "${shortcut_helper}" ]]; then
	cargo build --manifest-path "${project_root}/Cargo.toml" --release --bin screenshot-shortcut-setup
fi

shortcut_restored=false
if [[ -f "${shortcut_backup}" ]]; then
	if "${shortcut_helper}" --restore "${shortcut_backup}"; then
		shortcut_restored=true
		rm -f "${shortcut_backup}"
	else
		echo "WARNING: shortcut restoration failed; keeping ${shortcut_backup}." >&2
	fi
elif ! "${shortcut_helper}" --release; then
	echo "WARNING: could not release the screenshot Print shortcut." >&2
fi

kpackagetool6 --type=KWin/Script --remove "${kwin_plugin_id}" >/dev/null 2>&1 || true
rm -f \
	"${application_dir}/io.github.terrydaktal.screenshot.desktop" \
	"${application_dir}/io.github.terrydaktal.screenshot-daemon.desktop" \
	"${unit_dir}/screenshot-daemon.service"
systemctl --user daemon-reload
kbuildsycoca6 >/dev/null
kwriteconfig6 --file kwinrc --group Plugins --key "${kwin_plugin_id}Enabled" --delete
if command -v qdbus6 >/dev/null 2>&1; then
	qdbus6 org.kde.KWin /KWin reconfigure >/dev/null 2>&1 || true
fi

echo "Removed the screenshot Wayland service and Plasma integration."
if [[ "${shortcut_restored}" == "true" ]]; then
	echo "Restored the Print shortcuts saved before installation."
elif [[ -f "${shortcut_backup}" ]]; then
	echo "Shortcut backup retained at ${shortcut_backup}." >&2
fi
