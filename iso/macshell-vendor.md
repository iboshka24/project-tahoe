# Vendored MacShell — provenance

The live desktop of the Tahoe profile is [MacShell](https://github.com/0-ss/MacShell)
(mit a matching [MacLock](https://github.com/0-ss/MacShell#maclock) lock screen),
a macOS-Tahoe-style menu bar + Dock + Spotlight + widgets for Hyprland, built
on [Quickshell](https://quickshell.org). It is MIT-licensed, so it can be
vendored here as-is with attribution.

| | |
|---|---|
| Upstream | `0-ss/MacShell`, branch `main` |
| Vendored commit | `eb3d89f9331cb18827b17cf4dd7e3d8c26bf433a` |
| Vendored on | 2026-10-18 |
| Vendored into | `iso/airootfs/etc/skel/.config/quickshell/{MacShell,MacLock}/` |
| License | MIT — `LICENSES/MIT.txt`, copyright Rickky; see REUSE.toml |
| Update tool | `./iso/vendor-macshell.sh [ref]` |

## What was NOT changed

Everything under those two directories is upstream's tree, shipped unmodified:

- `install.sh`, `macshell-update` and forked helpers are NOT part of the tree
  that ships — the ISO's session is integrated directly (see below), and auto
  updates from the live image make no sense.
- One upstream script ships verbatim because the upstream-owned Hyprland
  autostart expects it by name:
  `iso/airootfs/etc/skel/.local/bin/macshell-start` — the copy is byte-mostly
  identical to the file `install.sh` writes, minus nothing at all except this
  provenance comment.

## How the session is integrated

1. `~/.config/hypr/hyprland.lua` — the autostart, the Liquid Glass layer
   rules, the keybinds and `misc.allow_session_lock_restore`. The MacShell
   block sits between the same `>>> MacShell` markers that upstream's
   `install.sh` writes, so its installer can recognise and replace it later.
2. `~/.config/macshell/env` — the env file upstream's installer creates.
3. `~/.config/macdock/settings.json` — `wallpaperPath` pinned to the shipped
   wallpaper so Liquid Glass has an image from the first start.
4. `~/.config/hypr/hyprpaper.conf` + `~/Pictures/Wallpapers/Tahoe-{Dark,Light}.png`.
5. `packages.x86_64` — quickshell/hyprland/hyprlock/hyprpaper are the *extra*
   repository packages the upstream README asks for; `awww` (AUR) is left out
   on purpose — everything Apple-UI-related works against hyprpaper + the
   shipped wallpaper.

## Known-good upstream facts this integration relies on

Taken from the upstream README (2026-10-17 state): Hyprland ≥ 0.55 runs the
Lua config; Quickshell ≥ 0.3.0 needed, 0.3.2 recommended (Arch [extra] ships
0.3.2); the `hyprctl dispatch 'hl.dsp.no_op()'` smoke test; blur must be
enabled in the compositor's decoration config (it is, above).
