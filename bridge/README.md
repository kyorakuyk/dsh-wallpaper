# dsh-wallpaper-bridge

Companion DeepSeek Harness plugin for `dsh-wallpaper`. It exposes a versioned,
loopback-only REST/SSE surface backed by standard DSH agents and sessions.

The status route is intentionally public so the wallpaper can distinguish a
plain DSH web server from an installed bridge. All session routes require the
random bearer token stored in `$DSH_HOME/wallpaper/bridge-token` (or
`~/.dsh/wallpaper/bridge-token`). The token is consumed by the native wallpaper
process and must never be exposed to its WebView or logs.

## Desktop entry boundary

Wallpaper sessions receive a scoped `wallpaper:desktop-entry` system context.
It identifies the `桌面会话` workspace and active permission preset so the
model does not mistake the wallpaper composer for the full Harness Web UI.
New sessions default to the host's `workspace-write` permission preset and
dedicated workspace directory. Resumed sessions keep the user's explicit
permission choice. Tool approval that cannot be handled in the wallpaper is
handed off to Harness.

The host may override the initial preset with `desktopPermission`. This applies
only when creating a new desktop session; the wallpaper control surface remains
the place where the user changes the active session permission afterward.
