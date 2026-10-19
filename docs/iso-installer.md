# The installer: Calamares, like CachyOS — and the one thing blocking it

> Status: **the installer program is in the ISO; the install itself has never
> run.** Calamares 3.4.2 builds from the AUR in CI and lands in the image
> (verified in run 39265708121: `built calamares at …`, then
> `installing calamares…`, and `calamares-3.4.2-2` in the package list
> alongside `kpmcore-26.08.2`, `polkit-qt6` and `qt6-declarative`). What is
> still missing is the 20 module configs and any end-to-end install.
>
> Date: 2026-10-19.

---

## 1. What "like CachyOS" means concretely

CachyOS does not have its own installer program. It ships **Calamares** — the
same installer EndeavourOS, Garuda and most Arch derivatives use — plus a
branding directory of its own, and a source package that rebuilds it. The
"look" is configuration: `branding.desc`, a Qt stylesheet, a slideshow and a
product logo.

So this is what Tahoe has now:

| Path | What it is |
|---|---|
| `etc/calamares/settings.conf` | the module sequence and the branding selection |
| `etc/calamares/branding/tahoe/branding.desc` | product name, version, sidebar colours |
| `etc/calamares/branding/tahoe/stylesheet.qss` | Qt widget styling in the design system's palette |
| `etc/calamares/branding/tahoe/show.qml` | the slideshow shown while unpacking |
| `etc/calamares/branding/tahoe/logo.svg` | the mark |
| `etc/calamares/modules/unpackfs.conf` | installs by unpacking the live squashfs |
| `usr/share/applications/tahoe-install.desktop` | "Install Project Tahoe" in the app menu |
| `~/.config/hypr/hyprland.lua` | `SUPER + I` launches it |

The colours are not new: `#0A84FF`, `#1C1C22` and `#F5F5F7` are the tokens in
`crates/tahoe-shell/src/design.rs`, the same ones the compositor renders, the
COSMIC theme is generated from and the SwiftUI emitter writes out. An
installer that looks like a different product than the desktop it installs is
a branding bug, and this avoids it by construction rather than by eye.

Two decisions worth naming:

**`unpackfs`, not `pacstrap`.** The live rootfs already contains exactly the
system we ship, including the `[tahoe]` packages, and it is the same image
that was boot-tested. Unpacking it cannot drift from "live" the way a package
list can.

**The slideshow states facts, not marketing.** It ends with the sentence that
Windows and macOS programs run through Wine, Proton or Darling in separate
processes and that Tahoe never modifies them — the same claim the rest of the
project has to keep.

## 2. The blocker: Calamares is not in Arch

Checked, not assumed:

```
$ curl -s 'https://archlinux.org/packages/search/json/?q=calamares' | jq '.results | length'
0
$ grep -c calamares <the extra/ mirror listing>
0
```

`archinstall` **is** in `extra` (4.5-1), but it is a text menu, not the
graphical installer that was asked for. Calamares lives in the AUR, which
means a binary cannot simply be installed — one of these has to happen:

1. ~~Build it ourselves in CI from the AUR~~ — **chosen, and working.**
   `iso/build.sh` clones the AUR package as the build user, installs its
   dependencies as root and runs makepkg into the same local repository the
   first-party packages land in. One package, not two: the AUR `kpmcore`
   repository is empty, and kpmcore turns out to be in `extra` at 26.08.2, so
   `AUR_PKGS=(calamares)` and every dependency comes from a signed repository.
   Verified present in the ISO by the build log and the package list.
2. ~~Take it from a binary repository~~ — rejected by the decision above; it
   would make our ISO depend on another distribution's rolling repository.
3. ~~Go without, ship `archinstall`~~ — it is in `extra`, but it is a text
   menu, not the graphical installer that was asked for.

**What it costs in CI time.** The ISO job grew from about 13 minutes to
16-19 minutes when the installer was added, and one run took 50 — building
Calamares from source is the bulk of it, and AUR HEAD moving between runs is
the likeliest reason the same job is not always the same length. That is the
price of packaging it ourselves instead of pulling a binary; it is worth
knowing before wondering why the ISO job is slow.

`iso/build.sh` already handles the "no installer" case: first-party packages
that fail to build are dropped from a sanitized profile copy, and `calamares`
now goes through the same path, so **the ISO keeps building with or without
it**. `packages.x86_64` lists it so that the moment a build produces it, the
ISO picks it up.

## 3. Not done, in order of importance

- [ ] **The 20 module configurations.** `settings.conf` names the modules, but
      only `unpackfs` has a config file, so Calamares would fall back to
      upstream defaults for things like `users` and `bootloader` — defaults
      written for other distributions. `tests/check-iso.sh` reports the list
      and skips (82 passed, 1 skipped).
- [ ] **Pin the AUR revision.** The build records the commit it built, but
      the clone takes AUR HEAD, so two builds of the same commit of *this*
      repository can produce different installer versions. MacShell is pinned
      by commit (`iso/macshell-vendor.md`); calamares should be too.
- [ ] **A whole install, in a VM.** Boot the ISO with an empty disk, run the
      installer, then boot what it produced. Until that has been done and
      photographed, the honest statement is "the installer runs but has never
      installed anything", and **nobody should install to hardware**.
- [ ] Which bootloader, and for which firmware: `bootloader` handling both
      EFI and BIOS from an image whose own UEFI path has never been booted is
      optimistic.
- [ ] The installed system's own first boot: the live image needs
      `systemd.firstboot=no`, and the installed system will need the opposite
      — a real `machine-id`, a locale, a user password. `unpackfs` copies the
      live configuration, so this must be undone explicitly.
