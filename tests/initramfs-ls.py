#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""List or search a kernel initramfs, every segment of it.

    tests/initramfs-ls.py <initramfs> [pattern]

An initramfs is not one archive. mkinitcpio and archiso concatenate several
cpio segments, and the early segment is deliberately left *uncompressed* so
the kernel can read microcode before it is able to decompress anything. A tool
that reads "the" archive stops at the first trailer and reports a truncated
view: `bsdtar -tf initramfs-linux.img` on the Tahoe image lists 3262 entries
and no `init`, no `/usr/bin`, and no `blkid` — and every one of those absences
is an artefact of the truncation, not a fact about the image.

This walks each segment in turn, reporting the segment's compression so a
"missing" path can be told apart from an unread one. It is read-only and has
no dependencies outside the standard library, which is the point: the tool
that explains a failed boot should not need a working boot.

Compression support: none, xz, gzip and zstd (the last needs Python 3.14's
`compression.zstd`; without it a zstd segment is reported and skipped rather
than silently ignored).
"""

import argparse
import io
import lzma
import re
import sys
import zlib
from pathlib import Path

CPIO_MAGIC = (b"070701", b"070702", b"070707")
MAGIC = {
    b"\xfd7zXZ\x00": "xz",
    b"\x1f\x8b": "gzip",
    b"\x28\xb5\x2f\xfd": "zstd",
}


def decompress(data: bytes) -> tuple[bytes, str, int] | None:
    """Decompress one layer, returning (output, name, bytes_consumed).

    `bytes_consumed` is 0 when the end of the layer cannot be known without
    decoding it, in which case the caller falls back to searching for the next
    segment magic. Returning 0 is honest; guessing would be worse.
    """
    for magic, name in MAGIC.items():
        if not data.startswith(magic):
            continue
        if name == "xz":
            dec = lzma.LZMADecompressor()
            try:
                out = dec.decompress(data)
            except lzma.LZMAError as exc:
                print(f"xz layer at this offset failed: {exc}", file=sys.stderr)
                return None
            return out, name, len(data) - len(dec.unused_data)
        if name == "gzip":
            # A two-byte magic is not a signature: inside a 12 MB compressed
            # stream it occurs by chance, and treating that as a gzip layer
            # crashes with "unknown compression method". Require the method
            # byte too, and fail soft.
            if len(data) < 3 or data[2] != 0x08:
                continue
            obj = zlib.decompressobj(31)
            try:
                out = obj.decompress(data)
            except zlib.error:
                continue
            return out, name, len(data) - len(obj.unused_data)
        if name == "zstd":
            try:
                from compression import zstd  # Python 3.14+
            except ImportError:
                return b"", "zstd (unsupported here)", 0
            dec = zstd.ZstdDecompressor()
            return dec.decompressobj().decompress(data), name, 0
    return None


def walk_cpio(data: bytes, start: int = 0):
    """Yield (name, size, mode) for one newc/crc cpio image.

    Returns nothing; the caller needs the offset *after* the trailer, which
    is not simply the sum of the entry sizes — mkinitcpio pads between
    segments — so `end_of_cpio()` below finds it the honest way.
    """
    pos = start
    while pos + 110 <= len(data):
        magic = data[pos : pos + 6]
        if magic not in CPIO_MAGIC:
            return
        fields = [int(data[pos + 6 + i * 8 : pos + 14 + i * 8], 16) for i in range(13)]
        mode, filesize = fields[1], fields[6]
        namesize = fields[11]
        name_start = pos + 110
        raw_name = data[name_start : name_start + namesize - 1]
        name = raw_name.decode("utf-8", "replace")
        pos = (name_start + namesize + 3) & ~3
        if name == "TRAILER!!!":
            return
        yield (name, filesize, mode)
        pos = (pos + filesize + 3) & ~3


def end_of_cpio(data: bytes, start: int) -> int:
    """Offset just past the TRAILER!!! record of the cpio image at `start`."""
    pos = start
    while pos + 110 <= len(data):
        if data[pos : pos + 6] not in CPIO_MAGIC:
            return pos
        fields = [int(data[pos + 6 + i * 8 : pos + 14 + i * 8], 16) for i in range(13)]
        filesize, namesize = fields[6], fields[11]
        name = data[pos + 110 : pos + 110 + namesize - 1]
        pos = (pos + 110 + namesize + 3) & ~3
        if name == b"TRAILER!!!":
            return (pos + filesize + 3) & ~3
        pos = (pos + filesize + 3) & ~3
    return pos


def next_magic(data: bytes, start: int) -> int:
    """The earliest offset >= start where any known segment starts.

    Arithmetic on segment sizes gets this wrong: the gaps between mkinitcpio's
    segments are real, and a 1464-byte gap after the first trailer is what made
    an earlier version of this script declare the remaining 12 MB of xz
    "unrecognised".
    """
    candidates = [data.find(magic, start) for magic in (*MAGIC, *CPIO_MAGIC)]
    return min((c for c in candidates if c != -1), default=len(data))


def segments(path: Path):
    """Yield (label, entries) for each archive segment in the file."""
    data = path.read_bytes()
    pos = 0
    while pos < len(data):
        if data[pos : pos + 6] in CPIO_MAGIC:
            entries = list(walk_cpio(data, pos))
            yield f"cpio (uncompressed) @ {pos}", entries
            pos = next_magic(data, end_of_cpio(data, pos))
            continue
        layer = decompress(data[pos:])
        if layer is None:
            print(f"{len(data) - pos} unrecognised bytes at {pos}", file=sys.stderr)
            return
        out, name, _consumed = layer
        yield f"cpio via {name} @ {pos}", list(walk_cpio(out))
        pos = next_magic(data, pos + 1)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("initramfs", type=Path)
    ap.add_argument("pattern", nargs="?", default="",
                    help="regular expression; only matching paths are printed")
    ap.add_argument("--count", action="store_true", help="print only counts per segment")
    args = ap.parse_args()

    rx = re.compile(args.pattern) if args.pattern else None
    total = 0
    for label, entries in segments(args.initramfs):
        total += len(entries)
        print(f"== {label}: {len(entries)} entries")
        if args.count or not rx:
            continue
        for name, size, mode in entries:
            if rx.search(name):
                print(f"   {mode:06o} {size:>10} {name}")
    print(f"== total {total} entries")
    return 0


if __name__ == "__main__":
    sys.exit(main())
