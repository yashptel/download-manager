#!/usr/bin/env python3
"""Renders the store listing graphics, the GitHub images and the site's
og image at the exact sizes each place asks for.

Every image is an HTML page drawn with headless Google Chrome: the icon from
docs/icon.svg, the Geist font from site/fonts, the app captures in
site/assets, and a faded field of the icon's chunks behind them. The captures
predate the new icon, so the old toolbar and popup icon in them is covered
with the new one first. Requires Pillow and Google Chrome.

    python3 store/compose.py
"""
import importlib.util
import random
import shutil
import subprocess
import tempfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
STORE = ROOT / "store"
ASSETS = ROOT / "site" / "assets"
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"

INK, INK_2, CORAL = "#171717", "#5d5d5f", "#FF5A3C"

spec = importlib.util.spec_from_file_location("make_icons", ROOT / "scripts" / "make-icons.py")
make_icons = importlib.util.module_from_spec(spec)
spec.loader.exec_module(make_icons)

WORK = Path(tempfile.mkdtemp(prefix="opendm-compose-"))


def captures():
    """Copies of the app captures with the old icon covered by the new one."""
    out = {}
    for name in ("main", "fileinfo", "progress", "popup", "video-menu", "settings"):
        im = Image.open(ASSETS / f"{name}.webp").convert("RGBA")
        if name == "popup":  # the 22 pt icon in the popup's header, at 2x
            im.paste((255, 255, 255, 255), (26, 40, 74, 92))
            im.alpha_composite(make_icons.tile(44), (28, 44))
        if name == "video-menu":  # the extension's toolbar button
            im.paste((255, 255, 255, 255), (1410, 76, 1446, 110))
            im.alpha_composite(make_icons.tile(32), (1412, 77))
        path = WORK / f"{name}.png"
        im.save(path)
        out[name] = path.as_uri()
    return out


CAP = captures()
ICON = (ROOT / "docs" / "icon.svg").as_uri()
FONT = (ROOT / "site" / "fonts" / "Geist-Variable.woff2").as_uri()


def chunks(width, height, pitch, seed, solid=0.06, tint=0.12, ink=0.045):
    """A field of the icon's blocks: a few finished in coral, some tinted
    coral, the rest faint ink. Returned as an inline SVG."""
    rnd = random.Random(seed)
    side = pitch * 7 / 8
    rx = side * 0.2
    rects = []
    for y in range(0, height + pitch, pitch):
        for x in range(0, width + pitch, pitch):
            r = rnd.random()
            fill = (f'fill="{CORAL}"' if r < solid else
                    f'fill="{CORAL}" fill-opacity="0.2"' if r < solid + tint else
                    f'fill="{INK}" fill-opacity="{ink}"')
            rects.append(f'<rect x="{x}" y="{y}" width="{side:g}" height="{side:g}" rx="{rx:g}" {fill}/>')
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" '
            f'viewBox="0 0 {width} {height}">{"".join(rects)}</svg>')


BASE = """
@font-face { font-family: Geist; src: url(%(font)s) format("woff2"); font-weight: 100 900; }
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { width: %(w)dpx; height: %(h)dpx; overflow: hidden; }
body { position: relative; font-family: Geist, -apple-system, sans-serif; font-feature-settings: "ss01";
  -webkit-font-smoothing: antialiased; color: %(ink)s; background: linear-gradient(180deg, #ffffff 0%%, #f6f5f4 100%%); }
.field { position: absolute; }
.icon { filter: drop-shadow(0 10px 24px rgba(23, 23, 23, 0.10)) drop-shadow(0 2px 4px rgba(23, 23, 23, 0.06)); }
.eyebrow { font-size: 15px; font-weight: 550; letter-spacing: 0.08em; text-transform: uppercase; color: %(coral)s; }
h1 { font-weight: 650; letter-spacing: -0.03em; line-height: 1.05; }
.sub { color: %(ink2)s; line-height: 1.4; }
.shot { position: absolute; border-radius: 12px; overflow: hidden; display: block;
  box-shadow: 0 0 0 1px rgba(23, 23, 23, 0.08), 0 30px 70px rgba(23, 23, 23, 0.14), 0 8px 20px rgba(23, 23, 23, 0.06); }
.bare { position: absolute; display: block;
  filter: drop-shadow(0 0 0.5px rgba(23, 23, 23, 0.35)) drop-shadow(0 28px 50px rgba(23, 23, 23, 0.14)); }
"""


def render(path, w, h, body, css="", scale=1):
    html = ("<!doctype html><html><head><meta charset='utf-8'><style>"
            + BASE % {"font": FONT, "w": w, "h": h, "ink": INK, "ink2": INK_2, "coral": CORAL}
            + css + "</style></head><body>" + body + "</body></html>")
    page = WORK / (path.stem + ".html")
    page.write_text(html, encoding="utf-8")
    shot = WORK / (path.stem + ".png")
    subprocess.run([CHROME, "--headless=new", "--disable-gpu", "--hide-scrollbars",
                    f"--force-device-scale-factor={scale}", f"--user-data-dir={WORK / 'profile'}",
                    f"--window-size={w},{h}", f"--screenshot={shot}", "--virtual-time-budget=2000",
                    "--allow-file-access-from-files", page.as_uri()], stderr=subprocess.DEVNULL, check=True)
    Image.open(shot).convert("RGB").save(path, optimize=True)  # the stores want no alpha
    print(path.relative_to(ROOT))


def field(x, y, w, h, pitch, seed, fade, clear=40, **density):
    """A chunk field faded in with a CSS mask, e.g. fade='to right', and
    empty for the first clear percent, where the text sits."""
    return (f"<div class='field' style='left:{x}px;top:{y}px;width:{w}px;height:{h}px;"
            f"-webkit-mask-image:linear-gradient({fade}, transparent 0%, transparent {clear}%, #000 90%)'>"
            f"{chunks(w, h, pitch, seed, **density)}</div>")


# --- Promo tiles, banners and social images ---------------------------------

def brand_block(x, y, icon, title, title_size, sub, sub_size, sub_width, eyebrow=None, gap=None):
    gap = gap or round(icon * 0.28)
    eb = f"<div class='eyebrow' style='margin-bottom:{round(title_size * 0.32)}px'>{eyebrow}</div>" if eyebrow else ""
    return (f"<div style='position:absolute;left:{x}px;top:{y}px'>"
            f"<img class='icon' src='{ICON}' style='width:{icon}px;display:block;margin-bottom:{gap}px'>{eb}"
            f"<h1 style='font-size:{title_size}px'>{title}</h1>"
            f"<p class='sub' style='font-size:{sub_size}px;max-width:{sub_width}px;margin-top:{round(title_size * 0.3)}px'>{sub}</p></div>")


def promo_small(path):
    render(path, 440, 280,
           field(0, 0, 440, 280, 22, 4, "to right", clear=62, solid=0.05)
           + brand_block(32, 34, 84, "OpenDM Connect", 30, "Fast, resumable downloads and video in any quality", 15.5, 300))


def promo_marquee(path):
    render(path, 1400, 560,
           field(0, 0, 1400, 560, 34, 9, "to right", clear=36, solid=0.04)
           + brand_block(96, 118, 112, "OpenDM Connect", 56,
                         "Send downloads and videos from the browser to OpenDM, the open-source download manager.", 22, 470,
                         eyebrow="Free and open source")
           + f"<img class='shot' src='{CAP['main']}' style='left:660px;top:84px;width:860px'>")


def social(path, w, h, scale=1):
    """GitHub's social preview and the site's og image."""
    k = h / 640
    render(path, w, h,
           field(0, 0, w, h, round(34 * k), 21, "to right", clear=38, solid=0.04)
           + brand_block(round(80 * k), round(150 * k), round(104 * k), "OpenDM", round(76 * k),
                         "The open-source download manager for macOS and Windows.", round(25 * k), round(430 * k),
                         eyebrow="Free and open source")
           + f"<img class='shot' src='{CAP['main']}' style='left:{round(620 * k)}px;top:{round(118 * k)}px;width:{round(800 * k)}px'>",
           scale=scale)


def banner(path, dark=False):
    """The README header: icon, name and line, chunks off to the right."""
    w, h = 1280, 320
    css = ("body { background: #0f0f10; color: #f2f2f2; } .sub { color: #a1a1a6; }" if dark else "")
    ink = 0.09 if dark else 0.045
    body = (f"<div class='field' style='right:0;top:0;width:700px;height:{h}px;"
            f"-webkit-mask-image:linear-gradient(to right, transparent 0%, transparent 45%, #000 95%)'>"
            + chunks(700, h, 30, 33, solid=0.06, ink=ink).replace(f'fill="{INK}"', 'fill="#ffffff"' if dark else f'fill="{INK}"')
            + "</div>"
            + f"<img class='icon' src='{ICON}' style='position:absolute;left:72px;top:96px;width:128px'>"
            + "<div style='position:absolute;left:236px;top:104px'><h1 style='font-size:64px'>OpenDM</h1>"
            + "<p class='sub' style='font-size:23px;margin-top:14px'>The open-source download manager for macOS and Windows.</p></div>")
    render(path, w, h, body, css, scale=2)


def hero(path, scale):
    """Microsoft Store super hero art: no text or UI, the icon in the middle,
    the bottom third left calm for the Store's own title."""
    w, h = 1920, 1080
    body = ("<div class='field' style='inset:0;-webkit-mask-image:radial-gradient(900px 560px at 50% 36%, #000 0%, "
            "rgba(0,0,0,0.55) 55%, transparent 100%)'>" + chunks(w, h, 48, 57, solid=0.06, tint=0.14) + "</div>"
            + f"<img class='icon' src='{ICON}' style='position:absolute;left:{(w - 300) // 2}px;top:{390 - 150}px;width:300px'>")
    render(path, w, h, body, scale=scale)


# --- Screenshots -------------------------------------------------------------

SHOTS = [
    ("1-video-menu", "Download any video in the quality you want",
     "Hover a video and OpenDM Connect lists every resolution it finds, and the audio on its own.",
     "video-menu", "shot", 1000),
    ("2-download-list", "Browser downloads, in a real download manager",
     "Files you download go to OpenDM: split into parts for speed, resumable, and sorted into folders.",
     "main", "shot", 980),
    ("3-file-info", "Choose the name and folder first",
     "Every new download asks where it should go, and it is already under way while you decide.",
     "fileinfo", "bare", 720),
    ("4-progress", "Watch every part of the file arrive",
     "Eight connections at once, live speed and time left. Pause and resume whenever you like.",
     "progress", "bare", 620),
    ("5-popup", "Everything on the page, one click away",
     "The toolbar button lists the page's videos and audio, and turns capture on or off.",
     "popup", "bare", 380),
]


def screenshot(path, title, sub, cap, kind, width, headline=True):
    """1280x800. With a headline for Chrome and Edge; without for Firefox,
    whose guidelines ask for no text in screenshots."""
    w, h = 1280, 800
    im = Image.open(WORK / f"{cap}.png")
    shown = round(im.height * width / im.width)
    if headline:
        top = 196
        text = (f"<div style='position:absolute;left:0;right:0;top:58px;text-align:center'>"
                f"<h1 style='font-size:46px'>{title}</h1>"
                f"<p class='sub' style='font-size:21px;margin:14px auto 0;max-width:1040px'>{sub}</p></div>")
    else:
        width = min(round(width * 1.2), 1120)
        shown = round(im.height * width / im.width)
        top, text = max(48, (h - shown) // 2), ""
    top = top if shown + top <= h - 40 or kind == "shot" else max(top, h - 40 - shown)
    body = (field(0, 0, w, h, 40, sum(map(ord, cap)), "to bottom", clear=30, solid=0.04, tint=0.08, ink=0.035)
            + text + f"<img class='{kind}' src='{CAP[cap]}' style='left:{(w - width) // 2}px;top:{top}px;width:{width}px'>")
    render(path, w, h, body)


def msix_assets(out):
    """The visual assets an MSIX package's manifest names, at 100, 200 and
    400 percent, plus the unplated app-list icons Windows 11 uses on the
    taskbar and Start. The app ships unpackaged today; these are ready for
    a Store package."""
    scales = (1, 2, 4)
    square = {"Square44x44Logo": 44, "Square71x71Logo": 71, "Square150x150Logo": 150,
              "Square310x310Logo": 310, "StoreLogo": 50}
    for name, base in square.items():
        for k in scales:
            n = base * k
            inset = round(n * (0.1 if base >= 71 else 0.04))
            make_icons.tile(n, inset=inset).save(out / f"{name}.scale-{k * 100}.png")
    for name, (bw, bh) in {"Wide310x150Logo": (310, 150), "SplashScreen": (620, 300)}.items():
        for k in scales:
            canvas = Image.new("RGBA", (bw * k, bh * k), (0, 0, 0, 0))
            side = round(bh * k * 0.6)
            canvas.alpha_composite(make_icons.tile(side), ((bw * k - side) // 2, (bh * k - side) // 2))
            canvas.save(out / f"{name}.scale-{k * 100}.png")
    for n in (16, 20, 24, 30, 32, 36, 40, 48, 60, 64, 72, 80, 96, 256):
        icon = make_icons.tile(n)
        for suffix in ("", "_altform-unplated", "_altform-lightunplated"):
            icon.save(out / f"Square44x44Logo.targetsize-{n}{suffix}.png")


def main():
    chrome, edge, firefox = STORE / "chrome", STORE / "edge", STORE / "firefox"
    microsoft, github = STORE / "microsoft", STORE / "github"
    for d in (chrome, edge, firefox, microsoft, github):
        d.mkdir(parents=True, exist_ok=True)

    for name, title, sub, cap, kind, width in SHOTS:
        screenshot(chrome / f"{name}.png", title, sub, cap, kind, width)
        screenshot(firefox / f"{name}.png", title, sub, cap, kind, width, headline=False)
        shutil.copy(chrome / f"{name}.png", edge / f"{name}.png")

    promo_small(chrome / "promo-small.png")
    promo_marquee(chrome / "promo-marquee.png")
    shutil.copy(chrome / "promo-small.png", edge / "promo-small.png")
    shutil.copy(chrome / "promo-marquee.png", edge / "promo-large.png")

    make_icons.tile(300, inset=30).save(microsoft / "tile-icon-300.png")
    (microsoft / "msix").mkdir(exist_ok=True)
    msix_assets(microsoft / "msix")
    hero(microsoft / "hero-1920x1080.png", 1)
    hero(microsoft / "hero-3840x2160.png", 2)

    social(github / "social-preview.png", 1280, 640)
    banner(github / "banner.png")
    banner(github / "banner-dark.png", dark=True)
    social(ASSETS / "og.png", 1200, 480)

    shutil.rmtree(WORK)


if __name__ == "__main__":
    main()
