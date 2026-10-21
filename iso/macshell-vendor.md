# Vendored MacShell — provenance

The live desktop of the Tahoe profile is [MacShell](https://github.com/0-ss/MacShell)
(with a matching [MacLock](https://github.com/0-ss/MacShell#maclock) lock screen),
a macOS-Tahoe-style menu bar + Dock + Spotlight + widgets for Hyprland, built
on [Quickshell](https://quickshell.org). It is MIT-licensed, so it can be
vendored here as-is with attribution.

| | |
|---|---|
| Upstream | `0-ss/MacShell`, branch `main`, tag `0.2.8` |
| Vendored commit | `eebaaad667bc847287dc8baeda93dbec97401fa4` |
| Vendored on | 2026-10-21 |
| Vendored into | `iso/airootfs/etc/skel/.config/quickshell/{MacShell,MacLock}/` |
| Installed version marker | `~/.config/macshell/vendor` (written from the same pin) |
| License | MIT — `LICENSES/MIT.txt`, copyright Rickky; see REUSE.toml |
| Update tool | `./iso/vendor-macshell.sh [ref]` |

This pin replaces the 2026-10-18 `eb3d89f` snapshot. The visible changes are
MacShell 0.2.8's desktop-widget settings, Settings / About windows, the
Liquid Glass refinements and the Russian translation; none of the upstream
MacShell or MacLock source is edited by Tahoe.

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
4. `~/.config/macwidgets/settings.json` — the first-run widget layout:
   large Clock + Calendar, medium Weather + Player. MacWidgets persists
   changes here. It is part of `/etc/skel`, so Calamares installs the same
   layout for the new user.
5. `~/Pictures/Wallpapers/Tahoe-{Dark,Light}.png` — the shipped images. The
   dark default is a CC0 Yosemite Valley photograph (see `NOTICE`); the
   light image is the original Tahoe gradient. The session starts `swaybg`
   on the pixman renderer from `hyprland.lua` (see `docs/iso-session.md` §2a
   for why not hyprpaper).
6. `packages.x86_64` — quickshell/hyprland/hyprlock/swaybg/qt6-multimedia
   are the *extra* repository packages the session needs. `jq` is used by
   MacLock to find the wallpaper, `curl` by the Weather widget, and
   `playerctl` is the hardware-key fallback. `awww` (AUR) is left out on
   purpose — the default wallpaper is a static picture, and MacShell's
   picker falls back to the running swaybg.

## Known-good upstream facts this integration relies on

Taken from the upstream README at tag `0.2.8`: Hyprland ≥ 0.55 runs the Lua
config; Quickshell ≥ 0.3.0 needed, 0.3.2 recommended (Arch [extra] ships
0.3.2); the `hyprctl dispatch 'hl.dsp.no_op()'` smoke test; blur must be
enabled in the compositor's decoration config (it is, above). The upstream
installer now puts its keybinds in `~/.config/hypr/macshell.lua`; Tahoe keeps
the equivalent block inline in `hyprland.lua` because the session already has
a first-party Lua config and only needs the shell block.
