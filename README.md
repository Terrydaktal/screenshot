# screenshot

`screenshot` is a fast screen capture tool for Linux (X11).

## Why this tool exists
This tool was needed because Flameshot could not reliably capture right-click context menus in this workflow.

Right-click menus are short-lived popup windows. They can disappear when focus changes, and many screenshot tools show their UI first or add enough launch delay that the menu is gone before pixels are captured.

This tool works because it handles two separate failure points:
- Failure point 1 (trigger): context menus can grab keyboard input inside X11 and steal PrintScreen from normal shortcut handlers.
- The daemon avoids that by reading `/dev/input/event*` (evdev), so the trigger still fires even when a menu is focused.
- Failure point 2 (timing): after trigger, the screen must be captured immediately.
- `screenshot` grabs the frame as soon as the binary starts, before showing any overlay UI.
- If overlay UI appears before capture, the context menu can lose focus and disappear from the image.
- Example: you may see the Start menu close on screen when the overlay appears, but it is still present in the crop image because the frame was captured first.
- After capture, it releases X11 pointer/keyboard grabs to keep crop interaction stable.

## Recommended operation
For reliable right-click menu capture, `screenshot-daemon` must be running and must be the only trigger path.

Do not keep parallel PrintScreen launchers active (desktop shortcut, `xbindkeys`, etc.), or you can get duplicate screenshot flows.

## Features
- **Instant startup**: Rust binary with low launch overhead.
- **Capture-first flow**: Freezes the exact screen state at trigger time.
- **Crop UI**: Click and drag to select an area.
- **Copy/Save actions**: Copy to clipboard or save to disk.
- **Esc to cancel**: Exit without saving.
- **Auto output path**: Saves to `~/Pictures/Screenshots` with timestamps.

## Build
```bash
cd /home/lewis/Dev/screenshot
cargo build --release --bins
```

## Daemon-only workflow
`screenshot-daemon` is the required launcher. It starts `screenshot` for each capture.

Manual `./target/release/screenshot` runs are only for debugging, not normal use.

## Service files in this repo
- `deploy/systemd/screenshot-daemon.service`: systemd user-service template.
- `scripts/install-user-service.sh`: installs/enables/starts the service for the current user.

## Run the daemon
The daemon listens to keyboard events from `/dev/input/event*` and launches `screenshot` when configured keys are pressed.

### Run one time (current session only)
Use this when testing or debugging. It does not persist across reboot/logout.

```bash
cd /home/lewis/Dev/screenshot
cargo build --release --bin screenshot-daemon
./target/release/screenshot-daemon
```

### Run as a service (starts automatically at boot/login)
Use this for normal operation. This installs a systemd user service and enables it.

```bash
cd /home/lewis/Dev/screenshot
cargo build --release --bin screenshot-daemon
./scripts/install-user-service.sh
```

Check/start/restart logs:
```bash
systemctl --user status screenshot-daemon.service
systemctl --user restart screenshot-daemon.service
journalctl --user -u screenshot-daemon.service -f
```

If you need it to run before login (true boot-time user service), enable linger:
```bash
loginctl enable-linger "$USER"
```

Recommended cleanup (avoid duplicate launches):
```bash
pkill -f xbindkeys
```

Also remove any desktop PrintScreen shortcut that launches `screenshot` directly.

If daemon cannot read `/dev/input/event*`, run it with `sudo` or grant your user access to the `input` devices.
