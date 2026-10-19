
"""Writes the Chrome Web Store screenshots and promo tiles as HTML, then
renders them with headless Chrome at the exact sizes the store asks for."""
import re, subprocess
from pathlib import Path

OUT = Path("/tmp/opendm-store/out"); OUT.mkdir(exist_ok=True)
SRC = Path("/tmp/opendm-store")
CHROME = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
content_js = Path("/Users/eniac/Developer/download-manager/extension/content.js").read_text()
MENU_CSS = re.search(r"const STYLE = \x60([\s\S]*?)\x60;", content_js).group(1)

BASE = """
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { width: %(w)dpx; height: %(h)dpx; overflow: hidden; }
body { font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "Helvetica Neue", sans-serif;
  -webkit-font-smoothing: antialiased; color: #0f1222; }
.bg { position: absolute; inset: 0;
  background: radial-gradient(1100px 600px at 85%% 110%%, rgba(47,109,246,0.20), transparent 60%%),
              radial-gradient(900px 500px at 0%% -10%%, rgba(120,160,255,0.22), transparent 60%%),
              linear-gradient(180deg, #f7f9fd 0%%, #eaf0fa 100%%); }
.head { position: absolute; left: 0; right: 0; top: 54px; text-align: center; }
h1 { font-size: 46px; line-height: 1.12; font-weight: 700; letter-spacing: -0.02em; color: #0f1222; }
.sub { margin: 14px auto 0; max-width: 880px; font-size: 21px; line-height: 1.4; color: #4a5068; font-weight: 400; }
.shot { position: absolute; }
.shot img { display: block; width: 100%%; }
"""

def page(name, w, h, css, body):
    html = "<!doctype html><html><head><meta charset='utf-8'><style>" + (BASE % {"w": w, "h": h}) + css + "</style></head><body>" + body + "</body></html>"
    path = OUT / (name + ".html")
    path.write_text(html)
    subprocess.run([CHROME, "--headless=new", "--disable-gpu", "--hide-scrollbars", "--force-device-scale-factor=1",
                    f"--user-data-dir=/tmp/opendm-store/prof", f"--window-size={w},{h}", f"--screenshot={OUT / (name + '.png')}",
                    "--virtual-time-budget=1500", "--allow-file-access-from-files", path.as_uri()],
                   stderr=subprocess.DEVNULL, check=True)

def head(title, sub):
    return f"<div class='bg'></div><div class='head'><h1>{title}</h1><p class='sub'>{sub}</p></div>"

# A light Chrome window drawn in CSS, with a page area for content.
BROWSER_CSS = """
.win { position: absolute; border-radius: 12px; overflow: hidden; background: #fff;
  box-shadow: 0 0 0 1px rgba(15,18,34,0.10), 0 30px 70px rgba(25,40,90,0.22), 0 8px 20px rgba(25,40,90,0.10); }
.tabs { height: 40px; background: #e8ebf1; display: flex; align-items: flex-end; padding-left: 86px; position: relative; }
.lights { position: absolute; left: 16px; top: 14px; display: flex; gap: 8px; }
.lights i { width: 12px; height: 12px; border-radius: 50%; display: block; }
.tab { height: 32px; width: 250px; background: #fff; border-radius: 9px 9px 0 0; display: flex; align-items: center;
  gap: 8px; padding: 0 12px; font-size: 12.5px; color: #1f2333; }
.fav { width: 16px; height: 16px; border-radius: 4px; background: linear-gradient(135deg,#ff9a3c,#e8590c); }
.bar { height: 44px; background: #fff; display: flex; align-items: center; gap: 12px; padding: 0 14px;
  border-bottom: 1px solid #e3e6ec; }
.nav { display: flex; gap: 14px; color: #6b7080; font-size: 15px; }
.url { flex: 1; height: 30px; border-radius: 15px; background: #f0f2f6; display: flex; align-items: center;
  padding: 0 14px; font-size: 13.5px; color: #30344a; gap: 8px; }
.url b { font-weight: 500; color: #111; }
.ext { width: 28px; height: 28px; border-radius: 8px; display: grid; place-items: center; }
.ext.on { background: #e6eefc; }
.ext img { width: 18px; height: 18px; }
.page { position: relative; background: #0d0f14; }
"""

def browser(x, y, w, h, title, host, path, page_html, ext_on=False):
    return f"""<div class='win' style='left:{x}px;top:{y}px;width:{w}px;height:{h}px'>
  <div class='tabs'><div class='lights'><i style='background:#ff5f57'></i><i style='background:#febc2e'></i><i style='background:#28c840'></i></div>
    <div class='tab'><span class='fav'></span>{title}</div></div>
  <div class='bar'><div class='nav'>&#8592; &#8594; &#8635;</div>
    <div class='url'><svg width='12' height='13' viewBox='0 0 12 13'><rect x='1.5' y='5.5' width='9' height='7' rx='1.6' fill='#6b7080'/><path d='M3.6 5.6V4a2.4 2.4 0 0 1 4.8 0v1.6' fill='none' stroke='#6b7080' stroke-width='1.4'/></svg> <span><b>{host}</b>{path}</span></div>
    <div class='ext {"on" if ext_on else ""}'><img src='file:///Users/eniac/Developer/download-manager/extension/icons/icon-32.png'></div></div>
  <div class='page' style='height:{h - 84}px'>{page_html}</div></div>"""

FRAME = (SRC / "frame-33.png").as_uri()
VIDEO_PAGE = """
.site { position: absolute; inset: 0; background: #0d0f14; color: #e9ecf3; }
.player { position: absolute; left: 40px; top: 28px; right: 40px; aspect-ratio: 1920/818; border-radius: 10px; overflow: hidden;
  background: url(%s) center/cover; }
.controls { position: absolute; left: 0; right: 0; bottom: 0; height: 64px;
  background: linear-gradient(transparent, rgba(0,0,0,0.65)); }
.scrub { position: absolute; left: 18px; right: 18px; bottom: 40px; height: 4px; border-radius: 2px; background: rgba(255,255,255,0.3); }
.scrub i { position: absolute; left: 0; top: 0; bottom: 0; width: 18%%; background: #ff3b30; border-radius: 2px; }
.ctl { position: absolute; left: 18px; bottom: 12px; font-size: 13px; color: #fff; opacity: 0.9; }
.meta { position: absolute; left: 40px; right: 40px; font-size: 20px; font-weight: 600; }
.meta small { display: block; margin-top: 6px; font-size: 14px; font-weight: 400; color: #9aa1b5; }
""" % FRAME

def video_page(meta_top):
    return f"""<div class='site'><div class='player'><div class='controls'><div class='scrub'><i></i></div>
      <div class='ctl'>&#9654;&nbsp;&nbsp; 2:41 / 14:48</div></div>%s</div>
      <div class='meta' style='top:{meta_top}px'>Sintel — Blender Open Movie<small>Blender Foundation · 14:48 · CC BY 3.0</small></div></div>"""

OPTIONS = [("Video", [("2160p", "VP9 · ≈ 3.92 GB"), ("1440p", "VP9 · ≈ 1.96 GB"), ("1080p", "H.264 · ≈ 1.18 GB"),
                      ("720p", "H.264 · ≈ 541.3 MB"), ("480p", "H.264 · ≈ 262.8 MB")]),
           ("Audio only", [("English", "Original · 128 kbps · AAC · ≈ 14.2 MB")])]

def menu_html(hover="1080p"):
    items = "".join("<div class='group'>" + t + "</div>" + "".join(
        f"<button class='option{' hover' if l == hover else ''}'><span class='label'>{l}</span><span class='detail'>{d}</span></button>" for l, d in opts)
        for t, opts in OPTIONS)
    css = MENU_CSS.replace(".option:hover,", ".option.hover, .option:hover,").replace("animation: open 0.14s ease-out;", "")
    return f"""<div class='menuhost'><template shadowrootmode='open'><style>{css}</style>
      <div class='row'><button class='main shown open'><svg viewBox='0 0 16 16'><path d='M8 2v8M4.5 6.8 8 10.3l3.5-3.5M3 13.5h10'/></svg><span>Download video</span></button>
      <div class='menu options'>{items}</div></div></template></div>"""

# 1. The video button and its quality menu.
player_menu = video_page(432).replace("%s", menu_html())
page("1-video-menu", 1280, 800, BROWSER_CSS + VIDEO_PAGE + ".menuhost { position: absolute; top: 12px; right: 12px; } .menuhost .menu { animation: none; }",
     head("Download any video in the quality you want", "Hover a video and OpenDM Connect lists every resolution it finds, and the audio on its own.")
     + browser(150, 205, 980, 640, "Sintel — Blender Open Movie", "durian.blender.org", "/sintel", player_menu))

# 2. The app's download list.
page("2-download-list", 1280, 800, ".shot { left: 160px; top: 186px; width: 960px; }",
     head("Browser downloads, in a real download manager", "Files you download go to OpenDM: split into parts for speed, resumable, and sorted into folders.")
     + f"<div class='shot'><img src='{(SRC / 'main.png').as_uri()}'></div>")

# 3. Download File Info over the video page.
dialog_css = BROWSER_CSS + VIDEO_PAGE + ".shot { left: 312px; top: 300px; width: 656px; } .dim { position: absolute; inset: 0; background: rgba(8,10,16,0.35); }"
page("3-file-info", 1280, 800, dialog_css,
     head("Choose the name and folder first", "Every new download asks where it should go, and it is already under way while you decide.")
     + browser(150, 205, 980, 640, "Sintel — Blender Open Movie", "durian.blender.org", "/download", video_page(432).replace("%s", "") + "<div class='dim'></div>")
     + f"<div class='shot'><img src='{(SRC / 'fileinfo.png').as_uri()}'></div>")

# 4. The progress window.
page("4-progress", 1280, 800, ".shot { left: 290px; top: 190px; width: 700px; }",
     head("Watch every part of the file arrive", "Eight connections at once, live speed and time left. Pause and resume whenever you like.")
     + f"<div class='shot'><img src='{(SRC / 'progress.png').as_uri()}'></div>")

# 5. The toolbar popup.
popup_css = BROWSER_CSS + VIDEO_PAGE + """
.pop { position: absolute; right: 168px; top: 270px; width: 340px; height: 492px; border-radius: 12px; overflow: hidden; background: #fff;
  box-shadow: 0 0 0 1px rgba(15,18,34,0.12), 0 24px 60px rgba(15,25,60,0.30); }
.pop img { width: 340px; display: block; }
"""
page("5-popup", 1280, 800, popup_css,
     head("Everything on the page, one click away", "The toolbar button lists the page's videos and audio, and turns capture on or off.")
     + browser(150, 205, 980, 640, "Sintel — Blender Open Movie", "durian.blender.org", "/sintel", video_page(432).replace("%s", ""), ext_on=True)
     + f"<div class='pop'><img src='{(SRC / 'popup.png').as_uri()}'></div>")

# Promo tiles.
ICON = (SRC / "store-icon-512.png").as_uri()
PROMO = """
.dark { position: absolute; inset: 0; background: radial-gradient(700px 400px at 90% 120%, rgba(70,130,255,0.55), transparent 65%),
  linear-gradient(135deg, #0b1020 0%, #14204a 60%, #1d3a8f 100%); color: #fff; }
"""
page("promo-small", 440, 280, PROMO + """
.ic { position: absolute; left: 32px; top: 46px; width: 104px; }
.t { position: absolute; left: 32px; top: 160px; font-size: 30px; font-weight: 700; letter-spacing: -0.02em; color: #fff; }
.s { position: absolute; left: 32px; top: 202px; right: 30px; font-size: 15.5px; line-height: 1.35; color: #c3cbe6; }
""", f"<div class='dark'></div><img class='ic' src='{ICON}'><div class='t'>OpenDM Connect</div><div class='s'>Fast, resumable downloads and video in any quality</div>")

page("promo-marquee", 1400, 560, PROMO + """
.ic { position: absolute; left: 90px; top: 120px; width: 120px; }
.t { position: absolute; left: 90px; top: 268px; font-size: 54px; font-weight: 700; letter-spacing: -0.025em; color: #fff; }
.s { position: absolute; left: 92px; top: 345px; width: 470px; font-size: 22px; line-height: 1.4; color: #c3cbe6; }
.app { position: absolute; left: 640px; top: 70px; width: 860px; border-radius: 14px; }
""", f"<div class='dark'></div><img class='ic' src='{ICON}'><div class='t'>OpenDM Connect</div><div class='s'>Send downloads and videos from the browser to OpenDM, the open-source download manager.</div><img class='app' src='{(SRC / 'main.png').as_uri()}'>")
print("rendered", sorted(p.name for p in OUT.glob("*.png")))

