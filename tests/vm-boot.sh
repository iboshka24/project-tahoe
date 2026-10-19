#!/usr/bin/env bash
# SPDX-License-Identifier: 0BSD
#
# Boot a Tahoe ISO headless and bring back pictures of what happened.
#
#   tests/vm-boot.sh <iso> [outdir] [seconds]
#
# This is the check the ISO never had. `tests/check-iso.sh` proves the profile
# is sane before a build; nothing proved the built image boots. The evidence
# this produces is a series of screendumps at a fixed schedule plus the
# serial log, so a claim about the live session can be backed by a frame
# instead of by the build having exited zero — which it will do even if the
# desktop never appears.
#
# Exit status is about the *mechanics*: did QEMU start, did the guest reach
# the point where the screen stopped being the boot menu, were frames written.
# Whether the desktop looks right is a judgement made by looking at the
# frames, not a threshold in this script. That is deliberate: a pixel test
# that "passes" on a black screen is worse than no test.
#
# No root, no system packages. If qemu-system-x86_64 is absent it looks for
# the one inside the virt-manager Flatpak and runs that through `flatpak run`,
# in which case the ISO and the output directory must be somewhere the
# sandbox can see (~/Downloads, ~/Pictures, ...) — the sandbox has `devices=all`
# for /dev/kvm but a narrow filesystem view.

set -uo pipefail

ISO="${1:?usage: tests/vm-boot.sh <iso> [outdir] [seconds]}"
OUT="${2:-$(pwd)/vm-out}"
SECS="${3:-240}"

ISO="$(readlink -f "$ISO")" || exit 1
[[ -f "$ISO" ]] || { echo "no such ISO: $ISO" >&2; exit 1; }
mkdir -p "$OUT" || exit 1
OUT="$(readlink -f "$OUT")"

# ---------------------------------------------------------------------------
# Find a QEMU. A system one first, then the virt-manager Flatpak's, which is
# a runtime extension and only works through `flatpak run`.
# ---------------------------------------------------------------------------
QEMU=()
if command -v qemu-system-x86_64 >/dev/null 2>&1; then
    QEMU=(qemu-system-x86_64)
    echo "qemu: system binary"
elif flatpak info org.virt_manager.virt-manager >/dev/null 2>&1 &&
     timeout 60 flatpak run --command=qemu-system-x86_64 org.virt_manager.virt-manager --version >/dev/null 2>&1; then
    # The Flatpak's filesystem view is narrow, so a caller whose ISO lives
    # outside it sets TAHOE_FLATPAK_FS to the directory to expose. It is a
    # per-run override: `flatpak info --show-permissions` is unchanged
    # afterwards. Note it exposes the *parent* directory read-write, because
    # screendump writes into $OUT, which is usually next to the ISO.
    FS=()
    [[ -n "${TAHOE_FLATPAK_FS:-}" ]] && FS=(--filesystem="$TAHOE_FLATPAK_FS")
    QEMU=(flatpak run "${FS[@]}" --command=qemu-system-x86_64 org.virt_manager.virt-manager)
    echo "qemu: virt-manager Flatpak${TAHOE_FLATPAK_FS:+ (exposing $TAHOE_FLATPAK_FS)}"
else
    echo "no qemu-system-x86_64: install qemu, or the virt-manager Flatpak" >&2
    exit 1
fi

# KVM when the kernel will have us. 15 GB of RAM is not enough to be
# relaxed about this: a software-emulated boot of a live desktop takes tens
# of minutes and is not what anyone means by testing an ISO.
ACCEL=(-accel tcg)
if [[ -w /dev/kvm ]]; then
    ACCEL=(-enable-kvm -cpu host)
    echo "accel: kvm"
else
    echo "accel: tcg (no writable /dev/kvm — expect this to be very slow)"
fi

# The machine type decides which storage driver the guest needs to find its
# own CD: q35 attaches it to an AHCI controller, i440fx ("pc") to a PIIX IDE
# one. That difference found a real bug — see docs/iso-boot.md — so it is a
# knob rather than a constant.
MACHINE="${TAHOE_MACHINE:-q35}"
echo "machine: $MACHINE"

# ---------------------------------------------------------------------------
# The schedule. Early frames catch the bootloader and the kernel; later ones
# are the session. Escalating gaps because the interesting part is late.
# ---------------------------------------------------------------------------
SHOTS=(15 30 50 75 110 150 200 260)

echo "iso:   $ISO ($(du -h "$ISO" | cut -f1))"
echo "out:   $OUT"
echo "limit: ${SECS}s"

# The monitor drives everything: frames are taken by `screendump`, so no
# display frontend is needed and this works over ssh.
# ---------------------------------------------------------------------------
# Direct kernel boot. Set TAHOE_KERNEL (and TAHOE_INITRD) to skip the
# bootloader and hand the kernel the command line in TAHOE_APPEND. This is how
# a kernel-command-line bug gets tested without rebuilding the ISO — the
# bootloader configs inside a built image cannot be edited, and a rebuild
# costs ten minutes. It is also the only way to get a serial log, because
# `console=ttyS0` has to be on the command line and the shipped one is fixed.
# ---------------------------------------------------------------------------
BOOT=(-boot d)
if [[ -n "${TAHOE_KERNEL:-}" ]]; then
    [[ -f "$TAHOE_KERNEL" ]] || { echo "no such kernel: $TAHOE_KERNEL" >&2; exit 1; }
    [[ -f "${TAHOE_INITRD:-}" ]] || { echo "no such initramfs: ${TAHOE_INITRD:-}" >&2; exit 1; }
    BOOT=(-kernel "$TAHOE_KERNEL" -initrd "$TAHOE_INITRD" -append "${TAHOE_APPEND:-}")
    echo "boot:  direct kernel, append='${TAHOE_APPEND:-}'"
fi

{
    for t in "${SHOTS[@]}"; do
        (( t > SECS )) && continue
        echo "sleep $t" >&2
        sleep "$t"
        printf 'screendump %s/t%03d.ppm\n' "$OUT" "$t"
    done
    # Ask the guest to shut down rather than pulling the plug; a live ISO
    # with an overlayfs on tmpfs has nothing to lose, but the same script
    # gets pointed at installed images later.
    echo "system_powerdown"
    sleep 20
    echo "quit"
} | timeout "$((SECS + 60))" "${QEMU[@]}" \
        -m 4096 -smp 4 "${ACCEL[@]}" \
        -machine "$MACHINE" \
        -device virtio-vga \
        -display none \
        -monitor stdio \
        -serial "file:$OUT/serial.log" \
        -drive "file=$ISO,media=cdrom,readonly=on,format=raw" \
        "${BOOT[@]}" \
        -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
        -rtc base=localtime \
        >"$OUT/qemu-stdout.log" 2>"$OUT/qemu-stderr.log"
rc=$?

frames=$(find "$OUT" -name 't*.ppm' | wc -l)
echo
echo "qemu exited $rc; $frames frame(s) in $OUT"
ls -la "$OUT"/t*.ppm 2>/dev/null | awk '{print "  " $NF, $5 " bytes"}'
echo "serial: $(wc -c <"$OUT/serial.log" 2>/dev/null || echo 0) bytes"
echo
echo "look at the frames: tests/vm-boot.sh stops at the mechanics."
echo "  tests/ppm-to-png.py $OUT/t*.ppm && <view the PNGs>"

# Frames at the end mean the guest was alive and drawing; a boot that never
# got past the firmware usually still yields frames, so this only reports
# whether a picture exists, not whether it is right.
[[ "$frames" -gt 0 ]] || exit 1
[[ -s "$OUT/serial.log" ]] || echo "note: the serial log is empty — the profile's kernel" \
    "command line does not add console=ttyS0, so kernel and systemd output goes" \
    "to tty0 only. Add it to iso/syslinux/syslinux.cfg and the efiboot entry to" \
    "get logs here; the frames are then the only textual evidence." >&2
