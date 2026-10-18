#!/usr/bin/env python3
"""Draws the OpenDM icons and writes every size the apps need.

A download arrow over a part-filled progress bar, drawn as a dark outline
with a white inside on a transparent background, so it reads on light and
dark surfaces alike. The tray icons are single-color versions for light and
dark taskbars; the macOS icon puts the glyph on Apple's rounded tile.

The shapes live on a 16-unit grid and are snapped to whole pixels at each
size, with a one-unit outline. Requires Pillow.

    python scripts/make-icons.py
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
INK = "#202020"
PAPER = "#ffffff"
TRACK = "#9a9a9e"
MAC_TILE = "#f2f2f4"
SUPERSAMPLE = 16

# Outer outline of the arrow on a 16-unit grid; the head has 45-degree sides.
ARROW = [(6, 1), (10, 1), (10, 6), (13, 6), (8, 11), (3, 6), (6, 6)]
# Progress bar: outer capsule and where the filled part ends.
BAR = dict(left=2, top=12, right=14, bottom=16, filled=9)
# Up to this size, edges are drawn without anti-aliasing: a 1 px diagonal
# outline turns into a gray blur, while whole-pixel steps stay crisp.
ALIASED_UP_TO = 24


def inset(points, d, staircase=False):
    """The polygon moved inward by d, with mitred corners. With staircase,
    diagonal edges move one pixel sideways instead of d along their normal,
    so an aliased 45-degree outline stays a connected one-pixel staircase."""
    area = sum(x0 * y1 - x1 * y0 for (x0, y0), (x1, y1) in zip(points, points[1:] + points[:1]))
    sign = 1 if area > 0 else -1
    lines = []
    for (x0, y0), (x1, y1) in zip(points, points[1:] + points[:1]):
        dx, dy = x1 - x0, y1 - y0
        length = (dx * dx + dy * dy) ** 0.5
        nx, ny = -dy / length * sign, dx / length * sign  # inward normal
        e = d * max(abs(nx), abs(ny)) if staircase else d
        lines.append(((x0 + nx * e, y0 + ny * e), (dx, dy)))
    out = []
    for (p, r), (q, s) in zip(lines[-1:] + lines[:-1], lines):
        cross = r[0] * s[1] - r[1] * s[0]
        t = ((q[0] - p[0]) * s[1] - (q[1] - p[1]) * s[0]) / cross
        out.append((p[0] + t * r[0], p[1] + t * r[1]))
    return out


class Canvas:
    """Maps grid units to snapped pixels and draws at SUPERSAMPLE times the size."""

    def __init__(self, size, margin, box=None):
        self.size = size
        self.ss = 1 if size <= ALIASED_UP_TO else SUPERSAMPLE
        self.image = Image.new("RGBA", (size * self.ss,) * 2, (0, 0, 0, 0))
        self.draw = ImageDraw.Draw(self.image)
        x0, y0, x1, y1 = box or (0, 0, size, size)
        self.unit = (x1 - x0) * (1 - 2 * margin) / 16
        # The glyph spans grid rows 1-16; centre it where half a unit is a whole pixel.
        shift = -0.5 * self.unit if self.unit >= 2 else 0
        self.origin = (x0 + margin * (x1 - x0), y0 + margin * (y1 - y0) + shift)
        self.stroke = max(1, round(self.unit))

    def px(self, x, y):
        return round(self.origin[0] + x * self.unit), round(self.origin[1] + y * self.unit)

    def polygon(self, points, fill):
        s = self.ss
        if s == 1:
            # Pillow fills pixel centres; shift so whole-pixel edges land on them.
            points = [(x - 0.5, y - 0.5) for x, y in points]
        self.draw.polygon([(x * s, y * s) for x, y in points], fill=fill)

    def capsule(self, x0, y0, x1, y1, fill):
        s = self.ss
        radius = (y1 - y0) * s / 2 if s > 1 else max(0, (y1 - y0) // 2 - 1)
        self.draw.rounded_rectangle([x0 * s, y0 * s, x1 * s - 1, y1 * s - 1], radius=radius, fill=fill)

    def rect(self, x0, y0, x1, y1, fill):
        s = self.ss
        self.draw.rectangle([x0 * s, y0 * s, x1 * s - 1, y1 * s - 1], fill=fill)

    def result(self):
        return self.image.reduce(self.ss) if self.ss > 1 else self.image


def draw_glyph(c, ink, paper, track):
    """The arrow and the progress bar. Inside the outline the arrow and the
    filled part of the bar are paper and the rest of the bar is track; None
    leaves a part transparent."""
    clear = (0, 0, 0, 0)
    arrow = [c.px(x, y) for x, y in ARROW]
    c.polygon(arrow, ink)
    if paper is not None:
        c.polygon(inset(arrow, c.stroke, staircase=c.ss == 1), paper)
    x0, y0 = c.px(BAR["left"], BAR["top"])
    x1, y1 = c.px(BAR["right"], BAR["bottom"])
    filled = c.px(BAR["filled"], 0)[0]
    w = c.stroke
    c.capsule(x0, y0, x1, y1, ink)
    c.capsule(x0 + w, y0 + w, x1 - w, y1 - w, track or clear)
    # The filled part starts at the left end and stops at a straight edge.
    c.capsule(x0 + w, y0 + w, filled + (y1 - y0) // 2, y1 - w, paper or ink)
    c.rect(filled, y0 + w, filled + (y1 - y0) // 2, y1 - w, track or clear)


def app_icon(size):
    c = Canvas(size, margin=0.02)
    draw_glyph(c, INK, PAPER, TRACK)
    return c.result()


def tray_icon(size, ink):
    """Single color with a transparent inside, like the system's tray icons."""
    c = Canvas(size, margin=0.02)
    draw_glyph(c, ink, None, None)
    return c.result()


def mac_icon(size):
    """Apple's grid: an 824-point tile with rounded corners on a 1024 canvas."""
    if size < 64:
        # Too small for a one-pixel outline on the tile; scale the large drawing.
        return mac_icon(1024).resize((size, size), Image.LANCZOS)
    tile = Image.new("RGBA", (size * SUPERSAMPLE,) * 2, (0, 0, 0, 0))
    t0, t1 = round(size * 100 / 1024), size - round(size * 100 / 1024)
    ImageDraw.Draw(tile).rounded_rectangle(
        [t0 * SUPERSAMPLE, t0 * SUPERSAMPLE, t1 * SUPERSAMPLE - 1, t1 * SUPERSAMPLE - 1],
        radius=0.225 * (t1 - t0) * SUPERSAMPLE, fill=MAC_TILE)
    c = Canvas(size, margin=0.17, box=(t0, t0, t1, t1))
    draw_glyph(c, INK, PAPER, TRACK)
    image = tile.reduce(SUPERSAMPLE)
    image.alpha_composite(c.result())
    return image


def svg(size=256, margin=0.02):
    u = size * (1 - 2 * margin) / 16
    o = margin * size
    oy = o - 0.5 * u  # the glyph spans rows 1-16; centre it

    def p(x, y):
        return f"{round(o + x * u, 2)},{round(oy + y * u, 2)}"

    arrow = " ".join(p(x, y) for x, y in ARROW)
    inner = " ".join(p(x, y) for x, y in inset(ARROW, 1))
    b = BAR
    x0, y0 = o + b["left"] * u, oy + b["top"] * u
    w, h = (b["right"] - b["left"]) * u, (b["bottom"] - b["top"]) * u
    ix, iy, iw, ih = x0 + u, y0 + u, w - 2 * u, h - 2 * u
    fw = (b["filled"] - b["left"] - 1) * u
    r = lambda v: round(v, 2)
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {size} {size}">
  <polygon points="{arrow}" fill="{INK}"/>
  <polygon points="{inner}" fill="{PAPER}"/>
  <rect x="{r(x0)}" y="{r(y0)}" width="{r(w)}" height="{r(h)}" rx="{r(h / 2)}" fill="{INK}"/>
  <rect x="{r(ix)}" y="{r(iy)}" width="{r(iw)}" height="{r(ih)}" rx="{r(ih / 2)}" fill="{TRACK}"/>
  <rect x="{r(ix)}" y="{r(iy)}" width="{r(fw)}" height="{r(ih)}" rx="{r(ih / 2)}" fill="{PAPER}"/>
  <rect x="{r(ix + fw - ih / 2)}" y="{r(iy)}" width="{r(ih / 2)}" height="{r(ih)}" fill="{PAPER}"/>
</svg>
"""


def save_ico(path, images):
    images[-1].save(path, sizes=[im.size for im in images], append_images=images[:-1])


def main():
    windows = ROOT / "windows" / "OpenDM" / "Assets"
    save_ico(windows / "OpenDM.ico", [app_icon(n) for n in (16, 20, 24, 32, 40, 48, 64, 256)])
    for n in (16, 24, 32):
        app_icon(n).save(windows / f"OpenDM-{n}.png")
    tray = (16, 20, 24, 32, 40, 48)
    save_ico(windows / "Tray-light.ico", [tray_icon(n, "#1c1c1c") for n in tray])  # for light taskbars
    save_ico(windows / "Tray-dark.ico", [tray_icon(n, "#ffffff") for n in tray])   # for dark taskbars

    mac = [mac_icon(n) for n in (16, 32, 64, 128, 256, 512, 1024)]
    mac[-1].save(ROOT / "apple" / "AppIcon.icns", append_images=mac[:-1])

    extension = ROOT / "extension" / "icons"
    extension.mkdir(parents=True, exist_ok=True)
    for n in (16, 32, 48, 128):
        app_icon(n).save(extension / f"icon-{n}.png")

    (ROOT / "docs" / "icon.svg").write_text(svg(), encoding="utf-8")


if __name__ == "__main__":
    main()




