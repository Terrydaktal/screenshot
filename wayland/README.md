# screenshot for KDE Plasma 6 Wayland

`screenshot` is a fast, capture-first screenshot and annotation tool for KDE
Plasma 6 running natively on Wayland.

## Why this tool exists

Some screenshot flows fail around transient UI for two separate reasons:

1. **The trigger never reaches the screenshot tool.** A focused application or
   popup can consume PrintScreen. With some right-click menus open, this looks
   like pressing PrintScreen and seeing no crop overlay at all. A terminal can
   also react to the key, for example by jumping to the bottom before content in
   its scrollback buffer is captured.
2. **The trigger works, but capture happens too late.** Creating and focusing a
   crop overlay dismisses menus and other transient UI. Capturing after the
   overlay appears therefore records the changed desktop rather than the state
   that was visible when PrintScreen was pressed.

The Wayland implementation handles both failure points:

- A KWin script owns the compositor-level `Print` global shortcut. KWin sees the
  shortcut before a Wayland client or its popup can consume it.
- The shortcut calls an already-running daemon over D-Bus, avoiding process
  startup work on the trigger path.
- The daemon immediately requests the current workspace pixels from KWin's
  `org.kde.KWin.ScreenShot2` interface.
- Only after KWin has frozen and returned that frame does the daemon launch the
  overlay and stream the frame to it.

For example, you may see the application launcher or a context menu close when
the crop overlay takes focus, but it remains visible in the image being cropped
because the image was captured first.

The daemon and installed KWin shortcut are required for normal operation. Do
not add a second desktop shortcut, `xbindkeys` binding, or direct launcher for
PrintScreen; parallel trigger paths cause duplicate crop flows and do not
preserve the same capture ordering.

## Features

- Native KDE Plasma 6 Wayland capture through KWin, not XWayland screen scraping.
- Compositor-level PrintScreen trigger that works while popups are focused.
- Capture-before-overlay ordering for right-click menus and transient UI.
- Resident daemon and in-memory frame transfer for low trigger latency.
- DPI-aware whole-workspace overlay and crop selection.
- Move and resize the selection with matching resize cursors.
- Freehand pen, rectangle, straight-line, and arrow annotation tools.
- Color selector, on-screen eyedropper, annotation size slider, and undo.
- Annotations remain anchored when a selection moves or resizes and reappear if
  a temporarily smaller crop is enlarged again.
- Copy button, `Ctrl+C`, `Ctrl+X`, or `Enter` to copy the current selection.
- Save button or `Ctrl+S` to write a timestamped PNG under
  `~/Pictures/Screenshots`.
- `Esc` to cancel.
- Wayland clipboard integration with CopyQ history support.
- No temporary capture files.
- Per-tool KWin animation enable/disable scripts.

## Requirements

- KDE Plasma 6 on a Wayland session.
- KWin 6 with `org.kde.KWin.ScreenShot2` version 5.
- Rust and Cargo to build from source.
- Plasma tools: `kpackagetool6`, `kbuildsycoca6`, `kwriteconfig6`, and `qdbus6`.
- systemd user services.
- `wl-copy` from `wl-clipboard` for the preferred native clipboard path.
- CopyQ is optional. When running, it records the `wl-copy` clipboard update in
  its history; it is also used as the fallback writer if `wl-copy` fails.

The installer is per-user and does not require root access.

## Build

```bash
cd /path/to/screenshot/wayland
cargo build --release --bins
```

This creates:

- `target/release/screenshot`: crop and annotation overlay.
- `target/release/screenshot-daemon`: resident capture service.
- `target/release/screenshot-shortcut-setup`: installer helper that assigns
  PrintScreen to the KWin script and releases that key from conflicting actions.

## Install and start at login

For normal use, run:

```bash
cd /path/to/screenshot/wayland
./scripts/install-user-service.sh
```

This command intentionally takes ownership of `Print`, starts the screenshot
daemon, and changes the active desktop configuration. Do not run it until you
want `screenshot` to replace the current PrintScreen handler.

The script builds all release binaries, installs KWin screenshot authorization,
installs and enables the KWin script, assigns `Print`, installs the systemd user
unit, stops duplicate daemon processes, and enables and starts the service.
Before changing any conflicting `Print` assignments, it saves their complete
key lists in `$XDG_STATE_HOME/screenshot/print-shortcuts.json`, or under
`~/.local/state/screenshot/` when `XDG_STATE_HOME` is unset, so uninstall can
restore them exactly. The service is enabled under the user manager's
`default.target`, so it starts at each graphical login. A compositor and user
session do not exist at early machine boot, so "start at boot" for this desktop
tool means start automatically when that user logs in.

Check it with:

```bash
systemctl --user status screenshot-daemon.service
journalctl --user -u screenshot-daemon.service -f
```

Press `Print` to capture. The daemon is the only supported normal trigger path.

## Remove from the desktop

To stop and disable the daemon, remove the installed Plasma integration, and
restore the global shortcuts saved before installation:

```bash
./scripts/uninstall-user-service.sh
```

The uninstaller does not delete the repository or release binaries. If shortcut
restoration cannot connect to KGlobalAccel, it retains the backup and reports
its path instead of guessing or silently discarding the previous bindings.

## Run one time

The Plasma integration must be installed once because KWin restricts screenshot
access and owns global shortcuts. After running the installer at least once, a
foreground daemon can be used for one-session testing:

```bash
systemctl --user stop screenshot-daemon.service
./target/release/screenshot-daemon
```

Press `Ctrl+C` in that terminal to stop the foreground daemon. Restart the
installed service afterward with:

```bash
systemctl --user start screenshot-daemon.service
```

Running `target/release/screenshot` directly is useful only for debugging the
capture and overlay; it is not the normal PrintScreen path.

## Capture pipeline

The exact runtime order is:

1. KWin activates `ScreenshotWaylandCapture` when `Print` is pressed.
2. `kwin/screenshot-wayland-trigger/contents/code/main.js` asynchronously calls
   `io.github.terrydaktal.Screenshot.Trigger` on session D-Bus.
3. The resident daemon rejects duplicate triggers while an overlay is active.
4. The daemon calls KWin `CaptureWorkspace` with native-resolution output and a
   Unix file descriptor.
5. KWin renders the compositor scene and writes raw QImage pixels through that
   descriptor. The daemon converts them to RGBA in memory.
6. The daemon starts the sibling `screenshot` binary and pipes width, height,
   scale, and RGBA pixels through stdin.
7. The KWin script identifies the overlay by application ID, spans it across the
   virtual workspace, keeps it above normal windows, and focuses it.
8. The user crops and optionally annotates the already-frozen frame.
9. Copy encodes PNG in memory and writes it to `wl-copy`; save writes the final
   image to `~/Pictures/Screenshots`.

KWin's capture call does not use X11 frame retries. It returns the compositor's
completed frame through its supported Wayland screenshot API. Daemon logs include
the compositor capture dimensions, scale, and elapsed time.

Verify the authorized capture path without opening an overlay:

```bash
./target/release/screenshot-daemon --capture-test
```

This captures one frame into memory, prints its dimensions and elapsed time,
discards it, and exits without changing the clipboard or desktop.

## Clipboard and CopyQ

Copying uses this order:

1. `wl-copy --type image/png` becomes the native Wayland clipboard owner.
2. A running CopyQ instance observes that clipboard update and stores it in
   history.
3. If `wl-copy` is unavailable or exits unsuccessfully, `copyq copy image/png -`
   is used as a direct fallback.

The tool still runs and can save PNG files when CopyQ is not installed. For copy
support without CopyQ, install `wl-clipboard`.

## Temporary files

The runtime creates **zero temporary image files**:

- KWin sends raw capture data through a Unix file descriptor.
- The daemon sends RGBA data to the overlay through stdin.
- The overlay sends encoded PNG data to `wl-copy` or CopyQ through stdin.
- There is no temporary-file cleanup phase.

Only an explicit Save action writes an image file. The installer separately
creates persistent configuration files under the user's systemd, applications,
and KWin script directories; those are installation artifacts, not temporary
captures.

## Animation controls

Disable screenshot overlay close transitions:

```bash
./scripts/disable-screenshot-animations.sh
```

Re-enable them:

```bash
./scripts/enable-screenshot-animations.sh
```

The setting applies only to windows identified as `screenshot`; it does not
change the global Plasma animation speed.

## Project structure

```text
wayland/
├── assets/icons/                 # Embedded SVG toolbar icons
├── deploy/applications/          # ScreenShot2 authorization templates
├── deploy/systemd/               # Daemon user-unit template
├── kwin/screenshot-wayland-trigger/
│   ├── metadata.json             # Plasma 6 KWin package metadata
│   └── contents/code/main.js     # Shortcut and overlay window integration
├── scripts/
│   ├── install-user-service.sh   # Build and install all user integration
│   ├── uninstall-user-service.sh # Remove integration and restore shortcuts
│   ├── disable-screenshot-animations.sh
│   └── enable-screenshot-animations.sh
├── src/
│   ├── capture.rs                # KWin ScreenShot2 capture and pixel conversion
│   ├── daemon.rs                 # D-Bus trigger service and overlay launcher
│   ├── main.rs                   # Crop, annotation, copy, and save UI
│   └── shortcut_setup.rs         # KGlobalAccel Print assignment helper
├── Cargo.toml
└── README.md
```

## Script inputs and outputs

- `scripts/install-user-service.sh`
  - Input: repository location, current user home/XDG directories, active Plasma
    session D-Bus, and the current `Print` shortcut assignments.
  - Output: release binaries; two generated `.desktop` authorization entries;
    an installed KWin script; an enabled `screenshot-daemon.service`; and `Print`
    assigned exclusively to the screenshot KWin action.
- `scripts/uninstall-user-service.sh`
  - Input: installed user integration and the pre-install shortcut backup.
  - Output: stopped/disabled daemon, removed KWin and desktop integration, and
    restored original global shortcuts when the backup is available.
- `scripts/disable-screenshot-animations.sh`
  - Input: current user's `kwinrc`.
  - Output: `disableAnimations=true` for this KWin script followed by a KWin
    reconfigure request.
- `scripts/enable-screenshot-animations.sh`
  - Input: current user's `kwinrc`.
  - Output: `disableAnimations=false` for this KWin script followed by a KWin
    reconfigure request.

## Troubleshooting

If `Print` does nothing, inspect both the daemon and KWin script:

```bash
systemctl --user status screenshot-daemon.service
journalctl --user -u screenshot-daemon.service -n 100 --no-pager
journalctl --user -b _COMM=kwin_wayland | rg screenshot-wayland-trigger
```

Re-run `./scripts/install-user-service.sh` after moving the repository because
the service and KWin authorization entries contain the absolute release-binary
paths.

If two overlays appear, remove the other PrintScreen assignment and re-run the
installer. Do not run the X11 daemon and Wayland daemon together.
