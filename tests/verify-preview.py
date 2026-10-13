#!/usr/bin/env python3
# SPDX-License-Identifier: 0BSD
"""Independently verify the Tahoe desktop preview render.

Renders the chrome with `tahoe-preview`, then decodes the resulting PPM
(no image libraries — only `zlib` from the standard library) and checks
real visual properties: that the wallpaper is a smooth monotonic gradient,
that the panel is a flat translucent material that darkens it, that the
dock is horizontally centred, and that the focused window carries the
accent outline.

This is deliberately a *separate* implementation from the Rust tests: if
both the renderer and its tests are wrong in the same way, these checks
still catch it.

Usage:  python3 tests/verify-preview.py [--keep]
"""

import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PANEL_HEIGHT = 28  # must match tahoe_compositor::PANEL_HEIGHT

failures: list[str] = []


def check(name: str, condition: bool, detail: str = "") -> None:
    mark = "ok  " if condition else "FAIL"
    print(f"  [{mark}] {name}" + (f" — {detail}" if detail else ""))
    if not condition:
        failures.append(name)


def render(width: int, height: int, windows: int, out: Path) -> None:
    subprocess.run(
        [
            "cargo", "run", "-q", "-p", "tahoe-shell", "--bin", "tahoe-preview", "--",
            "--size", f"{width}x{height}", "--windows", str(windows), "--out", str(out),
        ],
        cwd=ROOT,
        check=True,
    )


def decode_ppm(path: Path) -> tuple[int, int, list[list[bytes]]]:
    data = path.read_bytes()
    assert data[:2] == b"P6", "not a binary PPM"
    header_end = 0
    fields: list[bytes] = []
    pos = 2
    while len(fields) < 3:
        while pos < len(data) and data[pos : pos + 1].isspace():
            pos += 1
        if data[pos : pos + 1] == b"#":
            while data[pos : pos + 1] != b"\n":
                pos += 1
            continue
        start = pos
        while pos < len(data) and not data[pos : pos + 1].isspace():
            pos += 1
        fields.append(data[start:pos])
    header_end = pos + 1
    width, height = int(fields[0]), int(fields[1])
    assert int(fields[2]) == 255, "expected 8-bit channels"
    pixels = data[header_end:]
    assert len(pixels) == width * height * 3, f"expected {width*height*3} bytes, got {len(pixels)}"
    stride = width * 3
    rows = [pixels[y * stride : (y + 1) * stride] for y in range(height)]
    return width, height, rows


def to_png(width: int, height: int, rows: list[bytes], out: Path) -> None:
    def chunk(tag: bytes, body: bytes) -> bytes:
        payload = tag + body
        return struct.pack(">I", len(body)) + payload + struct.pack(">I", zlib.crc32(payload) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + row for row in rows)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    out.write_bytes(png)


def main() -> int:
    keep = "--keep" in sys.argv
    tmp = Path(tempfile.mkdtemp(prefix="tahoe-preview-"))
    ppm = tmp / "desktop.ppm"
    png = tmp / "desktop.png"

    width, height, windows = 1440, 900, 2
    print(f"Rendering {width}x{height} with {windows} sample windows…")
    render(width, height, windows, ppm)
    w, h, rows = decode_ppm(ppm)
    to_png(w, h, rows, png)

    def px(x: int, y: int) -> bytes:
        return rows[y][x * 3 : x * 3 + 3]

    def lum(p: bytes) -> int:
        return p[0] + p[1] + p[2]

    print(f"Decoded {w}x{h} ({ppm.stat().st_size} bytes PPM, {png.stat().st_size} bytes PNG)")
    print("\nGeometry and materials:")

    check("size matches the request", (w, h) == (width, height), f"{w}x{h}")

    # A column that misses both sample windows and the dock.
    x = w - 40
    panel = [lum(px(x, y)) for y in range(0, PANEL_HEIGHT - 1)]
    hairline = px(x, PANEL_HEIGHT - 1)
    wall = [lum(px(x, y)) for y in range(PANEL_HEIGHT, h - 90)]

    check(
        "panel is a flat material",
        max(panel) - min(panel) <= 12,
        f"luminance {min(panel)}..{max(panel)}",
    )
    check(
        "panel darkens the wallpaper it covers",
        wall[0] > max(panel),
        f"panel {max(panel)} vs wallpaper {wall[0]}",
    )
    check(
        "panel has a separator hairline",
        lum(hairline) != min(panel) and lum(hairline) != max(panel),
        f"hairline {tuple(hairline)}",
    )

    steps = [abs(b - a) for a, b in zip(wall, wall[1:])]
    inversions = [b - a for a, b in zip(wall, wall[1:]) if b < a]
    check("wallpaper is a smooth gradient", max(steps) <= 2, f"max row step {max(steps)}")
    check(
        "wallpaper has many distinct values",
        len({px(x, y) for y in range(PANEL_HEIGHT, h - 90)}) > 100,
        f"{len({px(x, y) for y in range(PANEL_HEIGHT, h - 90)})} distinct rows",
    )
    check(
        "wallpaper brightens monotonically",
        min(inversions, default=0) >= -2,
        f"{len(inversions)} inversions, worst {min(inversions, default=0)} (8-bit rounding jitter)",
    )
    check(
        "wallpaper spans a wide luminance range",
        max(wall) - min(wall) > 300,
        f"{min(wall)}..{max(wall)}",
    )

    dock_y = h - 64 - 20 + 32
    background = px(5, dock_y)
    span = [cx for cx in range(w) if px(cx, dock_y) != background]
    if span:
        centre = (span[0] + span[-1]) // 2
        check("dock is horizontally centred", abs(centre - w // 2) <= 2, f"centre {centre}, want {w // 2}")
    else:
        check("dock is drawn", False, "no dock band found")

    accent = bytes((0x0A, 0x84, 0xFF))
    accent_pixels = sum(1 for row in rows for cx in range(0, w, 4) if row[cx * 3 : cx * 3 + 3] == accent)
    check("focused window carries the accent outline", accent_pixels > 100, f"{accent_pixels} pixels")

    interior = px(w // 2, h // 2 - 60)
    check("window interior is the window background", lum(interior) < 120, f"{tuple(interior)}")

    print(f"\nArtifacts kept in {tmp}" if keep else f"\n(artifacts in {tmp}; pass --keep to retain)")
    if not keep:
        for f in (ppm, png):
            f.unlink(missing_ok=True)

    if failures:
        print(f"\n{len(failures)} CHECK(S) FAILED: {', '.join(failures)}")
        return 1
    print("\nALL PREVIEW CHECKS PASSED")
    return 0


if __name__ == "__main__":
    sys.exit(main())
