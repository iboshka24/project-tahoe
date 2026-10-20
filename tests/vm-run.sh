#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Boot a Project Tahoe ISO in a window you can use, on this machine.
#
# This is the interactive counterpart to tests/vm-boot.sh, which is headless
# and takes timed screenshots. Use this one to look at the desktop yourself.
#
#   tests/vm-run.sh ~/Downloads/tahoe-iso/tahoe-*.iso
#
# Afterwards, close the window (or File > Quit) to power the machine off. The
# disk image is not deleted, so `--disk-boot` on a later run starts the system
# you installed instead of the live medium.
#
# Options:
#   --disk-boot        boot the hard disk instead of the ISO (after an install)
#   --disk <path>      use this disk image (default: tahoe-vm-disk.qcow2)
#   --memory <MiB>     guest RAM (default: 4096)
#   --display <name>   gtk (default), sdl, or none
#   --dry-run          print the QEMU command and exit
#
# Environment: TAHOE_QEMU overrides the QEMU binary, TAHOE_QEMU_IMG the image
# tool. On this machine QEMU only exists inside the virt-manager Flatpak, so
# the sandbox's filesystem permissions decide which paths the guest can see:
# ~/Downloads is already allowed, /tmp is not, and this script adds the ISO's
# own directory for the run rather than copying 1.7 GB around.

set -euo pipefail

usage() {
    sed -n '3,24p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

disk_boot=0
disk=""
memory=4096
display=gtk
dry_run=0
iso=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --disk-boot) disk_boot=1 ;;
        --disk) disk="$2"; shift ;;
        --memory) memory="$2"; shift ;;
        --display) display="$2"; shift ;;
        --dry-run) dry_run=1 ;;
        -h|--help) usage 0 ;;
        -*) echo "unknown option: $1" >&2; usage 1 ;;
        *) iso="$1" ;;
    esac
    shift
done

[[ -n "$iso" ]] || usage 1
[[ -f "$iso" ]] || { echo "no such ISO: $iso" >&2; exit 1; }
iso="$(realpath "$iso")"
[[ "$display" =~ ^(gtk|sdl|none)$ ]] || { echo "display must be gtk, sdl or none" >&2; exit 1; }

[[ -n "$disk" ]] || disk="$HOME/Downloads/tahoe-iso/tahoe-vm-disk.qcow2"
mkdir -p "$(dirname "$disk")"

# System QEMU if there is one, otherwise the Flatpak. The Flatpak gets the
# ISO's directory added for this run only; --filesystem is a per-run override
# and does not change the application's stored permissions.
if [[ -n "${TAHOE_QEMU:-}" ]]; then
    qemu=("$TAHOE_QEMU")
    qemu_img=("${TAHOE_QEMU_IMG:-qemu-img}")
elif command -v qemu-system-x86_64 >/dev/null 2>&1; then
    qemu=(qemu-system-x86_64)
    qemu_img=(qemu-img)
else
    qemu=(flatpak run
          --filesystem="$(dirname "$iso")" --filesystem="$(dirname "$disk")"
          --command=qemu-system-x86_64 org.virt_manager.virt-manager)
    qemu_img=(flatpak run --filesystem="$(dirname "$disk")"
              --command=qemu-img org.virt_manager.virt-manager)
fi

if [[ "$dry_run" -eq 0 && ! -f "$disk" ]]; then
    echo "creating $disk (24 GiB, sparse)"
    "${qemu_img[@]}" create -f qcow2 "$disk" 24G >/dev/null
fi

args=(
    -machine q35
    -enable-kvm -cpu host
    -smp 4 -m "$memory"
    # virtio-vga is what the guest's Mesa can drive without a real GPU.
    -device virtio-vga
    -display "$display"
    # Without a tablet the guest sees a relative mouse and the pointer drifts
    # away from the window edge; the tablet gives absolute coordinates.
    -device qemu-xhci -device usb-tablet
    -nic user,model=virtio-net-pci
    -drive "file=$disk,if=virtio,format=qcow2"
)

if [[ "$disk_boot" -eq 1 ]]; then
    args+=(-boot c)
else
    args+=(-drive "file=$iso,media=cdrom,readonly=on,format=raw" -boot d)
fi

if [[ "$dry_run" -eq 1 ]]; then
    printf '%q ' "${qemu[@]}" "${args[@]}"
    printf '\n'
    exit 0
fi

echo "booting ${iso##*/} (${memory} MiB, ${display})"
exec "${qemu[@]}" "${args[@]}"
