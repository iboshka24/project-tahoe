# TODO — what is left to finish

> Ordered by what unblocks the most. Each item says what exists already, so
> nobody re-derives it. "Verified" means a command in the box at the bottom
> of this file passed — see `docs/STATUS.md` for the honest state of every
> claim.

---

## 1. Put the shell on screen (the renderer exists; the Wayland client does not)

The visual part of the shell renders and is verified: `tahoe-preview --scene all`
writes five PNGs (desktop, launcher, control centre, dark and light) from the
same code that defines the layout and palette. What is missing is the *client*:

- [ ] A `smithay-client-toolkit` program that renders `tahoe_shell::render::Canvas`
      into a `wl_shm` buffer. `Canvas` already hands out its pixel bytes, so this
      is a `memcpy` plus buffer lifecycle — not a second renderer. Do not write one.
- [ ] Panel as a `zwlr_layer_shell_v1` client anchored to the top. The compositor
      already implements layer-shell and answers the configure with the right
      size; the reserved strip is tested in `tests/` (`wayland_client` e2e).
- [ ] The dock as a second layer surface, bottom-anchored, horizontally centred
      (the geometry already lives in `tahoe_shell::scenes::Decor`).
- [ ] Seat input: pointer hit-testing over panel/dock/launcher, scroll over the
      workspaces pills, keyboard for the launcher. No animations until this is real.
- [ ] **Definition of done:** run `tahoe-compositor`, launch the shell client,
      and capture a screenshot from the *live compositor* (not from the scene
      renderer). Compare against `~/Pictures/tahoe/*.png` by eye, not by patch:
      the scene renderer deliberately cannot fake a dead client, an uncommitted
      buffer or input that goes nowhere. That is exactly what this step buys.

## 2. Make the launcher list real applications

- [ ] Feed `tahoe_shell::LauncherIndex` (parses freedesktop `.desktop` files,
      18 tests) into the launcher scene instead of the fixed row list.
- [ ] Run the chosen entry through `tahoe-uel run`, showing the strategy
      (native / Wine / Proton) and `Confidence` next to the result — for a
      fail visibly rather than silently.
- [ ] Dock tiles from the same index; the running dot should reflect
      reality (read the compositor's surface list over the shell protocol).

## 3. Finish the "native" story (one gap remains)

- [x] PE → `tahoe-dispatch` (binfmt_misc) → Wine, real program, exit 0.
- [x] PE → Proton with a real prefix and NTSync, chosen from the PE import table.
- [ ] **Darling: build it and run one real Mach-O.** This is the only
      unexecuted layer. Everything up to `execve` is tested; the missing link
      is the launch itself. Nothing in Tahoe touches Darling's process, so this
      is unpacking and running it — not integrating it.
- [ ] DMG extraction: delegate to an external tool (7-Zip or `apfs-fuse`) and
      wire the parsed trailer/`blkx` (`tahoe-format::dmg`) to it. Tahoe itself
      must never mount a DMG — see `docs/native-execution.md`.

## 4. Window management the compositor already promises

- [ ] Resize (`resize_request` is accepted and ignored — windows cannot be
      dragged from the corner).
- [ ] Maximize / fullscreen / minimize states, at least for the shell's own
      decorations.
- [ ] Popup grabs (`xdg_popup.grab` is a no-op: context menus appear but cannot
      take input).
- [ ] Cursor shape (`wp_cursor_shape` or `wl_cursor`); without it a TTY session
      has no pointer imagery at all.

## 5. Distribution

- [x] `iso/` gained a complete live desktop session (2026-10-18,
      `docs/iso-session.md`): Hyprland 0.55+ Lua config, vendored MacShell +
      MacLock (MIT, pinned commit), macshell-start, autologin skel,
      wallpapers, swaybg wallpaper daemon, NetworkManager/bluetooth service
      links, conflict-free package list. Parse-checked by
      `tests/check-iso.sh` (also in `tests/check-all.sh`).
- [x] `mkarchiso` in a clean chroot — done via CI (2026-10-18, GitHub run
      39159947256): `iso/build.sh` fail-soft on the three tag-less
      `[tahoe]` packages, `tahoe-2026.10.18-x86_64.iso` (1.7 GiB) uploaded
      as the `tahoe-live` artifact (1-day retention). Boot not yet tried,
      so the fixture itself stays a task:
- [x] First-party `[tahoe]` packages build from the `v0.1.0` tag and are
      installed in the ISO (2026-10-18, GitHub run 39162703139:
      `tahoe-uel-0.1.0-1`, `tahoe-compositor-0.1.0-1`, `tahoe-base-0.1.0-1`
      in the 504-package list; `tahoe-base` needed `install -d "$pkgdir/etc"`
      before the `tahoe-release` redirection — commit 52bb15a). The strip
      "No debugging symbols" note on the release Rust binaries is harmless.
- [x] Downloaded and checksummed the artifact (2026-10-19): the ZIP's own
      CRC and the ISO's built-in `airootfs.sha512` both verify; the ISO is
      `0037ca5e…27d278`, 1 739 030 528 bytes.
- [x] Booted it in QEMU (`tests/vm-boot.sh`, no root, KVM). **It did not
      boot**: `img_label=` is a parameter of the loop-mount hook, not of the
      hook that finds the medium, so the initramfs never found its own CD and
      dropped to an emergency shell. Fixed; a second defect (the first-boot
      prompt holding the desktop) also fixed; a third (`hyprland.lua:100`
      aborting the config) fixed. Evidence: `docs/iso-boot.md`.
- [x] The desktop really does come up: Hyprland starts and MacShell renders
      its menu bar, clock, workspace pills and Dock.
- [x] CI rebuilt the image (run 39229471074) and the result was booted through
      the real bootloader: `tahoe-2026.10.19-x86_64.iso`,
      `sha256 791a3819…1749`, no emergency shell, desktop up, config-error
      banner gone. This is the first ISO that boots.
- [ ] UEFI boot: the El Torito catalog has an EFI section, but no OVMF was
      available to boot it, so that path is fixed by inspection only.
- [x] `systemd-binfmt.service` on the live image: the rule was packaged from
      the `v0.1.0` tag (18 commits behind) and pointed at `/usr/bin/wine`;
      build `142ad4d` snapshots HEAD, and on the 2026-10-20 ISO
      `systemctl --failed` lists nothing. The image still ships no Wine.
- [x] Wallpaper reaches the screen: `swaybg` + `WLR_RENDERER=pixman` after
      hyprpaper 0.8.4 segfaulted without a GPU. Verified on the rebuilt ISO
      and on the installed system, full 1280×800 (`docs/iso-session.md` §2a).
- [ ] Verify clipboard/emoji IPC, MacLock unlocking via PAM, dark/light pair.
- [ ] Publish a **GitHub Release**. The ISO is a CI artifact with
      `retention-days: 1`; there is no release, so `v0.1.0` currently points
      at nothing downloadable. One day after each build the image is gone.
- [ ] `binfmt.d/` rules installed by the package (rootless namespaced setup is
      in `tests/dispatch-e2e.sh`).
- [ ] COSMIC theme: regenerate after every design change — `cargo run -p
      tahoe-cosmic-theme` (101 files today). Consider a test for it in
      `tests/check-all.sh` rather than a manual step.
- [ ] UEL desktop integration: MacShell's Spotlight/launcher still launches
      apps as normal `.desktop` entries; wiring double-click on a `.exe` /
      `.app` / `.dmg` into `tahoe-uel` is a separate, carefully scoped task.

## 5a. A graphical installer (asked for: "like CachyOS")

CachyOS ships **Calamares** plus its own branding; so does Tahoe now, except
for the program itself. `docs/iso-installer.md` has the whole picture.

- [x] Branding in the design system's palette, module sequence, `unpackfs`
      install, launcher, `SUPER + I`, checks in `tests/check-iso.sh`.
- [x] **`calamares` is not in Arch's repositories**: built from the AUR in CI,
      present in the image (run 39265708121).
- [x] The 22 module configs, the live-state cleanup job, `rsync` /
      `squashfs-tools` / `grub` / `dosfstools` in the image, the kernel copied
      back into `/boot`, the binfmt rule and `os-release`.
- [x] A whole install in a VM, and the installed system booted to the desktop
      (`docs/iso-installer.md` §8). The run needed two manual repairs because
      its ISO predated the package fixes.
- [x] Run the **rebuilt** ISO end to end with no manual step: run 39511318414,
      clean install on a fresh 24 GiB qcow2, `All done`, disk booted alone,
      `systemctl --failed` empty (`docs/iso-installer.md` §9).
- [x] Repeat it on the MacShell 0.2.8 / Yosemite image: run 39613914918,
      installed `~/.config/macshell/vendor` = 0.2.8, the four widgets and the
      photo wallpaper byte-identical to the profile's `etc/skel`, binfmt rule
      registered (`docs/iso-installer.md` §10).
- [ ] Remove `/etc/calamares` (`branding/`, `modules/`, `settings.conf`) from
      the target in the cleanup job; it is inert but should not be there.
- [ ] Decide whether the installed system should ship populated `pacman` sync
      databases (today `pacman -Sy` is required before installing anything;
      `docs/iso-installer.md` §10).
- [ ] UEFI install (host has no OVMF; the ESP path is untested).

## 6. Known-narrow, do not lose

- [ ] Proton's child stdout does not reach the supervisor's capture (Wine's
      does; `tahoe-pe-ok` printed). Where the `steam.exe` stub sends it is
      unverified. This is why the Proton run's exit code is the evidence, not
      the captured output.
- [ ] The settings portal does not exist, so `DesignSystem::from_env()` always
      picks dark under `auto`. Wire the portal before trusting light/auto.
- [ ] No icons anywhere: tiles show an initial. A drawn approximation would be
      a worse lie than a letter — either ship a real icon theme or keep letters.
- [ ] `tahoe-preview`'s PNG writer stores uncompressed deflate blocks (6.4 MB a
      frame at 1600×1000). Fine, dependency-free, honestly documented; a real
      encoder is welcome but is not the interesting part of a desktop.

---

## Working on this

```sh
# Everything from the repo root; needs no root and touches no system state.
tests/check-all.sh            # 17 checks + 1 skip; the single required gate
cargo test                    # 624 tests, currently 0 failed
python3 tests/verify-shell-render.py <png>   # 9 assertions on a rendered scene
cargo run -p tahoe-shell --bin tahoe-preview -- --scene all --out ~/Pictures/tahoe/
```

Ground rules for contributors (short version of `LICENSING.md` and
`CONTRIBUTING.md`):

- First-party code is `MIT OR Apache-2.0`. No GPL-3 code linked into Tahoe
  binaries; Wine/Darling/Proton stay out-of-process, unmodified.
- Never report a thing as working unless the command that proves it ran and
  you can say when. `docs/STATUS.md` exists because of this rule.
- Every claim about what does not *work* yet lives in `docs/limitations.md`.
  A limitation you discover goes there before the fix does.
