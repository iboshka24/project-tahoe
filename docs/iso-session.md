# docs/iso-session.md — the live desktop session

> Status: **booted in QEMU; the 2026-10-21 image switches the wallpaper
> daemon to swaybg after hyprpaper 0.8.4 crashed on a GPU-less VM. The
> desktop now also ships MacShell 0.2.8's widgets, a first-run layout and a
> Yosemite photograph as the default wallpaper.**
> `tests/check-iso.sh` proves every file the profile references exists, is
> syntactically sane and wired up; booting it is what found that the kernel
> command line named a parameter no hook reads, that the first-boot prompt
> held the desktop, and that the session config aborted at line 100. All the
> evidence is in [`iso-boot.md`](iso-boot.md).
>
> Date: 2026-10-18, revised 2026-10-21.

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
2026-10: `quickshell 0.3.2-1`, `hyprland`, `hyprlock`, `swaybg`, plus
`qt6-multimedia` for the QML import and `jq`/`curl`/`playerctl` for MacLock,
the Weather widget and the hardware-key fallback. (The original choice for
the wallpaper was `hyprpaper`; §2a explains why it is gone.) The first party `[tahoe]` repository is also baked in (`tahoe-base`,
`tahoe-uel`, `tahoe-compositor`): all three build from the `v0.1.0` tag on CI
and land in the ISO (0.1.0-1, verified in run 39162703139, 2026-10-18).
Nothing in the session wiring depends on them yet — `tahoe-uel`/`tahoe-compositor`
binaries are present but unused until the UEL desktop integration task in
`TODO.md` §5.

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
               ├─ env WLR_RENDERER=pixman swaybg -i …/Tahoe-Dark.png -m fill
               │    → paints the wallpaper, no GPU needed (§2a)
               ├─ hl.exec_cmd("sh ~/.local/bin/macshell-start")
               │    waits for the Wayland socket, env, icon theme, qs -c MacShell,
               │    restarts on crash
               └─ layer rules (macdock/macspot/macbarbg blur) + binds
```

### 2a. Why swaybg and not hyprpaper (2026-10-21)

The 2026-10-20 image started `hyprpaper`, as every build before it had. Its
desktop came up **plain black** with no error dialog. Inside the live
session, `pgrep -a hyprpaper` printed nothing, and running it by hand gave:

```
DEBUG ]: Monitor Virtual-1 has no target: no wp will be created
ERR from hyprtoolkit ]: [EGL] Command eglInitialize errored out with
    EGL_NOT_INITIALIZED (0x12289): DR12: failed to create screen
KMS: DRM_IOCTL_MODE_CREATE_DUMB failed: Permission denied
ERR from aquamarine ]: Swapchain: Failed to acquire a buffer
[1]+ Segmentation fault (core dumped) hyprpaper
```

Two separate faults were in that output. The first is a config format
change in hyprpaper 0.8.x: `wallpaper=,path` (empty monitor means "all") is
no longer parsed; the current syntax is a block with `monitor=` and
`path=`. With that block, the "no target" line went away — and the crash
remained. The second fault is the renderer: hyprpaper 0.8 renders through
hyprtoolkit/Aquamarine, which wants a working GBM/EGL device. A QEMU
`virtio-vga` without virgl has none, so EGL fails, GBM falls back to KMS
dumb buffers, and those need DRM master — hence `Permission denied` even
after `chmod 666 /dev/dri/*`. `AQ_RENDERER=pixman` was not honoured.

The permission theory was tested and rejected: the live user `tahoe` is in
no supplementary groups (`groups=1000`), `/dev/dri/renderD128` is
`root:render 660`, but making both nodes world-writable did not change the
error. Hyprland itself works because systemd-logind hands it a DRM fd;
hyprpaper opens the device itself.

`swaybg 1.2.2` with `WLR_RENDERER=pixman` uses the wlroots software
renderer, needs no DRM access and paints a shm buffer. Run in the same live
session it printed `Found config * for output Virtual-1 (Red Hat Inc. QEMU
Monitor)` and the wallpaper appeared. It also fixes a 16:10 artefact found
on the installed system: hyprpaper letterboxed the 16:9 wallpaper into a
1280x800 output, leaving the bottom 80 px black with the tail of the tty1
console showing through; `-m fill` covers the whole output.

The package list and the autostart now use swaybg; `hyprpaper.conf` is
removed. MacShell's glass does not depend on it: the path comes from
`~/.config/macdock/settings.json` (`wallpaperPath`), with the picker in
`MacBar.qml` restarting a running swaybg. MacShell 0.2.8's picker does not
set `WLR_RENDERER` itself, so Tahoe ships `/usr/local/bin/swaybg`, a small
first-party wrapper that adds `WLR_RENDERER=pixman` when the variable is
unset and then execs `/usr/bin/swaybg`. A newly picked image therefore also
uses the software renderer in this VM.

### 2b. First-run desktop and the installed copy of its config

The live user's home is populated by mkarchiso from `/etc/skel`, so the
2026-10-21 image starts with:

- MacShell 0.2.8 (upstream commit `eebaaad`), vendored unmodified;
- a `~/.config/macwidgets/settings.json` layout that places the large Clock
  and Calendar and the medium Weather and Player cards on the left of the
  desktop (the same cards shown in the upstream screenshots);
- the CC0 Yosemite photograph as `Tahoe-Dark.png`, painted full-screen by
  `swaybg -m fill`;
- `~/.config/macshell/vendor` recording the pin.

Calamares' `shellprocess` runs before `users`: it deletes the live `tahoe`
account and its home, then the `users` module creates the install-time user
and copies `/etc/skel` into the new home. The installed desktop is therefore
not a stale copy of the live home; it is the same current `/etc/skel` tree,
including the widget layout and the version marker. This was checked on the
clean install after the rebuild (see [`iso-installer.md`](iso-installer.md)).

The Weather card needs network access (Open-Meteo). Without it the card still
occupies its slot and reports the lookup failure; the Clock, Calendar, Player,
wallpaper and Dock do not depend on the network.

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
| `etc/skel/.config/quickshell/MacShell/` + `MacLock/` | vendored upstream (MIT, pinned commit) |
| `etc/skel/.local/bin/macshell-start` | upstream's start script, verbatim |
| `etc/skel/.config/macshell/env` | env overrides (upstream's file format) |
| `etc/skel/.config/macshell/vendor` | installed MacShell version: `0.2.8`, commit `eebaaad`; proves the user home was populated from the current `/etc/skel` |
| `etc/skel/.config/macdock/settings.json` | `wallpaperPath` → shipped wallpaper |
| `etc/skel/.config/macwidgets/settings.json` | first-run widget layout: large Clock + Calendar, medium Weather + Player |
| `etc/skel/Pictures/Wallpapers/Tahoe-Dark.png` | default wallpaper: CC0 Yosemite Valley photograph by EF5, cropped to 1920×1200 |
| `etc/skel/Pictures/Wallpapers/Tahoe-Light.png` | light-appearance wallpaper: first-party Tahoe gradient |

The dark wallpaper is EF5's CC0 photograph
[“Tunnel View” overlook, file 03](https://commons.wikimedia.org/wiki/File:%E2%80%9CTunnel_View%E2%80%9D_overlook_showing_famous_rock_formations_in_Yosemite_Valley_03.jpg),
cropped to 1920×1200 and stripped of metadata; see `NOTICE` and `REUSE.toml`.

## 4. What has been verified (and how)

| Claim | Evidence |
|---|---|
| Every referenced file exists, is sane, has the right git mode | `tests/check-iso.sh` (also wired into `tests/check-all.sh`) |
| The `hl.*` calls used are real API of Hyprland 0.55 | **partly wrong, found by booting:** `hl.dsp.reload_config` is nil (`hyprland.lua:100`), and because a Lua error aborts the file, everything after it never ran. Lines above 100 demonstrably work: `hl.env`, `hl.config`, `hl.bind`, `hl.exec_cmd` and the autostart all took effect |
| MacShell's requirements are in `[extra]` | quickshell 0.3.2-1 listed at archlinux.org (checked 2026-10-18) |
| Shell scripts parse | `sh -n` / `bash -n` inside `tests/check-iso.sh` |
| The vendored tree matches its signed provenance | `check-iso.sh` compares `macshell-vendor.md` to the pinned ref inside `vendor-macshell.sh` |

## 5. What has NOT been verified (be honest, in order)

> Written 2026-10-18. The image has since been booted many times, installed
> and booted from disk (docs/iso-installer.md §§8-9), so items 1, 2 and 5
> below are historical. What is still open lives in
> [`limitations.md`](limitations.md).

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
