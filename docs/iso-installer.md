# The installer: Calamares, like CachyOS — and the one thing blocking it

> Status: **scaffold only. There is no installer in the ISO yet, and the
> install path has never run.** The branding, the module sequence, the
> launcher and the checks are in the repository; the program they configure is
> not, because it is not packaged in Arch.
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

1. **Build it ourselves in CI** from the AUR (`kpmcore`, then `calamares`,
   both CMake/Qt6). This is what a distribution does, and it is the
   architecture the rest of the project follows: first-party packaging of a
   third-party program, MIT/GPL kept at a process boundary. It costs CI time
   and it will take a couple of iterations to get the dependency order right.
2. **Take it from a binary repository that already packages it** (CachyOS's,
   for instance). Faster, verifiable today, but it makes our ISO depend on
   another distribution's rolling repository — a decision that should be made
   deliberately, not as a shortcut.
3. Go without, and ship `archinstall` — honest, in-repo, and not what was
   asked for.

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
- [ ] **The package.** Nothing in the repository produces `calamares` yet; see
      the decision above.
- [ ] **A whole install, in a VM.** Boot the ISO with an empty disk, run the
      installer, then boot what it produced. Until that has been done and
      photographed, the honest statement is "the installer does not exist
      yet", and **nobody should install to hardware**.
- [ ] Which bootloader, and for which firmware: `bootloader` handling both
      EFI and BIOS from an image whose own UEFI path has never been booted is
      optimistic.
- [ ] The installed system's own first boot: the live image needs
      `systemd.firstboot=no`, and the installed system will need the opposite
      — a real `machine-id`, a locale, a user password. `unpackfs` copies the
      live configuration, so this must be undone explicitly.
