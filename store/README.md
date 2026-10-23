# Store listings and GitHub images

`compose.py` renders everything in this folder except the store icons, plus
the site's `og.png`, with headless Google Chrome (Pillow too). It draws from
`docs/icon.svg`, the Geist font in `site/fonts` and the app captures in
`site/assets`; the old icon in those captures is covered with the current one.
The icons themselves come from `scripts/make-icons.py`.

    python3 store/compose.py

| Where | Files | Upload as |
| --- | --- | --- |
| Chrome Web Store | `chrome/store-icon-128.png` | Store icon (128x128, 96 px art) |
| | `chrome/1-…5-*.png` | Screenshots, 1280x800 |
| | `chrome/promo-small.png`, `chrome/promo-marquee.png` | Small promo tile 440x280, marquee 1400x560 |
| Edge Add-ons | `edge/store-icon-300.png` | Extension logo, 300x300 |
| | `edge/1-…5-*.png` | Screenshots, 1280x800 (same as Chrome) |
| | `edge/promo-small.png`, `edge/promo-large.png` | Small 440x280 and large 1400x560 promotional tiles |
| Firefox Add-ons | `firefox/1-…5-*.png` | Screenshots, 1280x800, without headlines as AMO advises. The icon comes from the manifest |
| Microsoft Store | `microsoft/tile-icon-300.png` | 1:1 app tile icon, 300x300 |
| | `microsoft/hero-1920x1080.png`, `microsoft/hero-3840x2160.png` | 16:9 Super hero art: no text, bottom third kept clear |
| | `microsoft/msix/` | Visual assets for an MSIX package manifest, at 100/200/400% and the app-list target sizes. The app ships unpackaged today |
| GitHub | `github/social-preview.png` | Settings → Social preview, 1280x640 |
| | `github/banner.png`, `github/banner-dark.png` | README header for light and dark themes, 2560x640 |

The Microsoft Store also needs desktop screenshots (1366x768 or larger) of
the Windows app, without marketing text. Take those on Windows; the captures
here are of the macOS app.

The video frame in the captures is from Sintel, Blender Foundation, CC BY 3.0.
