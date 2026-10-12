# Tinline logo assets (mark D)

Everything here is generated. Do not edit by hand; edit `../playground/src/lib/brand/tiers.js` (tier table,
palettes) or `tinmark.js` (model) and run:

    node docs/design/brand/generate.mjs      # ~13 s; needs Playwright + /usr/sbin/chromium (CHROMIUM_PATH, PLAYWRIGHT_PATH override)

No npm dependencies were added. ICO / ICNS / alpha-stripped PNGs are written by small functions in generate.mjs.
Open `index.html` for the contact sheet (every tier zoomed, launcher masks, status bar, taskbar, tab).

| path | what | where it goes later |
|---|---|---|
| `svg/tinline-<px>-<color|dark|mono|white>.svg` | bare mark, per tier (12..1024); `-taut` for 16/20/24/28; `-tile` = app tile | docs, site, in-app art |
| `svg/tiers.json` | the tier parameter table | reference |
| `android/ic_launcher_{foreground,background,monochrome}.xml` | adaptive layers, 108 dp, glyph 60 dp inside the 66 dp safe zone | android/app/src/main/res/drawable/ |
| `android/mipmap-anydpi/ic_launcher{,_round}.xml` | adaptive icon, incl. themed monochrome | res/mipmap-anydpi/ (or -v26) |
| `android/mipmap-{m,h,xh,xxh,xxxh}dpi/ic_launcher{,_round}.png` | legacy 48/72/96/144/192 | res/mipmap-*dpi/ |
| `android/ic_stat_tinline.xml` | status-bar icon, 24 dp, white, hinted 20 px tier | res/drawable/, setSmallIcon |
| `android/play-store-icon-512.png` | 512x512 RGB, no alpha, square (Play rounds it) | Play Console |
| `desktop/png/tinline-<px>.png` | tile PNGs 16..1024, each from its own tier | window icon, packaging |
| `desktop/tinline.ico` | 16,20,24,32,40,48,64,256 (PNG-in-ICO) | Windows build |
| `desktop/tinline.icns` | 16..1024 incl. @2x | macOS bundle |
| `desktop/hicolor/<n>x<n>/apps/tinline.png`, `scalable/apps/tinline.svg` | freedesktop theme tree | Linux packaging |
| `desktop/tray/` | 16/22/24/32 white and black PNGs, `tinline-symbolic.svg` | tray / status notifier |
| `web/` | favicon.svg (dark-mode aware), favicon-16/32, apple-touch-icon 180, 192/512, maskable 512, manifest | site |

Notes: no Play feature graphic is produced. Small sizes are rendered from their own tier SVG at exact pixel size,
never downscaled. Only the slack (rest) pose is used for app icons.
