#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
project_root="$(cd -- "${script_dir}/.." && pwd)"
unit_dir="${XDG_CONFIG_HOME:-${HOME}/.config}/systemd/user"
application_dir="${XDG_DATA_HOME:-${HOME}/.local/share}/applications"
state_dir="${XDG_STATE_HOME:-${HOME}/.local/state}/screenshot"
unit_path="${unit_dir}/screenshot-daemon.service"
shortcut_backup="${state_dir}/print-shortcuts.json"
kwin_package="${project_root}/kwin/screenshot-wayland-trigger"
kwin_plugin_id="screenshot-wayland-trigger"
daemon_pattern='[/]screenshot-daemon'

required_commands=(cargo kbuildsycoca6 kpackagetool6 kwriteconfig6 qdbus6 systemctl)
for command_name in "${required_commands[@]}"; do
	if ! command -v "${command_name}" >/dev/null 2>&1; then
		echo "Missing required command: ${command_name}" >&2
		exit 1
	fi
done

if [[ "${XDG_SESSION_TYPE:-}" != "wayland" ]]; then
	echo "WARNING: this package targets KDE Plasma Wayland; XDG_SESSION_TYPE=${XDG_SESSION_TYPE:-unset}." >&2
fi

if ! qdbus6 org.kde.KWin /KWin org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1; then
	echo "An active KDE Plasma 6 session is required to install the KWin integration." >&2
	exit 1
fi

echo "Building Wayland screenshot binaries..."
cargo build --manifest-path "${project_root}/Cargo.toml" --release --bins

mkdir -p "${unit_dir}" "${application_dir}" "${state_dir}"
sed "s|__PROJECT_ROOT__|${project_root}|g" \
	"${project_root}/deploy/systemd/screenshot-daemon.service" >"${unit_path}"

for desktop_name in \
	io.github.terrydaktal.screenshot.desktop \
	io.github.terrydaktal.screenshot-daemon.desktop; do
	sed "s|__PROJECT_ROOT__|${project_root}|g" \
		"${project_root}/deploy/applications/${desktop_name}" >"${application_dir}/${desktop_name}"
done
kbuildsycoca6 >/dev/null

if kpackagetool6 --type=KWin/Script --show "${kwin_plugin_id}" >/dev/null 2>&1; then
	kpackagetool6 --type=KWin/Script --upgrade "${kwin_package}" >/dev/null
else
	kpackagetool6 --type=KWin/Script --install "${kwin_package}" >/dev/null
fi

# Force a clean reload so upgrades take effect without restarting KWin.
kwriteconfig6 --file kwinrc --group Plugins --key "${kwin_plugin_id}Enabled" false
qdbus6 org.kde.KWin /KWin reconfigure >/dev/null
script_unloaded=false
for _ in {1..20}; do
	if [[ "$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.isScriptLoaded "${kwin_plugin_id}" 2>/dev/null)" != "true" ]]; then
		script_unloaded=true
		break
	fi
	sleep 0.1
done
if [[ "${script_unloaded}" != "true" ]]; then
	echo "KWin did not unload the previous ${kwin_plugin_id} instance." >&2
	exit 1
fi

kwriteconfig6 --file kwinrc --group Plugins --key "${kwin_plugin_id}Enabled" true
qdbus6 org.kde.KWin /KWin reconfigure >/dev/null
script_loaded=false
for _ in {1..30}; do
	if [[ "$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.isScriptLoaded "${kwin_plugin_id}" 2>/dev/null)" == "true" ]]; then
		script_loaded=true
		break
	fi
	sleep 0.1
done
if [[ "${script_loaded}" != "true" ]]; then
	echo "KWin did not load ${kwin_plugin_id}." >&2
	exit 1
fi
"${project_root}/target/release/screenshot-shortcut-setup" --install "${shortcut_backup}"

echo "Stopping existing screenshot daemon instances..."
systemctl --user stop screenshot-daemon.service 2>/dev/null || true
systemctl --user kill screenshot-daemon.service 2>/dev/null || true
for _ in {1..10}; do
	if ! pgrep -f "${daemon_pattern}" >/dev/null 2>&1; then
		break
	fi
	pkill -TERM -f "${daemon_pattern}" 2>/dev/null || true
	sleep 0.15
done
if pgrep -f "${daemon_pattern}" >/dev/null 2>&1; then
	pkill -KILL -f "${daemon_pattern}" 2>/dev/null || true
fi

systemctl --user import-environment \
	WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE 2>/dev/null || true
systemctl --user daemon-reload
systemctl --user enable --now screenshot-daemon.service

for _ in {1..20}; do
	if qdbus6 io.github.terrydaktal.Screenshot /io/github/terrydaktal/Screenshot org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1; then
		break
	fi
	sleep 0.1
done
if ! qdbus6 io.github.terrydaktal.Screenshot /io/github/terrydaktal/Screenshot org.freedesktop.DBus.Peer.Ping >/dev/null 2>&1; then
	echo "The daemon started but did not claim its D-Bus service." >&2
	systemctl --user --no-pager --full status screenshot-daemon.service >&2 || true
	exit 1
fi

echo "Installed Plasma Wayland integration and ${unit_path}"
systemctl --user --no-pager --full status screenshot-daemon.service || true
