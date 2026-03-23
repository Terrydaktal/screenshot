# Screencap - Rust Flameshot Clone

A high-performance screen capture tool for Linux (X11) that starts instantly and works even when right-click menus are open.

## Features
- **Instant Startup**: Built in Rust for near-zero latency.
- **Capture-First**: Grabs the screen state the millisecond the binary is executed.
- **Right-Click Drag**: Click and drag with the **Right Mouse Button** to select your crop.
- **Save Button**: Interactive button appears at the bottom-right of your selection.
- **Esc to Cancel**: Quickly exit without saving.
- **Auto-Dir**: Saves to `~/Pictures/Screenshots` with timestamps.

## Setup Instructions

1. **Build the binary**:
   ```bash
   cd /home/lewis/Dev/rs-screencap
   cargo build --release
   ```

2. **Test it**:
   ```bash
   ./target/release/rs-screencap
   ```

3. **Bind to Print Screen in Cinnamon**:
   - Open **System Settings** -> **Keyboard** -> **Shortcuts**.
   - **Custom Shortcuts** -> **Add custom shortcut**.
   - **Name**: `Rust Screenshot`
   - **Command**: `/home/lewis/Dev/rs-screencap/target/release/rs-screencap`
   - **Binding**: Set to **Print**.

## Why Rust?
Unlike the Python version, this compiled binary does not need to load a large interpreter or heavy library runtimes on every press. This results in an "instant" feel where the screen dims exactly when you press the key.
