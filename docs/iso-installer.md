# The installer: Calamares, like CachyOS — and the one thing blocking it

> Status: **an install now completes and the installed system boots to the
> desktop from its own disk** (§8). The run that proved it needed two manual
> repairs, because the ISO it booted predated the package fixes; both repairs
> are in the repository and a rebuild carries them. Still unproven: the same
> run without hands, and UEFI. **Do not install to hardware yet.**
>
> Date: 2026-10-20.

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

---

## 7. What the guest actually said: five defects, in the order they appeared

Everything below was found by booting the built ISO in QEMU, opening a
terminal in the live session (`SUPER+Return`) and running the installer by
hand, then reading the message it printed. The screenshot of the working
welcome page is `Pictures/tahoe/installer-welcome.png`.

| # | Symptom in the guest | Cause | Fix |
|---|---|---|---|
| 1 | `bash: /usr/local/bin/tahoe-installer: Permission denied`<br>`-rw-r--r-- 1 root root` | `mkarchiso` does not carry the checkout's 0755 into the image; it applies `profiledef.sh`'s `file_permissions`. `macshell-start` escapes this only because the session runs it as `sh …`. | the wrapper is declared `0:0:755` in `file_permissions` |
| 2 | `calamares: Unknown option 'branding'.` | Calamares 3.4.2 has no `--branding`; its command line is `-d, -D, -T, -c/--config, -X` (`src/calamares/main.cpp`). Branding is read from `settings.conf`. | flag removed; `settings.conf` already says `branding: tahoe` |
| 3 | `qt.qpa.xcb: could not connect to display :0`<br>`Could not load the Qt platform plugin "xcb"` | `DISPLAY` is set, there is no X server, and Qt prefers xcb; it aborts before creating a window. | the wrapper exports `QT_QPA_PLATFORM=wayland` when `WAYLAND_DISPLAY` is set |
| 4 | `Calamares has invalid settings, shutting down.` | an `instances:` entry with `id:` but no `module:` — `InstanceDescription::fromSettings` reads both, an empty module name makes the descriptor invalid, and `Settings::isValid()` then fails. | the block is gone; a module named in the sequence already reads `modules/<module>.conf` |
| 5 | `Module "mkinitcpio" not found in module search paths.`<br>`Calamares Initialization Failed` | there is no `mkinitcpio` module in 3.4.2. Arch uses `initcpiocfg` (edit the config) and `initcpio` (run `mkinitcpio`). | `mkinitcpio` → `initcpio` |

Defects 1 and 2 were fixed in the image before the welcome page appeared; 3-5
were applied to `/etc/calamares/settings.conf` inside the running live
session, which is why the screenshot shows the page while the ISO that booted
it still contained the broken sequence. All five are fixed in the repository,
and checks 86 → 90 cover them: the wrapper is executable in the image, it
passes no option Calamares lacks, it selects the Wayland backend, and every
module named in the sequence exists in calamares 3.4.2.

Two warnings remain in the terminal and are not yet dealt with:
`os-prober cannot start` (the package is not in the live image; the
`bootloader`/`grubcfg` modules use it to find other systems) and an empty
black panel in the sidebar, which is an image slot in `branding.desc` that no
file fills yet.

### What is still missing before anyone installs

- A run of the **rebuilt** ISO that reaches the finished page with no manual
  step. Everything such a run needs is in the repository; what is missing is
  the observation, not the configuration.
- UEFI. Every install so far is BIOS/MBR, because the host has no OVMF.
- `os-prober`, or an explicit decision to disable other-OS detection (the
  current decision is that it stays out; `grub-mkconfig` warns once).

## 8. The first whole install: what broke, and the proof it boots

The ISO was booted in QEMU (BIOS, 4 GiB, a 24 GiB qcow2), Calamares was
started from a terminal, and the wizard was driven **with the keyboard only**:
`Alt+N` is the `&Next` mnemonic, `Alt+I` is `&Install`, and the users form was
filled with `Tab` and text. There is no mouse automation here, which is why the
buttons' mnemonics matter.

The install reached `initcpio` and stopped there:

```
==> Building image from preset: /etc/mkinitcpio.d/linux.preset: 'default'
  -> -k /boot/vmlinuz-linux -g /boot/initramfs-linux.img
==> ERROR: Invalid option -k -- '/boot/vmlinuz-linux' must be readable
```

`ls -l /boot` in the live session is empty: archiso strips the kernel and
initramfs from the live root because the live system boots them from the
medium. `unpackfs` copies that empty `/boot`, so the target had no kernel. The
package's real file is at `/usr/lib/modules/7.2.11-arch1-1/vmlinuz`; the
cleanup job now copies it to `/boot/vmlinuz-linux` before `initcpio` runs.

The second run got past `initcpio` and failed at the bootloader:

```
The bootloader could not be installed. The installation command
grub-install --target=i386-pc --recheck --force /dev/vda
returned error code 127.
```

127 is "not found": the ISO's squashfs was built before `grub` was added to
`packages.x86_64` (commit `87f2442`), and `unpackfs` copies the squashfs, not
the live session's overlay. Installing `grub` in the running live system could
not help — the target is not a copy of the live session.

To get past it **for the purpose of testing the boot chain**, `grub`'s files
were copied from the live session into the mounted target and `grub-install` +
`grub-mkconfig` were run in a chroot. That is the manual repair; the rebuilt
ISO has the package and the module does this itself.

What the installed system then did, and what was checked inside it:

| Check | Result |
|---|---|
| GRUB from the MBR | boots, menu entry `Arch Linux` (see the `os-release` defect below) |
| kernel / initramfs | `7.2.11-arch1-1` booted; `/boot/initramfs-linux.img` was built by `initcpio` |
| root filesystem | `fstab` mounts `UUID=e9493d86-… / ext4 defaults 0 1` |
| login | `tahoe-vm login:` — no autologin, so the live getty drop-in is gone |
| user | `uid=1000(tahoe)`, groups include `wheel`, `sudo` asks for the password |
| root | `/etc/shadow` is `0600`; the cleanup ran `passwd -l root` |
| live sudoers | absent; `/etc/sudoers.d/10-installer` is `%wheel ALL=(ALL) ALL`, not NOPASSWD |
| archiso initramfs hooks | `/etc/mkinitcpio.conf.d/` is empty |
| desktop | logging in on tty1 starts Hyprland + MacShell (`Pictures/tahoe/installed-desktop.png`) |

Two more defects were found on that installed system:

| # | Symptom | Cause | Fix |
|---|---|---|---|
| 6 | `systemd-binfmt.service` failed: `Failed to add binary format 'DOSWin': No such file or directory` | the rule named `wine` as the interpreter; the image does not install wine, and `tahoe-dispatch` was built but never packaged | rule names `/usr/lib/tahoe/tahoe-dispatch`, and `tahoe-uel` installs it |
| 7 | GRUB menu, login banner and fastfetch say **Arch Linux** | the image shipped no `os-release`, so `filesystem`'s was used | `iso/airootfs/etc/os-release`: `NAME="Project Tahoe"`, `ID=tahoe`, `ID_LIKE=arch` |

Running a Windows `.exe` in the installed system still needs Wine or Proton to
be installed: the dispatcher is registered, finds no Wine, and says so. The
image does not ship Wine; that is a packaging decision, not a defect in the
registration.

Screenshots: `Pictures/tahoe/installer-welcome.png` (wizard),
`Pictures/tahoe/installed-desktop.png` (the installed system's desktop).
