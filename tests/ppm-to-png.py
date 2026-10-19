#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""Turn QEMU screendumps (binary PPM) into PNGs that can be looked at.

    tests/ppm-to-png.py vm-out/t*.ppm

QEMU's monitor writes P6 PPM, which nothing on a desktop opens by double
click. This is the smallest thing that fixes that: no dependency, no
resizing, no colour management — the pixels that came out of the guest, in a
container every viewer understands.

Prints one line per file with the dimensions and how many distinct colours a
coarse sample found. That number is the honest quick read on a frame: a
handful of colours is a bootloader or a black screen, hundreds is a desktop.
"""

import struct
import sys
import zlib
from pathlib import Path


def read_ppm(data: bytes) -> tuple[int, int, bytes]:
    if not data.startswith(b"P6"):
        raise ValueError("not a binary PPM (expected P6)")
    fields: list[int] = []
    pos = 2
    while len(fields) < 3:
        # Comments start with '#' and run to the end of the line; QEMU does
        # not write any, but a PPM reader that ignores them is a bug waiting
        # for the first file that does.
        while pos < len(data) and data[pos : pos + 1].isspace():
            pos += 1
        if data[pos : pos + 1] == b"#":
            while pos < len(data) and data[pos : pos + 1] != b"\n":
                pos += 1
            continue
        start = pos
        while pos < len(data) and not data[pos : pos + 1].isspace():
            pos += 1
        fields.append(int(data[start:pos]))
    pos += 1  # single whitespace byte after maxval
    width, height, maxval = fields
    if maxval != 255:
        raise ValueError(f"only 8-bit PPM is handled, got maxval {maxval}")
    pixels = data[pos : pos + width * height * 3]
    if len(pixels) != width * height * 3:
        raise ValueError(f"truncated: {len(pixels)} bytes for {width}x{height}")
    return width, height, pixels


def chunk(tag: bytes, body: bytes) -> bytes:
    payload = tag + body
    return struct.pack(">I", len(body)) + payload + struct.pack(">I", zlib.crc32(payload) & 0xFFFFFFFF)


def to_png(width: int, height: int, rgb: bytes) -> bytes:
    # PNG scanlines need a filter byte; filter 1 (Sub) costs one subtraction
    # per byte and helps a lot on the flat areas a desktop is made of.
    stride = width * 3
    raw = bytearray()
    previous = bytes(stride)
    for y in range(height):
        row = rgb[y * stride : (y + 1) * stride]
        filtered = bytes((row[i] - row[i - 3]) & 0xFF if i >= 3 else row[i] for i in range(stride))
        raw += b"\x01" + filtered
        previous = row
    out = bytearray(b"\x89PNG\r\n\x1a\n")
    out += chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    out += chunk(b"IDAT", zlib.compress(bytes(raw), 6))
    out += chunk(b"IEND", b"")
    return bytes(out)


def sampled_colours(rgb: bytes, width: int, height: int, step: int = 7) -> int:
    seen = set()
    for y in range(0, height, step):
        row = y * width * 3
        for x in range(0, width, step):
            i = row + x * 3
            seen.add(rgb[i : i + 3])
    return len(seen)


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    for name in sys.argv[1:]:
        path = Path(name)
        width, height, rgb = read_ppm(path.read_bytes())
        png = path.with_suffix(".png")
        png.write_bytes(to_png(width, height, rgb))
        print(f"{png}  {width}x{height}  {sampled_colours(rgb, width, height)} distinct colours (sampled)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
