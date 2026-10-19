#!/usr/bin/env python3
"""Packages OpenDM Connect for the browser stores as
build/extension/opendm-connect-<version>.zip, the same file for the Chrome Web
Store, Edge Add-ons and addons.mozilla.org.

The Chromium stores assign their own extension IDs, so the package drops the
manifest's "key", which only fixes the ID of the unpacked development copy.
Once published, add the store IDs to CHROMIUM_EXTENSION_IDS in host/src/lib.rs
so the native host lets them in. Firefox keeps its ID from
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

# Only what the browser loads: no README or other notes.
files = sorted(
    path for path in source.rglob("*")
    if path.is_file() and path.name != "manifest.json" and path.suffix in {".js", ".html", ".css", ".png"}
)
target = out / f"opendm-connect-{manifest['version']}.zip"
with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as archive:
    archive.writestr("manifest.json", json.dumps(manifest, indent=2) + "\n")
    for path in files:
        archive.write(path, path.relative_to(source).as_posix())
print(target)

