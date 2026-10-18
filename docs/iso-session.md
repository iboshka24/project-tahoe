# docs/iso-session.md — the live desktop session

> Status: **written, verified structurally, not verified by booting.**
> `tests/check-iso.sh` proves every file the profile references exists, is
> syntactically sane and wired up; nothing here has yet been confirmed by
> actually booting the ISO on hardware or a VM. The first `mkarchiso` run is
> the next milestone (see "How to build" and TODO.md §5).
>
> Date: 2026-10-18.

---

## 1. What the live session is

An archiso profile (`iso/`) whose live user lands in a fully dressed desktop
instead of a bare console: **Hyprland 0.55+ running its Lua config**, with
**MacShell** (macOS-Tahoe-style menu bar, Dock, Spotlight, widgets) + **MacLock**
on top of Quickshell. This is the session-stand-in for the Tahoe compositor,
which still has to become a Wayland client (see ROADMAP.md) — MacShell gives
the distribution a working macOS-looking desktop *today*, from packages.

| [MacShell](https://github.com/0-ss/MacShell) component | Why it was chosen |
|---|---|
| Menu bar + Control Center + Spotlight + widgets | the macOS look Tahoe wants, already fully implemented upstream |
| Liquid Glass refraction shaders | needs Hyprland layer rules — checked into our config |
| MacLock separately processes | a shell crash cannot take the lock with it |
| MIT license | may be vendored, attributed, unchanged (REUSE: `MIT`) |

Everything MacShell needs is in Arch's own `[extra]` repository as of
2026-10: `quickshell 0.3.2-1`, `hyprland`, `hyprlock`, `hyprpaper`. The first
party `[tahoe]` repository is intentionally still there (`tahoe-base`,
`tahoe-uel`, `tahoe-compositor`), but nothing in the session depends on it.

## 2. How a boot goes

```
syslinux / systemd-boot
└─ vmlinuz-linux + initramfs  (archiso: tmpfs root, squashfs airootfs)
   └─ getty@tty1 (autologin.conf → auto-login as `tahoe`)
      └─ login shell reads /home/tahoe/.bash_profile
         └─ exec Hyprland           (only on tty1, only once, see TAHOE_SESSION)
            └─ hyprland.lua
               ├─ hl.exec_cmd("hyprpaper")            → ships wallpaper
               ├─ hl.exec_cmd("sh ~/.local/bin/macshell-start")
               │    waits for the Wayland socket, env, icon theme, qs -c MacShell,
               │    restarts on crash
               └─ layer rules (macdock/macspot/macbarbg blur) + binds
```

Maintenance: Ctrl+Alt+F2..F6 gives an ordinary console; `root` has no
password on the live image and `tahoe` can `sudo` without one.

## 3. What is baked in, file by file

| Path (under `iso/airootfs/`) | Purpose |
|---|---|
| `etc/passwd`, `etc/shadow` | `root` + `tahoe` (uid 1000). mkarchiso copies `/etc/skel` into `/home/tahoe` from this file at build time |
| `etc/systemd/system/getty@tty1.service.d/autologin.conf` | auto-login (releng pattern) |
| `etc/systemd/system/multi-user.target.wants/{NetworkManager,bluetooth}.service` | git symlinks enabling the two services MacShell's trays depend on |
| `etc/sudoers.d/10-tahoe-live` | passwordless sudo, live image only |
| `etc/locale.conf`, `etc/motd` | `C.UTF-8`, the greeting incl. keybinds |
| `etc/skel/.config/hypr/hyprland.lua` | the session config (see iso/macshell-vendor.md for what is upstream's block) |
| `etc/skel/.config/hypr/hyprpaper.conf` | default wallpaper daemon config |
| `etc/skel/.config/quickshell/MacShell/` + `MacLock/` | vendored upstream (MIT, pinned commit) |
| `etc/skel/.local/bin/macshell-start` | upstream's start script, verbatim |
| `etc/skel/.config/macshell/env` | env overrides (upstream's file format) |
| `etc/skel/.config/macdock/settings.json` | `wallpaperPath` → shipped wallpaper |
| `etc/skel/Pictures/Wallpapers/Tahoe-{Dark,Light}.png` | first boot wallpaper pair, generated gradients |

## 4. What has been verified (and how)

| Claim | Evidence |
|---|---|
| Every referenced file exists, is sane, has the right git mode | `tests/check-iso.sh` (also wired into `tests/check-all.sh`) |
| The `hl.*` calls used are real API of Hyprland 0.55 | checked against Hyprland 0.55 docs (dispatchers, layer rules, `hl.config`, `hl.env`/`hl.bind`) and against `MacShell` upstream `install.sh`, which writes the same forms |
| MacShell's requirements are in `[extra]` | quickshell 0.3.2-1 listed at archlinux.org (checked 2026-10-18) |
| Shell scripts parse | `sh -n` / `bash -n` inside `tests/check-iso.sh` |
| The vendored tree matches its signed provenance | `check-iso.sh` compares `macshell-vendor.md` to the pinned ref inside `vendor-macshell.sh` |

## 5. What has NOT been verified (be honest, in order)

1. `mkarchiso` has never run — `iso/build.sh` + CI exist, an ISO does not.
2. The session has never been booted: Hyprland → MacShell on real Wayland,
   Liquid Glass rendering, Spotlight, widgets, MacLock password check (PAM on
   the live image), window previews.
3. hi-scaling on laptops (Hyprland monitor config is default only).
4. Apple Silicon/T2, VM modes, BIOS-era display hardware.
5. The `[tahoe]` packages are still unbuilt (missing `v0.1.0` tags on GitHub);
   the build script tolerates their absence and warns.

## 6. How to build

> **Windows checkout gotcha:** the two `multi-user.target.wants/*.service`
> entries are git **symlinks** (mode 120000). They materialize on POSIX
> checkouts; on a Windows working tree `git status` shows them as deleted
> while the blobs are safely in the commits — never run `git add -A` from
> that clone again, or the entries leave the tree. (Pushing the committed
> tree is always safe.)

Locally (clean Arch chroot or container):

```sh
sudo ./iso/build.sh            # TAHOE_OUT_DIR=/somewhere/writable
$OUT_DIR/…/tahoe-… .iso
```

Or let CI do it: `.github/workflows/build-iso.yml` builds on the
`archlinux:base-devel` container and uploads the ISO as a one-day artifact.

Updating the vendored shell:

```sh
./iso/vendor-macshell.sh            # re-sync pinned commit
./iso/vendor-macshell.sh main       # move to upstream's newest commit
# then refresh the provenance block in iso/macshell-vendor.md
```

## 7. Conflicts bookkeeping

MacShell REPLACES the bar/dock/notification daemon. The profile must never
gain: waybar, hyprpanel, ags/quickshell configs like Noctalia, nwg-dock,
dunst, mako, swaync. `tests/check-iso.sh` fails if any of those names appear
in `packages.x86_64` — adding one becomes a conscious decision written down
here.
