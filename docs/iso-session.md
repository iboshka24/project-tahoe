# docs/iso-session.md — the live desktop session

> Status: **booted in QEMU on 2026-10-19, with three defects found and fixed.**
> `tests/check-iso.sh` (67 checks) proves every file the profile references
> exists, is syntactically sane and wired up; booting it is what found that the
> kernel command line named a parameter no hook reads, that the first-boot
> prompt held the desktop, and that the session config aborted at line 100.
> All the evidence is in [`iso-boot.md`](iso-boot.md).
>
> Date: 2026-10-18, revised 2026-10-19.

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
party `[tahoe]` repository is also baked in (`tahoe-base`, `tahoe-uel`,
`tahoe-compositor`): all three build from the `v0.1.0` tag on CI and land in
the ISO (0.1.0-1, verified in run 39162703139, 2026-10-18). Nothing in the
session wiring depends on them yet — `tahoe-uel`/`tahoe-compositor` binaries
are present but unused until the UEL desktop integration task in `TODO.md` §5.

## 2. How a boot goes

```
syslinux / systemd-boot
└─ vmlinuz-linux + initramfs  (archiso: tmpfs root, squashfs airootfs)
   └─ getty@tty1 (autologin.conf → auto-login as `tahoe`)
      └─ login shell reads /home/tahoe/.bash_profile
         └─ exec Hyprland           (only on tty1, only once, see TAHOE_SESSION)
            [needs systemd.firstboot=no, or the boot stops at the
             systemd-firstboot prompt before tty1 reaches bash_profile]
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
| The `hl.*` calls used are real API of Hyprland 0.55 | **partly wrong, found by booting:** `hl.dsp.reload_config` is nil (`hyprland.lua:100`), and because a Lua error aborts the file, everything after it never ran. Lines above 100 demonstrably work: `hl.env`, `hl.config`, `hl.bind`, `hl.exec_cmd` and the autostart all took effect |
| MacShell's requirements are in `[extra]` | quickshell 0.3.2-1 listed at archlinux.org (checked 2026-10-18) |
| Shell scripts parse | `sh -n` / `bash -n` inside `tests/check-iso.sh` |
| The vendored tree matches its signed provenance | `check-iso.sh` compares `macshell-vendor.md` to the pinned ref inside `vendor-macshell.sh` |

## 5. What has NOT been verified (be honest, in order)

1. The session has never been booted — not on real hardware, not in a VM.
   CI does produce the ISO (first green run 2026-10-18, run 39159947256:
   `tahoe-2026.10.18-x86_64.iso`, 1.7 GiB, SYSLINUX + systemd-boot both
   prepared), but Hyprland → MacShell on real Wayland, Liquid Glass
   rendering, Spotlight, widgets, MacLock password check (PAM on the live
   image), window previews are all still claims, not observations.
2. The artifact has not been downloaded and checksummed by a human, and its
   1-day retention means it must be re-fetched from the newest green run
   whenever it is needed.
3. hi-scaling on laptops (Hyprland monitor config is default only).
4. Apple Silicon/T2, VM modes, BIOS-era display hardware — SYSLINUX is
   packed for BIOS boot, but no BIOS machine has seen the image.
5. The `[tahoe]` packages are still unbuilt (missing `v0.1.0` tags on
   GitHub); the fail-soft skip path around them IS verified — CI ended
   green with all three warned.

## 6. How to build

> **Windows checkout gotcha:** the two `multi-user.target.wants/*.service`
> entries are git **symlinks** (mode 120000). They materialize on POSIX
> checkouts; on a Windows working tree `git status` shows them as deleted
> while the blobs are safely in the commits — never run `git add -A` from
> that clone again, or the entries leave the tree. (Pushing the committed
> tree is always safe.)
>
> **Windows local verification:** Git for Windows ships bash — run the
> structural checks locally with
> `& 'C:\Program Files\Git\bin\bash.exe' ./tests/check-iso.sh` (PowerShell).
> Two tool-layer traps these notes exist for:
>
> 1. `tests/check-iso.sh`'s CRLF guard counts CR bytes through `od | awk`,
>    not `grep`: in this MSYS environment both `grep` reading files and
>    `git grep`/`git cat-file` publishing through pipes rewrite line
>    endings on the fly, so their verdicts about CR bytes are untrustworthy.
>    `od` and PowerShell's `[IO.File]::ReadAllBytes` always report truth.
> 2. A Windows clone that predates the `eol=lf` attributes can hold CRLF
>    on disk while `git status` still reports clean (both sides normalize
>    for diff). The committed blobs are already LF; only the working copy
>    needs converting. Do it byte-wise with PowerShell
>    (`[IO.File]::ReadAllBytes` → drop CR that precedes LF →
>    `WriteAllBytes`) — and never with `git reset --hard`: on this clone
>    that would materialize the two `.wants` symlink entries as files and
>    re-create the accident from gotcha 1.

Locally (clean Arch chroot or container):

```sh
sudo ./iso/build.sh            # TAHOE_OUT_DIR=/somewhere/writable
$OUT_DIR/…/tahoe-… .iso
```

Or let CI do it: `.github/workflows/build-iso.yml` first runs the structural
checks on the runner, then starts its own **privileged** `archlinux:base-devel`
container (a `container:` job cannot mount `/proc` inside mkarchiso's
install chroot) and uploads the ISO as the `tahoe-live` one-day artifact.
First green run: 2026-10-18, run 39159947256.

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
