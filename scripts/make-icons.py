#!/usr/bin/env python3
"""Draws the OpenDM icons and writes every size the apps, site and stores need.

A download arrow built from blocks on a white tile. Some blocks are solid
coral, some faint grey and a few faint coral: a download split across
connections finishes its chunks out of order. The tray icons are the arrow
alone in one color, for light and dark taskbars; the macOS icon puts the
tile on Apple's grid.

The arrow lives on a 7x7 grid of cells. Up to 64 px the cells are snapped to
whole pixels so the blocks stay crisp; larger sizes are drawn smooth.
Requires Pillow.

    python scripts/make-icons.py
"""
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parent.parent
TILE = "#ffffff"
EDGE_ALPHA = 0.07     # hairline around the tile so it holds on white pages
CORAL = "#FF5A3C"     # finished chunks
PENDING = "#171717"   # waiting chunks, faint
PENDING_ALPHA = 0.11
TINT_ALPHA = 0.22     # the few waiting chunks tinted coral
TILE_RADIUS = 0.225   # corner radius as a share of the tile's side
MARK = 0.72           # arrow width as a share of the tile's side
SUPERSAMPLE = 8

# The arrow, top row first: # a finished chunk, o a waiting one,
# + a waiting one tinted coral.
ARROW = [
    "..o##..",
    "..oo#..",
    "..##o..",
    "##+#+##",
    ".###+#.",
    "..+#o..",
    "...#...",
]
CELLS = [(x, y, ch) for y, row in enumerate(ARROW) for x, ch in enumerate(row) if ch != "."]
TILE_COLORS = {"#": (CORAL, 1), "o": (PENDING, PENDING_ALPHA), "+": (CORAL, TINT_ALPHA)}
GAP = 1 / 8  # space between blocks, as a share of the cell pitch
BLOCK_RADIUS = 0.2  # block corner radius, as a share of the block's side


def rgba(hex_color, alpha=1.0):
    h = hex_color.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4)) + (round(255 * alpha),)


def blocks(draw, x0, y0, width, colors, snap, scale=1):
    """The arrow's blocks in a square of the given width at (x0, y0), in
    pixels of the final image; scale is how much larger draw's canvas is.
    colors maps each chunk character in ARROW to a (color, alpha) pair.
    With snap the pitch is a whole number of pixels and the arrow is centred
    on whole pixels, and the gaps close up when blocks would be under 3 px.
    Without gaps every block is drawn like a finished one: faint pixels
    butted against solid ones read as noise, not as chunks."""
    pitch = width / 7
    if snap:
        pitch = max(1, int(pitch))
        x0 = round(x0 + (width - 7 * pitch) / 2)
        y0 = round(y0 + (width - 7 * pitch) / 2)
        gap = 0 if pitch <= 2 else max(1, round(pitch * GAP))
        radius = 0 if pitch - gap < 4 else 1
        if gap == 0:
            colors = {ch: colors["#"] for ch in colors}
    else:
        gap = pitch * GAP
        radius = (pitch - gap) * BLOCK_RADIUS
        x0, y0 = x0 + gap / 2, y0 + gap / 2
    side = pitch - gap
    for x, y, ch in CELLS:
        left, top = x0 + x * pitch, y0 + y * pitch
        box = [left * scale, top * scale, (left + side) * scale - 1, (top + side) * scale - 1]
        draw.rounded_rectangle(box, radius=radius * scale, fill=rgba(*colors[ch]))


def tile(size, inset=0, shadow=False):
    """The tile with its arrow, on a transparent square of the given size.
    inset is the transparent margin on each side, in pixels."""
    side = size - 2 * inset
    ss = SUPERSAMPLE
    canvas = Image.new("RGBA", (size * ss,) * 2, (0, 0, 0, 0))

    mask = Image.new("L", (side * ss,) * 2, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, side * ss - 1, side * ss - 1], radius=TILE_RADIUS * side * ss, fill=255)
    if shadow:
        blur = side * ss * 0.025
        drop = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
        drop.paste((0, 0, 0, 70), (inset * ss, round((inset + side * 0.015) * ss)), mask)
        canvas.alpha_composite(drop.filter(ImageFilter.GaussianBlur(blur)))
    canvas.paste(rgba(TILE), (inset * ss, inset * ss), mask)

    # A one-pixel hairline just inside the tile's edge.
    edge = Image.new("L", mask.size, 0)
    ImageDraw.Draw(edge).rounded_rectangle([0, 0, side * ss - 1, side * ss - 1], radius=TILE_RADIUS * side * ss,
                                           outline=round(255 * EDGE_ALPHA), width=ss)
    line = Image.new("RGBA", mask.size, (0, 0, 0, 0))
    line.putalpha(edge)
    canvas.alpha_composite(line, (inset * ss, inset * ss))

    # The blocks go on their own layer: faint blocks drawn straight onto the
    # tile would darken where antialiased edges overlap.
    layer = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    width = side * MARK
    start = inset + (side - width) / 2
    blocks(ImageDraw.Draw(layer), start, start, width, TILE_COLORS, snap=size <= 64, scale=ss)
    canvas.alpha_composite(layer)
    return canvas.reduce(ss)


def tray_icon(size, color):
    """The arrow alone in one color, like the system's tray icons."""
    image = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    blocks(ImageDraw.Draw(image), 0, 0, size, {"#": (color, 1), "o": (color, 0.4), "+": (color, 0.4)}, snap=True)
    return image


def mac_icon(size):
    """Apple's grid: an 824-point tile on a 1024 canvas, with a soft shadow."""
    return tile(size, inset=round(size * 100 / 1024), shadow=True)


def svg():
    s = 64
    width = s * MARK
    pitch, gap = width / 7, width / 7 * GAP
    side = pitch - gap
    start = (s - width) / 2 + gap / 2
    r = lambda v: f"{v:.2f}".rstrip("0").rstrip(".")
    rects = []
    for x, y, ch in CELLS:
        color, alpha = TILE_COLORS[ch]
        opacity = "" if alpha == 1 else f' fill-opacity="{alpha}"'
        rects.append(f'  <rect x="{r(start + x * pitch)}" y="{r(start + y * pitch)}" width="{r(side)}" '
                     f'height="{r(side)}" rx="{r(side * BLOCK_RADIUS)}" fill="{color}"{opacity}/>')
    rx = s * TILE_RADIUS
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {s} {s}">\n'
            f'  <rect width="{s}" height="{s}" rx="{r(rx)}" fill="{TILE}"/>\n'
            f'  <rect x=".5" y=".5" width="{s - 1}" height="{s - 1}" rx="{r(rx - .5)}" fill="none" '
            f'stroke="#000" stroke-opacity="{EDGE_ALPHA}"/>\n'
            + "\n".join(rects) + "\n</svg>\n")


def save_ico(path, images):
    images[-1].save(path, sizes=[im.size for im in images], append_images=images[:-1])


def main():
    windows = ROOT / "windows" / "OpenDM" / "Assets"
    save_ico(windows / "OpenDM.ico", [tile(n) for n in (16, 20, 24, 32, 40, 48, 64, 256)])
    for n in (16, 24, 32):
        tile(n).save(windows / f"OpenDM-{n}.png")
    tray = (16, 20, 24, 32, 40, 48)
    save_ico(windows / "Tray-light.ico", [tray_icon(n, "#1c1c1c") for n in tray])  # for light taskbars
    save_ico(windows / "Tray-dark.ico", [tray_icon(n, "#ffffff") for n in tray])   # for dark taskbars

    mac = [mac_icon(n) for n in (16, 32, 64, 128, 256, 512, 1024)]
    mac[-1].save(ROOT / "apple" / "AppIcon.icns", append_images=mac[:-1])

    # Chrome asks for 96 px of art with 16 px of padding in the 128 px icon.
    extension = ROOT / "extension" / "icons"
    extension.mkdir(parents=True, exist_ok=True)
    for n in (16, 32, 48):
        tile(n).save(extension / f"icon-{n}.png")
    tile(128, inset=16).save(extension / "icon-128.png")

    store = ROOT / "store"
    tile(128, inset=16, shadow=True).save(store / "chrome" / "store-icon-128.png")
    tile(300, inset=30).save(store / "edge" / "store-icon-300.png")

    site = ROOT / "site" / "assets"
    mac_icon(256).save(site / "icon.png")
    tile(64, inset=2).save(site / "favicon.png")

    (ROOT / "docs" / "icon.svg").write_text(svg(), encoding="utf-8")


if __name__ == "__main__":
    main()
