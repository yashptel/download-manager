#!/usr/bin/env python3
"""Draws the OpenDM icon and writes every size the apps need.

A charcoal tile with a white download arrow over a part-filled progress bar.
The geometry lives on a 16-unit grid inside the tile and is snapped to whole
pixels at each size, so 16 and 24 px stay crisp. Requires Pillow.

    python scripts/make-icons.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
TILE = "#2c2c2e"
GLYPH = "#ffffff"
TRACK = "#5c5c61"
SUPERSAMPLE = 16

# Tile-relative shapes on a 16-unit grid; x pairs are symmetric about the middle.
SHAFT = dict(left=7, top=2, bottom=6.5)            # 2 units wide
HEAD = dict(left=4, top=6, tip=10)                 # 45-degree sides
BAR = dict(left=4, top=12, bottom=13.5, filled=9)  # progress at 5 of 8 units


def layout(size, margin):
    """Pixel coordinates of every shape for a square icon of this size."""
    inner = 1 - 2 * margin

    def px(unit):
        return round((margin + unit / 16 * inner) * size)

    def pair(left):
        return px(left), size - px(left)

    t0, t1 = round(margin * size), size - round(margin * size)
    return {
        "tile": (t0, t0, t1, t1, round(0.225 * (t1 - t0))),
        "shaft": (*pair(SHAFT["left"]), px(SHAFT["top"]), px(SHAFT["bottom"])),
        "head": (*pair(HEAD["left"]), px(HEAD["top"]), px(HEAD["tip"])),
        "bar": (*pair(BAR["left"]), px(BAR["top"]), px(BAR["bottom"]), px(BAR["filled"])),
    }


def render(size, margin):
    s = SUPERSAMPLE
    g = layout(size, margin)
    image = Image.new("RGBA", (size * s, size * s), (0, 0, 0, 0))
    draw = ImageDraw.Draw(image)
    x0, y0, x1, y1, radius = g["tile"]
    draw.rounded_rectangle([x0 * s, y0 * s, x1 * s - 1, y1 * s - 1], radius=radius * s, fill=TILE)
    left, right, top, bottom = g["shaft"]
    draw.rectangle([left * s, top * s, right * s - 1, bottom * s - 1], fill=GLYPH)
    left, right, top, tip = g["head"]
    if size <= 24:
        # Whole-pixel steps: a smooth 45-degree edge turns gray at this size.
        rows = tip - top
        step = (right - left) / 2 / rows
        for i in range(rows):
            x0, x1 = left + round(i * step), right - round(i * step)
            draw.rectangle([x0 * s, (top + i) * s, x1 * s - 1, (top + i + 1) * s - 1], fill=GLYPH)
    else:
        draw.polygon([(left * s, top * s), (right * s, top * s), ((left + right) * s / 2, tip * s)], fill=GLYPH)
    left, right, top, bottom, filled = g["bar"]
    # Filled part and track meet at a straight edge, as in a progress bar.
    r = (bottom - top) * s / 2
    draw.rounded_rectangle([left * s, top * s, right * s - 1, bottom * s - 1], radius=r, fill=GLYPH)
    draw.rounded_rectangle([filled * s, top * s, right * s - 1, bottom * s - 1], radius=r, fill=TRACK)
    draw.rectangle([filled * s, top * s, filled * s + r, bottom * s - 1], fill=TRACK)
    return image.reduce(s)


def svg(margin=0.03, size=256):
    """The unsnapped master drawing, for documents and the web."""
    inner = 1 - 2 * margin

    def u(unit):
        return round((margin + unit / 16 * inner) * size, 2)

    t0, t1 = round(margin * size, 2), round((1 - margin) * size, 2)
    bar_h = round(u(BAR["bottom"]) - u(BAR["top"]), 2)
    bar_x, bar_y, bar_w = u(BAR["left"]), u(BAR["top"]), round(u(16 - BAR["left"]) - u(BAR["left"]), 2)
    track_x = u(BAR["filled"])
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}">
  <rect x="{t0}" y="{t0}" width="{t1 - t0}" height="{t1 - t0}" rx="{round(0.225 * (t1 - t0), 2)}" fill="{TILE}"/>
  <rect x="{u(SHAFT['left'])}" y="{u(SHAFT['top'])}" width="{u(16 - SHAFT['left']) - u(SHAFT['left'])}" height="{u(SHAFT['bottom']) - u(SHAFT['top'])}" fill="{GLYPH}"/>
  <polygon points="{u(HEAD['left'])},{u(HEAD['top'])} {u(16 - HEAD['left'])},{u(HEAD['top'])} {u(8)},{u(HEAD['tip'])}" fill="{GLYPH}"/>
  <rect x="{bar_x}" y="{bar_y}" width="{bar_w}" height="{bar_h}" rx="{round(bar_h / 2, 2)}" fill="{GLYPH}"/>
  <rect x="{track_x}" y="{bar_y}" width="{round(bar_x + bar_w - track_x, 2)}" height="{bar_h}" rx="{round(bar_h / 2, 2)}" fill="{TRACK}"/>
  <rect x="{track_x}" y="{bar_y}" width="{round(bar_h / 2, 2)}" height="{bar_h}" fill="{TRACK}"/>
</svg>
"""


def main():
    # Windows: the tile nearly fills the square, as Fluent app icons do.
    windows = ROOT / "windows" / "OpenDM" / "Assets"
    ico_sizes = [16, 20, 24, 32, 40, 48, 64, 256]
    frames = [render(n, 0.03) for n in ico_sizes]
    frames[-1].save(windows / "OpenDM.ico", sizes=[(n, n) for n in ico_sizes], append_images=frames[:-1])
    for n in (16, 24, 32):
        render(n, 0.03).save(windows / f"OpenDM-{n}.png")

    # macOS: Apple's grid puts an 824-point tile on a 1024-point canvas.
    apple = ROOT / "apple" / "Resources"
    apple.mkdir(exist_ok=True)
    icns_sizes = [16, 32, 64, 128, 256, 512, 1024]
    mac = [render(n, 100 / 1024) for n in icns_sizes]
    mac[-1].save(apple / "OpenDM.icns", append_images=mac[:-1])

    (ROOT / "docs" / "icon.svg").write_text(svg(), encoding="utf-8")


if __name__ == "__main__":
    main()

