# dsh-wallpaper-bridge

Companion DeepSeek Harness plugin for `dsh-wallpaper`. It exposes a versioned,
loopback-only REST/SSE surface backed by standard DSH agents and sessions.

The status route is intentionally public so the wallpaper can distinguish a
plain DSH web server from an installed bridge. All session routes require the
random bearer token stored in `$DSH_HOME/wallpaper/bridge-token` (or
`~/.dsh/wallpaper/bridge-token`). The token is consumed by the native wallpaper
process and must never be exposed to its WebView or logs.
