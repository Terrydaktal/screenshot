# screenshot

`screenshot` is a fast screen capture tool for Linux (X11).

## Why this tool exists
This tool was needed because Flameshot could not reliably capture right-click context menus in this workflow.

Right-click menus are short-lived popup windows. They can disappear when focus changes, and many screenshot tools show their UI first or add enough launch delay that the menu is gone before pixels are captured.

This tool works because it captures first:
- It grabs the screen frame immediately when the binary starts.
- Only after the frame is captured does it show the crop UI.
- It releases X11 pointer/keyboard grabs after capture, which keeps selection interaction stable when a context menu had focus.
- The daemon reads key events from `/dev/input/event*` (evdev), so it still triggers when context menus grab input inside X11.
- Normal X11-level shortcut handlers can be blocked by menu grabs, which is why this workflow depends on the daemon path.

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
cargo build --release
```

## Daemon-only workflow
`screenshot-daemon` is the required launcher. It starts `screenshot` for each capture.

Manual `./target/release/screenshot` runs are only for debugging, not normal use.

## Run the daemon
The daemon listens to keyboard events from `/dev/input/event*` and launches the screenshot tool when configured keys are pressed.

Run in foreground (good for testing):
```bash
sudo ./target/release/screenshot-daemon
```

Run in background:
```bash
nohup ./target/release/screenshot-daemon > daemon.log 2>&1 &
```

Restart daemon:
```bash
pkill -f screenshot-daemon
nohup ./target/release/screenshot-daemon > daemon.log 2>&1 &
```

Verify daemon is running:
```bash
pgrep -af screenshot-daemon
tail -n 20 daemon.log
```

Recommended cleanup (avoid duplicate launches):
```bash
pkill -f xbindkeys
```

Also remove any desktop PrintScreen shortcut that launches `screenshot` directly.

If daemon cannot read `/dev/input/event*`, run it with `sudo` or grant your user access to the `input` devices.
