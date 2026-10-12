// Regenerates every Tinline logo file in this folder from the parametric model.
//   node docs/design/brand/generate.mjs
// Source of truth: ../playground/src/lib/brand/{tinmark.js,tiers.js}. Nothing here is hand-drawn.
// Needs Playwright's Chromium (PLAYWRIGHT_PATH, CHROMIUM_PATH override the defaults used by refined/qa/lib.js).
// No other dependencies: ICO / ICNS / PNG-alpha-strip are written below with node:zlib + Buffer.
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const BR = path.join(HERE, '../playground/src/lib/brand/');
const { TinMark } = await import(BR + 'tinmark.js');
const T = await import(BR + 'tiers.js');
const { PAL, SMALL, TIER_SIZES, tierPx, geometry, markSvg, tileSvg, paddedSvg, tileBox } = T;

const require = createRequire(import.meta.url);
const { chromium } = require(process.env.PLAYWRIGHT_PATH || '/home/abe/project/sveditorjs/node_modules/playwright');

// ---- output helpers -----------------------------------------------------------------------------------------
const out = (rel) => path.join(HERE, rel);
// start clean (only the generated subfolders + index.html; never README.md / generate.mjs)
for (const d of ['svg', 'android', 'desktop', 'web']) fs.rmSync(out(d), { recursive: true, force: true });
const written = [];
function put(rel, data) { const p = out(rel); fs.mkdirSync(path.dirname(p), { recursive: true }); fs.writeFileSync(p, data); written.push(rel); }
const f = (v) => { const r = Math.round(v * 1000) / 1000; return String(r === 0 ? 0 : r); };
const svgFile = (s) => s + '\n';

// ---- rasteriser ---------------------------------------------------------------------------------------------
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM_PATH || '/usr/sbin/chromium', args: ['--no-sandbox'] });
const ctx = await browser.newContext({ deviceScaleFactor: 1, viewport: { width: 1100, height: 1100 } });
const pg = await ctx.newPage();
/** Render an svg string (its own width/height = px) to a PNG Buffer at exactly that size. Transparent background. */
async function ras(svg, W, H = W) {
  await pg.setContent(`<!doctype html><body style="margin:0;background:transparent">${svg}</body>`);
  return pg.screenshot({ clip: { x: 0, y: 0, width: W, height: H }, omitBackground: true });
}

// ---- PNG: strip the alpha channel (Play Store / apple-touch want opaque images) ------------------------------
const crcT = (b) => zlib.crc32(b);
function chunk(type, data) { const t = Buffer.from(type), l = Buffer.alloc(4), c = Buffer.alloc(4); l.writeUInt32BE(data.length); c.writeUInt32BE(crcT(Buffer.concat([t, data])) >>> 0); return Buffer.concat([l, t, data, c]); }
function stripAlpha(png) {
  let o = 8, ihdr, idat = [];
  while (o < png.length) { const n = png.readUInt32BE(o), ty = png.toString('latin1', o + 4, o + 8), d = png.subarray(o + 8, o + 8 + n); if (ty === 'IHDR') ihdr = d; if (ty === 'IDAT') idat.push(d); o += 12 + n; }
  const w = ihdr.readUInt32BE(0), h = ihdr.readUInt32BE(4);
  if (ihdr[8] === 8 && ihdr[9] === 2 && ihdr[12] === 0) return png;   // Chromium already wrote RGB for opaque pages
  if (ihdr[8] !== 8 || ihdr[9] !== 6 || ihdr[12] !== 0) throw new Error('stripAlpha: expected 8-bit RGBA, non-interlaced');
  const raw = zlib.inflateSync(Buffer.concat(idat)), bpp = 4, st = w * bpp, cur = Buffer.alloc(st), prev = Buffer.alloc(st);
  const rgb = Buffer.alloc(h * (1 + w * 3));
  for (let y = 0; y < h; y++) {
    const ft = raw[y * (st + 1)], line = raw.subarray(y * (st + 1) + 1, (y + 1) * (st + 1));
    for (let i = 0; i < st; i++) {
      const a = i >= bpp ? cur[i - bpp] : 0, b = prev[i], c = i >= bpp ? prev[i - bpp] : 0; let v = line[i];
      if (ft === 1) v += a; else if (ft === 2) v += b; else if (ft === 3) v += (a + b) >> 1;
      else if (ft === 4) { const p = a + b - c, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - c); v += (pa <= pb && pa <= pc) ? a : (pb <= pc ? b : c); }
      cur[i] = v & 255;
    }
    const ro = y * (1 + w * 3); rgb[ro] = 0;
    for (let x = 0; x < w; x++) { if (cur[x * 4 + 3] !== 255) throw new Error('stripAlpha: image is not fully opaque'); rgb[ro + 1 + x * 3] = cur[x * 4]; rgb[ro + 2 + x * 3] = cur[x * 4 + 1]; rgb[ro + 3 + x * 3] = cur[x * 4 + 2]; }
    cur.copy(prev);
  }
  const hd = Buffer.alloc(13); hd.writeUInt32BE(w, 0); hd.writeUInt32BE(h, 4); hd[8] = 8; hd[9] = 2;
  return Buffer.concat([png.subarray(0, 8), chunk('IHDR', hd), chunk('IDAT', zlib.deflateSync(rgb, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}

// ---- containers ---------------------------------------------------------------------------------------------
function ico(entries) { // entries: [{size, png}] (PNG-in-ICO, Vista+)
  const hd = Buffer.alloc(6); hd.writeUInt16LE(1, 2); hd.writeUInt16LE(entries.length, 4);
  let off = 6 + 16 * entries.length; const dir = [], data = [];
  for (const e of entries) {
    const d = Buffer.alloc(16); d[0] = e.size >= 256 ? 0 : e.size; d[1] = e.size >= 256 ? 0 : e.size; d.writeUInt16LE(1, 4); d.writeUInt16LE(32, 6);
    d.writeUInt32LE(e.png.length, 8); d.writeUInt32LE(off, 12); off += e.png.length; dir.push(d); data.push(e.png);
  }
  return Buffer.concat([hd, ...dir, ...data]);
}
function icns(entries) { // entries: [{type, png}]
  const body = Buffer.concat(entries.map((e) => { const h = Buffer.alloc(8); h.write(e.type, 0, 'latin1'); h.writeUInt32BE(8 + e.png.length, 4); return Buffer.concat([h, e.png]); }));
  const h = Buffer.alloc(8); h.write('icns', 0, 'latin1'); h.writeUInt32BE(8 + body.length, 4); return Buffer.concat([h, body]);
}

// ---- tier table (also written to svg/tiers.json) ------------------------------------------------------------
const tierTable = TIER_SIZES.map((B) => {
  const t = tierPx(B), g = geometry(B), { M } = g;
  return { size: B, designed: !!SMALL[B], snap: B < 32 ? 'px-designed (1 model unit = 1 px)' : (B < 96 ? 'b whole px, rest 0.5 px' : 'exact'),
    c: +t.c.toFixed(3), a: +t.a.toFixed(3), b: +t.b.toFixed(3), L: +t.L.toFixed(3), mu: t.mu, noCap: !!t.noCap, stroke: t.sw, sag: +t.s.toFixed(3),
    tautSag: t.sT, tautMode: t.sT != null ? 'sag only' : 'cans pull in + straight', viewBox: M.vb.map(f).join(' ') };
});
put('svg/tiers.json', JSON.stringify(tierTable, null, 1) + '\n');

// ---- 1. svg/ ---------------------------------------------------------------------------------------------------
const VARS = ['color', 'dark', 'mono', 'white'];
for (const B of TIER_SIZES) {
  for (const v of VARS) put(`svg/tinline-${B}-${v}.svg`, svgFile(markSvg(B, v)));
  if (B >= 16) put(`svg/tinline-${B}-tile.svg`, svgFile(tileSvg(B)));
}
const TAUT = [16, 20, 24, 28];
for (const B of TAUT) for (const v of VARS) put(`svg/tinline-${B}-${v}-taut.svg`, svgFile(markSvg(B, v, 'taut')));
for (const B of [96, 128, 256, 512, 1024]) put(`svg/tinline-${B}-tile.svg`, svgFile(tileSvg(B)));

// ---- 2. android/ -----------------------------------------------------------------------------------------------
const XH = '<?xml version="1.0" encoding="utf-8"?>\n';
const VD = (extra = '') => `<vector xmlns:android="http://schemas.android.com/apk/res/android"\n    android:width="${extra.w}dp" android:height="${extra.w}dp" android:viewportWidth="${extra.w}" android:viewportHeight="${extra.w}">`;
const G1024 = geometry(1024);                              // exact (unsnapped) base geometry in model units
const bboxW = G1024.d.bbox.w;
const GLYPH_W = 60, K = +(GLYPH_W / bboxW).toFixed(4), TXY = +(54 - 12 * K).toFixed(3);
/** vector-drawable body for the left/right cans + string. g = geometry(), colours from a VARIANT. */
function vdBody(g, V, ind) {
  const d = g.d, sw = f(g.M.p.strokeW), I = ' '.repeat(ind);
  const canXml = (i) => V.mono
    ? `${i}<path android:fillColor="${V.ink}" android:fillType="evenOdd" android:pathData="${d.mono}" />\n`
    : `${i}<path android:fillColor="${V.can}" android:pathData="${d.canLeft}" />\n${i}<path android:fillColor="${V.rim}" android:pathData="${d.rim}" />\n${i}<path android:fillColor="${V.mouth}" android:pathData="${d.mouth}" />\n`;
  return canXml(I) + `${I}<group android:translateX="24" android:scaleX="-1">\n${canXml(I + '    ')}${I}</group>\n` +
    `${I}<path android:strokeColor="${V.mono ? V.ink : V.str}" android:strokeWidth="${sw}" android:strokeLineCap="round" android:pathData="${d.string}" />\n`;
}
const launcherGroup = (V) => `    <group android:scaleX="${K}" android:scaleY="${K}" android:translateX="${TXY}" android:translateY="${TXY}">\n${vdBody(G1024, V, 8)}    </group>\n`;
put('android/ic_launcher_foreground.xml', XH + `<!-- Tinline mark D, generated by docs/design/brand/generate.mjs from the parametric model. Glyph ${GLYPH_W}dp wide, centred, inside the 66dp safe zone. -->\n` + VD({ w: 108 }) + '\n' + launcherGroup(T.VARIANTS.tile) + '</vector>\n');
put('android/ic_launcher_monochrome.xml', XH + `<!-- Single ink, mouth is a real evenodd cutout (Android 13 themed icon). -->\n` + VD({ w: 108 }) + '\n' + launcherGroup({ mono: true, ink: '#FFFFFF' }) + '</vector>\n');
put('android/ic_launcher_background.xml', XH + VD({ w: 108 }) + `\n    <path android:fillColor="${PAL.teal}" android:pathData="M0,0h108v108h-108z" />\n</vector>\n`);
const adaptive = XH + `<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">\n    <background android:drawable="@drawable/ic_launcher_background" />\n    <foreground android:drawable="@drawable/ic_launcher_foreground" />\n    <monochrome android:drawable="@drawable/ic_launcher_monochrome" />\n</adaptive-icon>\n`;
put('android/mipmap-anydpi/ic_launcher.xml', adaptive);
put('android/mipmap-anydpi/ic_launcher_round.xml', adaptive);
// notification small icon: 24 dp canvas, the hinted 20-px tier centred (2 dp padding), white ink with mouth cutout
{
  const g = geometry(20);
  // tier 20's model crop starts at 12 - 10 = 2, so model coords + (-2) crop + 2 padding = model coords: no transform needed.
  put('android/ic_stat_tinline.xml', XH + '<!-- Status-bar icon: 24dp canvas, 20dp hinted tier (pixel-aligned at mdpi), alpha only. Android tints it. -->\n' + VD({ w: 24 }) + '\n' + vdBody(g, { mono: true, ink: '#FFFFFF' }, 4) + '</vector>\n');
}
const DENS = { mdpi: 48, hdpi: 72, xhdpi: 96, xxhdpi: 144, xxxhdpi: 192 };
const legacyBox = (S, shape) => Math.round(S * (shape === 'circle' ? 0.62 : 0.68) / 2) * 2;
for (const [d, S] of Object.entries(DENS)) {
  put(`android/mipmap-${d}/ic_launcher.png`, await ras(tileSvg(S, { shape: 'rounded', B: legacyBox(S, 'rounded') }), S));
  put(`android/mipmap-${d}/ic_launcher_round.png`, await ras(tileSvg(S, { shape: 'circle', B: legacyBox(S, 'circle') }), S));
}
const PLAY_B = 332;   // glyph width = 0.858 * 332 = 285 px = 0.556 of 512, same ratio as the 60/108 adaptive glyph
put('android/play-store-icon-512.png', stripAlpha(await ras(tileSvg(512, { shape: 'square', B: PLAY_B }), 512)));

// ---- 3. desktop/ -----------------------------------------------------------------------------------------------
const DESK = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256, 512, 1024];
const tilePng = {};
for (const S of [...new Set([...DESK, 22, 28])]) tilePng[S] = await ras(tileSvg(S), S);
for (const S of DESK) put(`desktop/png/tinline-${S}.png`, tilePng[S]);
put('desktop/tinline.ico', ico([16, 20, 24, 32, 40, 48, 64, 256].map((S) => ({ size: S, png: tilePng[S] }))));
{
  // @2x entries are the 1x tier's own artwork rendered at twice the pixels (still pixel-aligned: every tier coordinate is a whole/half px)
  const at2 = async (S) => ras(tileSvg(S).replace(/<svg ([^>]*?)width="(\d+)" height="(\d+)"/, (m, a) => `<svg ${a}width="${S * 2}" height="${S * 2}"`), S * 2);
  put('desktop/tinline.icns', icns([
    { type: 'icp4', png: tilePng[16] }, { type: 'icp5', png: tilePng[32] }, { type: 'icp6', png: tilePng[64] },
    { type: 'ic07', png: tilePng[128] }, { type: 'ic08', png: tilePng[256] }, { type: 'ic09', png: tilePng[512] }, { type: 'ic10', png: tilePng[1024] },
    { type: 'ic11', png: await at2(16) }, { type: 'ic12', png: await at2(32) }, { type: 'ic13', png: await at2(128) }, { type: 'ic14', png: await at2(256) },
  ]));
}
for (const S of [16, 22, 24, 32, 48, 64, 128, 256, 512]) put(`desktop/hicolor/${S}x${S}/apps/tinline.png`, tilePng[S] ?? await ras(tileSvg(S), S));
put('desktop/hicolor/scalable/apps/tinline.svg', svgFile(tileSvg(512)));
const trayArt = (S, v) => (S === 22 ? paddedSvg(22, 20, v) : markSvg(S, v));
for (const S of [16, 22, 24, 32]) {
  put(`desktop/tray/tinline-tray-${S}-white.png`, await ras(trayArt(S, 'monowhite'), S));
  put(`desktop/tray/tinline-tray-${S}-black.png`, await ras(trayArt(S, 'mono'), S));
}
put('desktop/tray/tinline-symbolic.svg', svgFile(markSvg(16, 'mono').replace(new RegExp(T.VARIANTS.mono.ink, 'g'), '#bebebe')));
put('desktop/tray/tinline-tray-white.svg', svgFile(markSvg(16, 'monowhite')));
put('desktop/tray/tinline-tray-black.svg', svgFile(markSvg(16, 'mono')));

// ---- 4. web/ ---------------------------------------------------------------------------------------------------
{
  const g = geometry(16), { M } = g, d = g.d, sw = f(M.p.strokeW);
  const MIR = 'translate(24 0) scale(-1 1)';
  const can = (tf) => `<g${tf}><path class="can" d="${d.canLeft}"/><path class="can" d="${d.rim}"/><path class="mouth" d="${d.mouth}"/></g>`;
  put('web/favicon.svg', svgFile(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="${M.vb.map(f).join(' ')}" width="16" height="16">` +
    `<style>.can{fill:${PAL.teal}}.mouth{fill:${PAL.mint}}.str{fill:none;stroke:${PAL.gold};stroke-width:${sw};stroke-linecap:round}` +
    `@media(prefers-color-scheme:dark){.can{fill:${PAL.cream}}.mouth{fill:${PAL.deep}}.str{stroke:${PAL.amber}}}</style>` +
    `${can('')}${can(` transform="${MIR}"`)}<path class="str" d="${d.string}"/></svg>`));
}
put('web/favicon-16.png', tilePng[16]);
put('web/favicon-32.png', tilePng[32]);
put('web/apple-touch-icon.png', stripAlpha(await ras(tileSvg(180, { shape: 'square', B: 126 }), 180)));
put('web/icon-192.png', await ras(tileSvg(192, { B: 134 }), 192));
put('web/icon-512.png', await ras(tileSvg(512, { B: 384 }), 512));
put('web/icon-maskable-512.png', stripAlpha(await ras(tileSvg(512, { shape: 'square', B: PLAY_B }), 512)));
put('web/site.webmanifest', JSON.stringify({ name: 'Tinline', short_name: 'Tinline', theme_color: PAL.teal, background_color: PAL.teal, display: 'standalone',
  icons: [{ src: 'icon-192.png', sizes: '192x192', type: 'image/png', purpose: 'any' }, { src: 'icon-512.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
    { src: 'icon-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' }] }, null, 2) + '\n');

// ---- 5. contact sheet ------------------------------------------------------------------------------------------
const b64 = (b) => 'data:image/png;base64,' + b.toString('base64');
async function zoomImg(svg, B, z, bg) { const p = await ras(svg, B); return `<img class="px" src="${b64(p)}" width="${B * z}" height="${B * z}" alt="">`; }
const surf = { light: '#ffffff', dark: '#161d1b', grey: '#8b918e' };
const varOn = { light: 'color', dark: 'dark', grey: 'color' };
let rows = '';
async function tierRow(B, state = 'slack') {
  const t = tierPx(B), z = B <= 16 ? 10 : B <= 32 ? 6 : B <= 64 ? 3 : B <= 96 ? 2 : 0, cells = [];
  const shown = Math.min(B, 128);
  for (const k of ['light', 'dark', 'grey']) cells.push(`<td class="s ${k}">${markSvg(B, varOn[k], state).replace(`width="${B}" height="${B}"`, `width="${shown}" height="${shown}"`)}</td>`);
  for (const k of ['light', 'dark', 'grey']) cells.push(`<td class="s ${k}">${z ? await zoomImg(markSvg(B, varOn[k], state), B, z) : '<span class="na">1× above</span>'}</td>`);
  const note = `${B} px${state === 'taut' ? ' taut' : ''}<br><small>c ${f(t.c)} a ${f(t.a)} b ${f(t.b)} L ${f(t.L)}<br>μ ${t.mu}${t.noCap ? ' · no cap' : ''} · stroke ${t.sw} · sag ${f(state === 'taut' ? (t.sT != null ? t.sT : 0) : t.s)}</small>`;
  rows += `<tr><th>${note}</th>${cells.join('')}</tr>`;
}
for (const B of TIER_SIZES) await tierRow(B);
for (const B of TAUT) await tierRow(B, 'taut');

// android launcher masks
const G = G1024;
let clipN = 0;
function adaptiveSvg(size, mask, themed) {
  const id = 'clip' + (clipN++);
  const bg = themed ? (themed === 'dark' ? '#2b3a36' : '#d9ebe5') : PAL.teal, ink = themed ? (themed === 'dark' ? '#bfeadd' : PAL.teal) : null;
  const d = G.d, sw = f(G.M.p.strokeW), V = themed ? null : T.VARIANTS.tile;
  const can = (tf) => themed ? `<path${tf} fill="${ink}" fill-rule="evenodd" d="${d.mono}"/>` :
    `<g${tf}><path fill="${V.can}" d="${d.canLeft}"/><path fill="${V.rim}" d="${d.rim}"/><path fill="${V.mouth}" d="${d.mouth}"/></g>`;
  const glyph = `<g transform="translate(${TXY} ${TXY}) scale(${K})">${can('')}${can(' transform="translate(24 0) scale(-1 1)"')}<path d="${d.string}" fill="none" stroke="${themed ? ink : V.str}" stroke-width="${sw}" stroke-linecap="round"/></g>`;
  let clip;
  if (mask === 'circle') clip = `<circle cx="54" cy="54" r="36"/>`;
  else if (mask === 'rounded') clip = `<rect x="18" y="18" width="72" height="72" rx="16"/>`;
  else { const pts = []; for (let i = 0; i < 96; i++) { const t = i / 96 * 2 * Math.PI, c = Math.cos(t), s = Math.sin(t); pts.push((54 + 36 * Math.sign(c) * Math.abs(c) ** 0.5).toFixed(2) + ',' + (54 + 36 * Math.sign(s) * Math.abs(s) ** 0.5).toFixed(2)); } clip = `<polygon points="${pts.join(' ')}"/>`; }
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="18 18 72 72"><defs><clipPath id="${id}">${clip}</clipPath></defs><g clip-path="url(#${id})"><rect width="108" height="108" fill="${bg}"/>${glyph}</g></svg>`;
}
const masks = ['circle', 'squircle', 'rounded'];
const launcherRows = [96, 72, 48].map((sz) => `<tr><th>${sz} px</th>${masks.map((m) => `<td class="s grey2">${adaptiveSvg(sz, m)}</td>`).join('')}${masks.slice(0, 2).map((m, i) => `<td class="s grey2">${adaptiveSvg(sz, m, i ? 'dark' : 'light')}</td>`).join('')}</tr>`).join('');
const legacyImgs = Object.keys(DENS).map((d) => `<figure><img src="android/mipmap-${d}/ic_launcher.png"><img src="android/mipmap-${d}/ic_launcher_round.png"><figcaption>${d} ${DENS[d]}</figcaption></figure>`).join('');
const stat = (px) => paddedSvg(24, 20, 'monowhite').replace('width="24" height="24"', `width="${px}" height="${px}"`);
const css = `body{font:13px/1.4 system-ui,sans-serif;margin:24px;background:#f4f5f3;color:#101614}h1{font-size:20px}h2{margin:32px 0 8px;font-size:15px}table{border-collapse:collapse}th{text-align:left;padding:6px 10px;vertical-align:top;font-weight:600;white-space:nowrap}th small{font-weight:400;color:#555}td.s{padding:10px 12px;vertical-align:middle;border:1px solid #0001}td.light{background:#fff}td.dark{background:#161d1b}td.grey{background:#8b918e}td.grey2{background:#8b918e;text-align:center}img.px{image-rendering:pixelated;display:block}.na{color:#999;font-size:11px}
figure{display:inline-block;margin:0 14px 0 0;text-align:center;font-size:11px}figure img{display:block;margin-bottom:4px}
.bar{display:inline-flex;align-items:center;gap:10px;background:#000;color:#fff;padding:0 10px;height:24px;font:12px system-ui;margin-right:20px}.bar svg{display:block}
.tb{display:flex;align-items:center;gap:6px;background:#202225;height:48px;padding:0 10px;color:#ddd;width:520px;margin:6px 0}.tb .sp{flex:1}.tb img,.tb svg{display:block}
.lx{background:#2b2d31}.mac{background:#e8e8ec;color:#222;border-radius:14px;height:84px;width:420px;display:flex;align-items:center;justify-content:center;gap:12px}.mac img{display:block}
.tabs{display:flex;gap:2px;padding:8px 8px 0;width:520px;margin-top:6px}.tab{display:flex;align-items:center;gap:8px;padding:8px 14px;border-radius:8px 8px 0 0;font:12px system-ui;width:170px}.tab svg,.tab img{display:block}
.pair{display:flex;gap:24px;flex-wrap:wrap}`;
const tray = (S, k) => `desktop/tray/tinline-tray-${S}-${k}.png`;
const html = `<!doctype html><html lang="en"><meta charset="utf-8"><title>Tinline logo contact sheet</title><style>${css}</style><body>
<h1>Tinline mark D: every size, from the one parametric model</h1>
<p>Generated by <code>generate.mjs</code>. Left three columns: the tier's own SVG at 1× on light / dark / mid-grey (≥128 px shown at 128). Right three: the exact pixels, zoomed (nearest neighbour). Rows after 1024: taut variants.</p>
<table>${rows}</table>
<h2>Android adaptive launcher (108 dp canvas, 72 dp window, glyph 60 dp)</h2>
<p>Left to right: circle, squircle, rounded square; then themed monochrome (light, dark). Rows are 96 / 72 / 48 px.</p><table>${launcherRows}</table>
<h2>Legacy mipmaps (square + round), mdpi to xxxhdpi, and Play Store 512</h2>
<div style="background:#8b918e;padding:16px;display:inline-block">${legacyImgs}<figure><img src="android/play-store-icon-512.png" width="128"><figcaption>Play 512 (shown 128)</figcaption></figure></div>
<h2>Notification small icon on a status bar (true size 24 px, then 2×)</h2>
<div><span class="bar">${stat(24)}<span>12:41</span><span>▲ ◼ 87%</span></span><span class="bar" style="height:48px">${stat(48)}<span>12:41</span></span></div>
<h2>Desktop taskbar / tray / dock at true size</h2>
<div class="tb"><img src="desktop/png/tinline-32.png" width="32" height="32"><img src="desktop/png/tinline-24.png" width="24" height="24"><span class="sp"></span><img src="${tray(16, 'white')}" width="16" height="16"><span>12:41</span></div>
<div class="tb lx"><img src="desktop/png/tinline-48.png" width="48" height="48"><span class="sp"></span><img src="${tray(22, 'white')}" width="22" height="22"><img src="${tray(24, 'white')}" width="24" height="24"><img src="${tray(32, 'white')}" width="32" height="32"></div>
<div class="tb" style="background:#f1f1f1"><img src="desktop/png/tinline-20.png" width="20" height="20"><img src="desktop/png/tinline-16.png" width="16" height="16"><span class="sp"></span><img src="${tray(16, 'black')}" width="16" height="16"><img src="${tray(22, 'black')}" width="22" height="22"></div>
<div class="mac"><img src="desktop/png/tinline-64.png" width="64" height="64"><img src="desktop/png/tinline-48.png" width="48" height="48"><img src="desktop/png/tinline-32.png" width="32" height="32"></div>
<h2>Favicon in a tab</h2>
<div class="pair"><div class="tabs" style="background:#dfe1e5"><div class="tab" style="background:#fff;color:#222"><img src="web/favicon-16.png" width="16" height="16">Tinline</div><div class="tab" style="color:#222"><img src="web/favicon-32.png" width="16" height="16">hi-dpi 32→16</div></div>
<div class="tabs" style="background:#202124"><div class="tab" style="background:#35363a;color:#eee"><img src="web/favicon-16.png" width="16" height="16">Tinline</div><div class="tab" style="color:#eee"><img src="web/favicon.svg" width="16" height="16">favicon.svg</div></div></div>
<div class="pair" style="margin-top:12px"><img src="web/apple-touch-icon.png" width="90"><img src="web/icon-192.png" width="96"><img src="web/icon-maskable-512.png" width="96"></div>
</body></html>`;
put('index.html', html);

await browser.close();
console.log(`wrote ${written.length} files under ${HERE}`);
