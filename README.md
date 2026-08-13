# screenshot

`screenshot` is a capture-first screenshot and annotation tool with separate
native implementations for X11 and KDE Plasma 6 Wayland.

The former project has been retired intact under `x11/`. The active
implementation is under `wayland/`; it uses KWin rather than X11 compatibility
APIs and is the correct version for a Plasma 6 Wayland session.

## Choose an implementation

| Session | Directory | Status |
| --- | --- | --- |
| KDE Plasma 6 Wayland | `wayland/` | Active implementation |
| X11 | `x11/` | Retired, retained for compatibility |

Check the current session with:

```bash
printf '%s\n' "$XDG_SESSION_TYPE"
```

Then follow the README inside the matching directory. Do not run both daemons
or configure another PrintScreen launcher at the same time.

## Project structure

```text
.
├── README.md                  # Platform selector and repository overview
├── wayland/
│   ├── assets/icons/         # SVG annotation and output button icons
│   ├── deploy/applications/  # KWin ScreenShot2 authorization entries
│   ├── deploy/systemd/       # User-service template
│   ├── kwin/                 # Plasma 6 shortcut/window-management script
│   ├── scripts/              # Installer, uninstaller, and animation controls
│   ├── src/                  # Capture, daemon, overlay, and shortcut setup
│   ├── Cargo.toml
│   └── README.md
└── x11/
    ├── assets/icons/         # X11 overlay icons
    ├── deploy/systemd/       # X11 user-service template
    ├── scripts/              # X11 service and animation scripts
    ├── src/                  # Retired X11 capture, daemon, and key test
    ├── Cargo.toml
    └── README.md
```

## Wayland pipeline

The Plasma 6 path deliberately executes in this order:

1. KWin consumes the global `Print` shortcut before the focused application or
   popup can consume it.
2. The KWin script calls the already-running `screenshot-daemon` over session
   D-Bus.
3. The daemon immediately asks KWin `ScreenShot2` for the current compositor
   frame.
4. Only after that frame is frozen does the daemon start the crop overlay and
   pipe the pixels to it in memory.
5. The overlay crops and annotates that frozen frame, then pipes PNG data to
   `wl-copy` or CopyQ, or saves the final PNG under
   `~/Pictures/Screenshots`.

This ordering is what preserves right-click menus and other transient content.
See `wayland/README.md` for setup, controls, dependencies, troubleshooting, and
the complete input/output contract for each script.
