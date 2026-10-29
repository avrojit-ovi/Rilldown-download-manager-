#!/usr/bin/env python3
"""Generate Rilldown's original application and extension icons.

No third-party libraries are required: PNGs are written directly and the ICO
container embeds PNG entries (supported by Windows Vista and later). The mark is
three streams falling into a tray - a small, literal riff on the name.
"""

from __future__ import annotations

import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SUPERSAMPLE = 3

BACKGROUND_TOP = (10, 42, 41)
BACKGROUND_BOTTOM = (20, 82, 78)
STREAMS = [
    (0.345, 0.215, 0.640, (124, 231, 214)),
    (0.500, 0.215, 0.745, (238, 255, 250)),
    (0.655, 0.215, 0.690, (74, 193, 176)),
]
STREAM_WIDTH = 0.092
TRAY = (0.265, 0.800, 0.735, 0.850, (124, 231, 214))


def inside_rounded_rect(x: float, y: float, x0: float, y0: float, x1: float, y1: float, radius: float) -> bool:
    if not (x0 <= x <= x1 and y0 <= y <= y1):
        return False
    cx = min(max(x, x0 + radius), x1 - radius)
    cy = min(max(y, y0 + radius), y1 - radius)
    return (x - cx) ** 2 + (y - cy) ** 2 <= radius**2


def inside_capsule(x: float, y: float, cx: float, top: float, bottom: float, radius: float) -> bool:
    if not (top <= y <= bottom):
        return False
    if abs(x - cx) <= radius and top + radius <= y <= bottom - radius:
        return True
    for cy in (top + radius, bottom - radius):
        if (x - cx) ** 2 + (y - cy) ** 2 <= radius**2:
            return True
    return False


def sample(nx: float, ny: float) -> tuple[int, int, int, int]:
    if not inside_rounded_rect(nx, ny, 0.035, 0.035, 0.965, 0.965, 0.215):
        return (0, 0, 0, 0)

    t = (ny - 0.035) / 0.93
    t = min(max(t, 0.0), 1.0)
    color = tuple(
        round(BACKGROUND_TOP[i] + (BACKGROUND_BOTTOM[i] - BACKGROUND_TOP[i]) * t)
        for i in range(3)
    )

    for cx, top, bottom, stream_color in STREAMS:
        if inside_capsule(nx, ny, cx, top, bottom, STREAM_WIDTH / 2):
            color = stream_color
            break

    x0, y0, x1, y1, tray_color = TRAY
    if inside_rounded_rect(nx, ny, x0, y0, x1, y1, (y1 - y0) / 2):
        color = tray_color

    return (*color, 255)


def render(size: int) -> bytes:
    pixels = bytearray(size * size * 4)
    samples = SUPERSAMPLE * SUPERSAMPLE
    for py in range(size):
        for px in range(size):
            red = green = blue = alpha = 0
            for sy in range(SUPERSAMPLE):
                for sx in range(SUPERSAMPLE):
                    nx = (px + (sx + 0.5) / SUPERSAMPLE) / size
                    ny = (py + (sy + 0.5) / SUPERSAMPLE) / size
                    r, g, b, a = sample(nx, ny)
                    red += r * a
                    green += g * a
                    blue += b * a
                    alpha += a
            offset = (py * size + px) * 4
            if alpha == 0:
                continue
            pixels[offset] = round(red / alpha)
            pixels[offset + 1] = round(green / alpha)
            pixels[offset + 2] = round(blue / alpha)
            pixels[offset + 3] = round(alpha / samples)
    return bytes(pixels)


def png_bytes(size: int) -> bytes:
    pixels = render(size)
    raw = b"".join(
        b"\x00" + pixels[y * size * 4 : (y + 1) * size * 4] for y in range(size)
    )
    compressed = zlib.compress(raw, 9)

    def chunk(tag: bytes, data: bytes) -> bytes:
        return (
            struct.pack(">I", len(data))
            + tag
            + data
            + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)
        )

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", compressed)
        + chunk(b"IEND", b"")
    )


def ico_bytes(sizes: list[int]) -> bytes:
    images = [(size, png_bytes(size)) for size in sizes]
    header = struct.pack("<HHH", 0, 1, len(images))
    entries = b""
    payload = b""
    offset = 6 + 16 * len(images)
    for size, data in images:
        dimension = 0 if size >= 256 else size
        entries += struct.pack(
            "<BBBBHHII",
            dimension,
            dimension,
            0,
            0,
            1,
            32,
            len(data),
            offset,
        )
        offset += len(data)
        payload += data
    return header + entries + payload


def main() -> None:
    app_icons = ROOT / "src-tauri" / "icons"
    extension_icons = ROOT / "extension" / "icons"
    app_icons.mkdir(parents=True, exist_ok=True)
    extension_icons.mkdir(parents=True, exist_ok=True)

    cache: dict[int, bytes] = {}

    def png(size: int) -> bytes:
        if size not in cache:
            cache[size] = png_bytes(size)
        return cache[size]

    for size, name in [
        (32, "32x32.png"),
        (128, "128x128.png"),
        (256, "128x128@2x.png"),
        (512, "icon.png"),
    ]:
        (app_icons / name).write_bytes(png(size))

    (app_icons / "icon.ico").write_bytes(ico_bytes([16, 32, 48, 64, 128, 256]))

    for size in (16, 32, 48, 128):
        (extension_icons / f"icon-{size}.png").write_bytes(png(size))

    print(f"wrote {len(list(app_icons.iterdir()))} app icons and 4 extension icons")


if __name__ == "__main__":
    main()
