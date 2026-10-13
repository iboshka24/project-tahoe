#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""Generate synthetic test binaries for tahoe-format.

Usage: python3 generate.py [out_dir]

Writes small, precisely-commented binaries covering every branch of the
format parser (ELF32/64 LE/BE, PE32/PE32+, thin Mach-O, fat header,
AppImage stub, scripts, Java-class/fat ambiguity, DMG koly trailer).
Deterministic, no privileges needed, no network.
"""
import struct
import sys
from pathlib import Path

OUT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).parent


def w(name: str, data: bytes) -> None:
    p = OUT / name
    p.write_bytes(data)
    print(f"{p.name:28s} {len(data):5d} bytes")


def elf_(class_: int, data: int, osabi: int, etype: int, machine: int) -> bytes:
    endian = "<" if data == 1 else ">"
    hdr = b"\x7fELF" + bytes([class_, data, 1, osabi]) + bytes(8)
    hdr += struct.pack(endian + "H", etype) + struct.pack(endian + "H", machine)
    return hdr + bytes(44)


def pe(machine: int, opt_magic: int, subsystem: int) -> bytes:
    b = bytearray(64)
    b[0:2] = b"MZ"
    struct.pack_into("<I", b, 0x3C, 0x40)
    b += b"PE\0\0" + struct.pack("<H", machine) + bytes(18)
    b += struct.pack("<H", opt_magic)
    while len(b) < 0x40 + 24 + 68:
        b.append(0)
    b += struct.pack("<H", subsystem) + bytes(16)
    return bytes(b)


def macho(magic: bytes, cputype: int) -> bytes:
    be_file = magic[0] == 0xFE
    endian = ">" if be_file else "<"
    return magic + struct.pack(endian + "I", cputype) + bytes(24)


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    w("elf64-le-exec-x86_64", elf_(2, 1, 0, 2, 62))
    w("elf64-le-dyn-x86_64", elf_(2, 1, 3, 3, 62))
    w("elf32-le-exec-x86", elf_(1, 1, 0, 2, 3))
    w("elf32-be-rel-ppc", elf_(1, 2, 0, 1, 20))
    core = elf_(2, 1, 0, 4, 62)
    w("elf64-core", core)
    ai = bytearray(elf_(2, 1, 0, 3, 62))
    ai[8:11] = b"AI\x02"
    w("appimage-type2-stub", bytes(ai))
    w("pe32plus-x86_64-gui", pe(0x8664, 0x20B, 2))
    w("pe32-x86-console", pe(0x014C, 0x10B, 3))
    w("pe-arm64-gui", pe(0xAA64, 0x20B, 2))
    w("mz-truncated", b"MZ")
    w("macho64-x86_64", macho(bytes([0xCF, 0xFA, 0xED, 0xFE]), 0x01000007))
    w("macho64-arm64", macho(bytes([0xCF, 0xFA, 0xED, 0xFE]), 0x0100000C))
    w("macho32-x86", macho(bytes([0xCE, 0xFA, 0xED, 0xFE]), 7))
    w("fat-be-nfat2", bytes([0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 2]) + bytes(32))
    w("java8-class", bytes([0xCA, 0xFE, 0xBA, 0xBE, 0, 0, 0, 0x34]) + bytes(32))
    w("script-sh.sh", b"#!/bin/sh\necho hi\n")
    w("script-py.py", b"#!/usr/bin/env python3\nprint(1)\n")
    dmg = bytearray(0xA5 for _ in range(2048)) + b"koly" + bytes(508)
    w("dmg-koly-trailer", bytes(dmg))
    w("empty", b"")
    w("garbage", b"\x00\x01\x02\x03not a binary" + bytes(32))


if __name__ == "__main__":
    main()
