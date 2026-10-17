#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""Re-encode a PNG with real compression.

`tahoe-preview` writes PNGs whose IDAT uses *stored* (uncompressed) deflate
blocks. That is valid and needs no dependency, which is why the Rust encoder
does it — but it makes a 1600x1000 screenshot 6.4 MB. This shrinks the same
pixels to a couple of hundred kilobytes so they can be shared.

Usage: python3 tests/png-compress.py <file.png> [...]
The files are replaced in place; the pixels are identical.
"""

import struct
import sys
import zlib
from pathlib import Path


def chunk(tag: bytes, body: bytes) -> bytes:
    payload = tag + body
    return struct.pack(">I", len(body)) + payload + struct.pack(">I", zlib.crc32(payload) & 0xFFFFFFFF)


def recompress(path: Path) -> tuple[int, int]:
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", f"{path}: not a PNG"
    pos, ihdr, idat, other = 8, None, b"", []
    while pos < len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        tag = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if tag == b"IHDR":
            ihdr = body
        elif tag == b"IDAT":
            idat += body
        elif tag != b"IEND":
            other.append((tag, body))
        pos += 12 + length

    raw = zlib.decompress(idat)
    out = bytearray(b"\x89PNG\r\n\x1a\n")
    out += chunk(b"IHDR", ihdr)
    for tag, body in other:
        out += chunk(tag, body)
    out += chunk(b"IDAT", zlib.compress(raw, 9))
    out += chunk(b"IEND", b"")
    before, after = len(data), len(out)
    path.write_bytes(bytes(out))
    return before, after


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    total_before = total_after = 0
    for name in sys.argv[1:]:
        path = Path(name)
        before, after = recompress(path)
        total_before += before
        total_after += after
        print(f"{path}: {before // 1024} KB -> {after // 1024} KB")
    if total_before:
        print(f"total {total_before // 1024} KB -> {total_after // 1024} KB "
              f"({100 - total_after * 100 // total_before}% smaller, pixels identical)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
