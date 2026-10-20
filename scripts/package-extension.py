#!/usr/bin/env python3
"""Packages OpenDM Connect for the browser stores:

- build/extension/opendm-connect-<version>.zip for Edge Add-ons,
- build/extension/opendm-connect-<version>-firefox.zip for addons.mozilla.org,
  without the service worker key Firefox ignores, and
- build/extension/opendm-connect-<version>-chrome.zip for the Chrome Web
  Store, which leaves videos on YouTube alone, as that store's policy asks.

Both drop the manifest's "key", which only fixes the ID of the unpacked
development copy; the stores assign their own IDs, which go into
CHROMIUM_EXTENSION_IDS in host/src/lib.rs. Firefox keeps its ID from
browser_specific_settings.
"""
import json
import zipfile
from pathlib import Path

root = Path(__file__).resolve().parent.parent
source = root / "extension"
manifest = json.loads((source / "manifest.json").read_text(encoding="utf-8"))
manifest.pop("key", None)
out = root / "build" / "extension"
out.mkdir(parents=True, exist_ok=True)

# The line both scripts carry, and what the Chrome Web Store copy puts there.
BLOCKED_LINE = "const MEDIA_BLOCKED_HOSTS = [];"
CHROME_BLOCKED = 'const MEDIA_BLOCKED_HOSTS = ["youtube.com", "youtube-nocookie.com", "youtu.be", "googlevideo.com"];'

# Only what the browser loads: no README or other notes.
files = sorted(
    path for path in source.rglob("*")
    if path.is_file() and path.name != "manifest.json" and path.suffix in {".js", ".html", ".css", ".png"}
)


def package(target, blocked=None, firefox=False):
    own = json.loads(json.dumps(manifest))
    # Each family reads its own background entry; Edge refuses a package
    # that carries Firefox's as well.
    own["background"].pop("service_worker" if firefox else "scripts", None)
    with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr("manifest.json", json.dumps(own, indent=2) + "\n")
        for path in files:
            name = path.relative_to(source).as_posix()
            data = path.read_bytes()
            if blocked and BLOCKED_LINE.encode() in data:
                data = data.replace(BLOCKED_LINE.encode(), blocked.encode())
            archive.writestr(name, data)
    print(target)


for name in ("background.js", "content.js"):
    assert (source / name).read_text(encoding="utf-8").count(BLOCKED_LINE) == 1, f"{name} lacks {BLOCKED_LINE}"
package(out / f"opendm-connect-{manifest['version']}.zip")
package(out / f"opendm-connect-{manifest['version']}-chrome.zip", CHROME_BLOCKED)
package(out / f"opendm-connect-{manifest['version']}-firefox.zip", firefox=True)
