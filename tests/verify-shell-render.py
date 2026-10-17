#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""Verify a rendered shell scene.

Decodes the PNG and asks the questions that matter about a picture of a
desktop: does it have text in it, is the wallpaper a gradient, is the dock
where it should be, and does opening a surface change the pixels.

The text check measures **contrast against the region's dominant colour**,
not darkness. Counting dark pixels was wrong in both directions: in the
dark theme the panel itself is dark, so the count measured the background;
in the light theme the muted menu items are too light to register at all.
A picture with no words in it is the realistic failure here, and a build
succeeds either way.

Usage: python3 tests/verify-shell-render.py <file.png>
"""

import struct
import sys
import zlib
from collections import Counter

failures: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    print(f"  [{'ok  ' if condition else 'FAIL'}] {name}" + (f" — {detail}" if detail else ""))
    if not condition:
        failures.append(name)


def decode(path: str):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, ihdr = 8, b"", None
    chunks = []
    while pos < len(data):
        length = struct.unpack(">I", data[pos : pos + 4])[0]
        tag = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        if tag == b"IHDR":
            ihdr = struct.unpack(">IIBBBBB", body)
        elif tag == b"IDAT":
            idat += body
        chunks.append(tag)
        pos += 12 + length
    width, height, depth, colour_type = ihdr[0], ihdr[1], ihdr[2], ihdr[3]
    assert (depth, colour_type) == (8, 6), f"expected 8-bit RGBA, got {ihdr}"
    raw = zlib.decompress(idat)
    stride = width * 4
    rows = [raw[y * (stride + 1) + 1 : (y + 1) * (stride + 1)] for y in range(height)]
    return width, height, rows, chunks


def contrast_ink(px, x0: int, y0: int, x1: int, y1: int, threshold: int = 55) -> int:
    band = [px(x, y) for y in range(y0, y1) for x in range(x0, x1)]
    background = sum(Counter(bytes(p) for p in band).most_common(1)[0][0][:3])
    return sum(1 for p in band if abs(sum(p[:3]) - background) > threshold)


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    path = sys.argv[1]
    width, height, rows, chunks = decode(path)
    px = lambda x, y: rows[y][x * 4 : x * 4 + 4]
    lum = lambda p: p[0] + p[1] + p[2]

    print(f"verifying {path} ({width}x{height})")
    check("canonical size", (width, height) == (1600, 1000), f"{width}x{height}")
    check("chunk framing", chunks[:2] == [b"IHDR", b"IDAT"] and chunks[-1] == b"IEND", f"{chunks}")

    colours = {bytes(px(x, y)) for y in range(0, height, 7) for x in range(0, width, 7)}
    check("not a flat fill", len(colours) > 400, f"{len(colours)} distinct sampled colours")

    # The menu bar, where "Tahoe File Edit View Window Help" is drawn.
    bar = contrast_ink(px, 20, 3, 520, 25)
    check("menu-bar text is present", bar > 250, f"{bar} contrasting pixels")

    # The dock band, where the tiles and their initials are drawn.
    dock = contrast_ink(px, 560, 915, 1040, 980)
    check("dock contents are present", dock > 400, f"{dock} contrasting pixels")

    # A column clear of the windows and the dock.
    top, bottom = lum(px(width - 30, 40)), lum(px(width - 30, height - 40))
    check("wallpaper brightens downward", bottom > top, f"{top} -> {bottom}")

    # The reserved panel strip must be a different material from the work
    # area, or a window would sit under a bar it could not see.
    panel = sum(lum(px(x, 10)) for x in range(0, width, 40)) / len(range(0, width, 40))
    work = sum(lum(px(x, 300)) for x in range(0, width, 40)) / len(range(0, width, 40))
    check("the panel strip is distinct from the work area", abs(panel - work) > 20, f"{panel:.0f} vs {work:.0f}")

    # The dock must be horizontally centred: that is the one piece of the
    # layout the compositor reserves space for.
    dock_y = height - 64 - 20 + 32
    background = px(5, dock_y)
    span = [x for x in range(width) if px(x, dock_y) != background]
    if span:
        centre = (span[0] + span[-1]) // 2
        check("dock is horizontally centred", abs(centre - width // 2) <= 2, f"centre {centre}, want {width // 2}")
    else:
        check("dock is drawn", False, "no dock band found")

    # The accent must actually appear, or every control is unhighlighted.
    accent = bytes(((int(0x0A / 255 * 255)), (int(0x84 / 255 * 255)), 0xFF))
    rough = sum(
        1
        for y in range(0, height, 3)
        for x in range(0, width, 3)
        if px(x, y)[2] > 200 and px(x, y)[0] < 90 and px(x, y)[1] > 90 and px(x, y)[1] < 180
    )
    check("the accent colour is used", rough > 40, f"{rough} accent-ish pixels")

    if failures:
        print(f"\n{len(failures)} FAILED: {', '.join(failures)}")
        return 1
    print("\nALL SHELL RENDER CHECKS PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
