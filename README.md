# screenshot

`screenshot` is a fast screen capture tool for Linux (X11).

## Why this tool exists
This tool was needed because Flameshot could not reliably capture right-click context menus in this workflow.

Right-click menus are short-lived popup windows. They can disappear when focus changes, and many screenshot tools show their UI first or add enough launch delay that the menu is gone before pixels are captured.

This tool works because it captures first:
- It grabs the screen frame immediately when the binary starts.
- Only after the frame is captured does it show the crop UI.
- It releases X11 pointer/keyboard grabs after capture, which keeps selection interaction stable when a context menu had focus.

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

## Run the screenshot tool
Run the interactive crop UI directly:
```bash
./target/release/screenshot
```

Hotkey command:
`/home/lewis/Dev/screenshot/target/release/screenshot`

Use only one trigger source for PrintScreen (Cinnamon shortcut, `xbindkeys`, or `screenshot-daemon`) to avoid duplicate prompts.

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

If daemon cannot read `/dev/input/event*`, run it with `sudo` or grant your user access to the `input` devices.
