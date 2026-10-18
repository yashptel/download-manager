#!/usr/bin/env python3
"""Draws the OpenDM icon and writes apple/AppIcon.icns and
windows/OpenDM/Assets/OpenDM.ico. Standard library only; the .icns step
uses macOS's iconutil.

    scripts/make-icons.py
"""
import math
import os
import shutil
import struct
import subprocess
import tempfile
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TOP = (74, 144, 255)     # gradient, top
BOTTOM = (29, 78, 216)   # gradient, bottom


def clamp(value):
    return max(0.0, min(1.0, value))


def rounded_rect_distance(x, y, left, top, right, bottom, radius):
    cx = min(max(x, left + radius), right - radius)
    cy = min(max(y, top + radius), bottom - radius)
    inside = left <= x <= right and top <= y <= bottom
    corner = math.hypot(x - cx, y - cy) - radius
    if inside and corner <= 0:
        return -min(x - left, right - x, y - top, bottom - y)
    return corner if not inside or corner > 0 else corner


def box_distance(x, y, left, top, right, bottom):
    dx = max(left - x, x - right)
    dy = max(top - y, y - bottom)
    if dx <= 0 and dy <= 0:
        return max(dx, dy)
    return math.hypot(max(dx, 0), max(dy, 0))


def triangle_distance(x, y, points):
    """Signed distance to a convex triangle (max of edge half-planes)."""
    worst = -1e9
    for i in range(3):
        (x1, y1), (x2, y2) = points[i], points[(i + 1) % 3]
        nx, ny = y2 - y1, x1 - x2
        length = math.hypot(nx, ny)
        worst = max(worst, ((x - x1) * nx + (y - y1) * ny) / length)
    return worst


def render(size, margin, radius_fraction):
    """RGBA bytes of the icon at size x size pixels."""
    s = float(size)
    left, top = s * margin, s * margin
    right, bottom = s - left, s - top
    body = right - left
    radius = body * radius_fraction
    cx = s / 2
    shaft = (cx - body * 0.085, top + body * 0.2, cx + body * 0.085, top + body * 0.52)
    head = [(cx - body * 0.24, top + body * 0.46), (cx + body * 0.24, top + body * 0.46), (cx, top + body * 0.72)]
    tray = (cx - body * 0.26, top + body * 0.78, cx + body * 0.26, top + body * 0.84)
    pixels = bytearray()
    for py in range(size):
        pixels.append(0)
        y = py + 0.5
        t = clamp((y - top) / body)
        base = tuple(int(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3))
        for px in range(size):
            x = px + 0.5
            body_cover = clamp(0.5 - rounded_rect_distance(x, y, left, top, right, bottom, radius))
            if body_cover == 0:
                pixels += b"\0\0\0\0"
                continue
            mark = min(box_distance(x, y, *shaft), triangle_distance(x, y, head), box_distance(x, y, *tray))
            white = clamp(0.5 - mark)
            color = tuple(int(base[i] + (255 - base[i]) * white) for i in range(3))
            pixels += bytes(color) + bytes([int(body_cover * 255)])
    return bytes(pixels)


def png(size, rgba):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"IDAT", zlib.compress(rgba, 9)) + chunk(b"IEND", b"")


def write_icns():
    """macOS icons sit inside the 1024 grid with a margin (body 824)."""
    iconset = os.path.join(tempfile.mkdtemp(), "AppIcon.iconset")
    os.makedirs(iconset)
    cache = {}
    for points in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            size = points * scale
            if size not in cache:
                cache[size] = png(size, render(size, 0.1, 0.225))
            suffix = "" if scale == 1 else "@2x"
            with open(os.path.join(iconset, f"icon_{points}x{points}{suffix}.png"), "wb") as file:
                file.write(cache[size])
    target = os.path.join(ROOT, "apple", "AppIcon.icns")
    subprocess.run(["iconutil", "-c", "icns", iconset, "-o", target], check=True)
    with open(os.path.join(ROOT, "build", "icon-preview.png"), "wb") as file:
        file.write(cache[512])
    shutil.rmtree(os.path.dirname(iconset))
    print(target)


def write_ico():
    """Windows icons use nearly the full square."""
    sizes = [16, 24, 32, 48, 64, 256]
    images = [png(size, render(size, 0.03, 0.2)) for size in sizes]
    entries, offset = b"", 6 + 16 * len(sizes)
    for size, data in zip(sizes, images):
        entries += struct.pack("<BBBBHHII", size % 256, size % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    target = os.path.join(ROOT, "windows", "OpenDM", "Assets", "OpenDM.ico")
    with open(target, "wb") as file:
        file.write(struct.pack("<HHH", 0, 1, len(sizes)) + entries + b"".join(images))
    print(target)


if __name__ == "__main__":
    os.makedirs(os.path.join(ROOT, "build"), exist_ok=True)
    write_ico()
    if shutil.which("iconutil"):
        write_icns()

