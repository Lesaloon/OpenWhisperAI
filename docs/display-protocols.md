# Wayland and X11 Notes

OpenWhisperAI should behave consistently on both Wayland and X11. Use the
guidance below when testing Linux builds.

## Detecting the Session

- Wayland sessions typically set `XDG_SESSION_TYPE=wayland` and `WAYLAND_DISPLAY`.
- X11 sessions typically set `XDG_SESSION_TYPE=x11` and `DISPLAY`.

## Things to Verify on Wayland

- Window creation and focus changes, especially for dialogs and file pickers.
- Clipboard operations for copy/paste between OpenWhisperAI and other apps.
- Screen capture or microphone prompts if the app uses portals.
- Drag and drop behaviors across windows.
- Fractional scaling and high-DPI rendering.

## Things to Verify on X11

- Multi-monitor placement and window restore positions.
- Global shortcuts or media keys if the app registers them.
- Window decoration styles provided by the window manager.

## Troubleshooting Tips

- To force X11 on Wayland systems, launch with `GDK_BACKEND=x11` or the
  equivalent backend flag for the UI toolkit in use.
- For Wayland-specific issues, confirm the portal services are installed and
  running (xdg-desktop-portal and the session backend).
