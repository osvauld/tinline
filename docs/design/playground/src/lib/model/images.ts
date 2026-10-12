/* Picture helpers for the profile area: input checks (the packet's limits), the crop/normalise step the app does
   before the core sees bytes, and generated sample portraits so the playground needs no stock photos or network.
   Plain DOM canvas code, no state. */

export const IMG = {
  maxBytes: 2 * 1024 * 1024,   // input PNG/JPEG/WebP
  maxSide: 4096,               // input pixels per side
  out: 512,                    // normalised PNG, pixels per side
  outMaxBytes: 1024 * 1024,    // normalised PNG
} as const;

const mb = (n: number) => (n / (1024 * 1024)).toFixed(1).replace(/\.0$/, '');

export type Sniff = 'png' | 'jpeg' | 'webp' | null;
/** Look at the first bytes, not the file name or the claimed type. */
export async function sniff(b: Blob): Promise<Sniff> {
  const h = new Uint8Array(await b.slice(0, 12).arrayBuffer());
  if (h[0] === 0x89 && h[1] === 0x50 && h[2] === 0x4e && h[3] === 0x47) return 'png';
  if (h[0] === 0xff && h[1] === 0xd8 && h[2] === 0xff) return 'jpeg';
  if (h[0] === 0x52 && h[1] === 0x49 && h[2] === 0x46 && h[3] === 0x46 && h[8] === 0x57 && h[9] === 0x45 && h[10] === 0x42 && h[11] === 0x50) return 'webp';
  return null;
}

export type Picked = { ok: true; bitmap: ImageBitmap; url: string; w: number; h: number; blob: Blob } | { ok: false; error: string };

/** Check a chosen file against the limits and decode it for the crop step. Rejections say what is wrong. */
export async function inspectImage(file: Blob): Promise<Picked> {
  if (file.size > IMG.maxBytes) return { ok: false, error: `That picture is ${mb(file.size)} MB. Pictures can be up to ${mb(IMG.maxBytes)} MB.` };
  if (!(await sniff(file))) return { ok: false, error: 'Tinline can use PNG, JPEG or WebP pictures. That file is a different kind of file.' };
  let bitmap: ImageBitmap;
  try { bitmap = await createImageBitmap(file); } catch { return { ok: false, error: 'That file couldn’t be read as a picture. It may be damaged.' }; }
  if (bitmap.width > IMG.maxSide || bitmap.height > IMG.maxSide) {
    const [w, h] = [bitmap.width, bitmap.height];
    bitmap.close();
    return { ok: false, error: `That picture is ${w} × ${h} pixels. Tinline accepts up to ${IMG.maxSide} pixels on a side.` };
  }
  return { ok: true, bitmap, url: URL.createObjectURL(file), w: bitmap.width, h: bitmap.height, blob: file };
}

/** The crop viewport is `view` css px square; the picture sits at (ox, oy) scaled so it always covers it. */
export interface Crop { ox: number; oy: number; zoom: number }
export const coverScale = (w: number, h: number, view: number, zoom: number) => (Math.max(view / w, view / h)) * zoom;
export function clampCrop(c: Crop, w: number, h: number, view: number): Crop {
  const s = coverScale(w, h, view, c.zoom);
  const minX = view - w * s, minY = view - h * s;
  return { zoom: c.zoom, ox: Math.min(0, Math.max(minX, c.ox)), oy: Math.min(0, Math.max(minY, c.oy)) };
}
/** Centre the picture in the viewport at a zoom. */
export function centred(w: number, h: number, view: number, zoom = 1): Crop {
  const s = coverScale(w, h, view, zoom);
  return { zoom, ox: (view - w * s) / 2, oy: (view - h * s) / 2 };
}
/** Zoom about the viewport centre, keeping the point under the centre where it is. */
export function zoomAt(c: Crop, zoom: number, w: number, h: number, view: number): Crop {
  const s0 = coverScale(w, h, view, c.zoom), s1 = coverScale(w, h, view, zoom);
  const cx = (view / 2 - c.ox) / s0, cy = (view / 2 - c.oy) / s0;
  return clampCrop({ zoom, ox: view / 2 - cx * s1, oy: view / 2 - cy * s1 }, w, h, view);
}

/** App-side preparation: crop the chosen square and scale it to a PNG of at most 512 px. */
export async function renderSquare(p: { bitmap: ImageBitmap; w: number; h: number }, crop: Crop, view: number): Promise<Blob> {
  const s = coverScale(p.w, p.h, view, crop.zoom);
  const side = view / s;                       // source pixels covered by the viewport
  const out = Math.min(IMG.out, Math.max(1, Math.round(side)));
  const c = canvas(out, out);
  const g = c.getContext('2d')!;
  g.imageSmoothingQuality = 'high';
  g.drawImage(p.bitmap, -crop.ox / s, -crop.oy / s, side, side, 0, 0, out, out);
  return toBlob(c);
}

const canvas = (w: number, h: number) => { const c = document.createElement('canvas'); c.width = w; c.height = h; return c; };
/** Encoded synchronously (toDataURL): canvas.toBlob and OffscreenCanvas.convertToBlob wait for an idle moment, which can take a second or more. */
export function toBlob(c: HTMLCanvasElement, type = 'image/png'): Promise<Blob> {
  const bin = atob(c.toDataURL(type).split(',')[1]);
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return Promise.resolve(new Blob([bytes], { type }));
}

/* ---------- generated sample pictures ---------- */

function rng(seed: string) {
  let h = 1779033703 ^ seed.length;
  for (let i = 0; i < seed.length; i++) { h = Math.imul(h ^ seed.charCodeAt(i), 3432918353); h = (h << 13) | (h >>> 19); }
  let a = h >>> 0;
  return () => { a = (a + 0x6d2b79f5) >>> 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return (((t ^ (t >>> 14)) >>> 0) / 4294967296); };
}

/** A soft, abstract portrait-like picture: a coloured wash, a blurred head and shoulders. Same seed, same picture.
    `light` forces a pale picture so the avatar's inner hairline can be judged against light surfaces. */
export async function samplePortrait(seed: string, size: number = IMG.out, opts: { light?: boolean; w?: number; h?: number } = {}): Promise<Blob> {
  const r = rng(seed);
  const W = opts.w ?? size, H = opts.h ?? size;
  const c = canvas(W, H);
  const g = c.getContext('2d')!;
  const hue = Math.floor(r() * 360), hue2 = (hue + 40 + Math.floor(r() * 80)) % 360;
  const L = opts.light ? 86 : 52 + Math.floor(r() * 12);
  const bg = g.createLinearGradient(0, 0, W, H);
  bg.addColorStop(0, `hsl(${hue} 55% ${L + 10}%)`);
  bg.addColorStop(1, `hsl(${hue2} 50% ${L - 8}%)`);
  g.fillStyle = bg; g.fillRect(0, 0, W, H);
  const m = Math.min(W, H);
  const blur = (px: number) => { g.filter = px ? `blur(${Math.round(px * m / 512)}px)` : 'none'; };
  blur(34);
  for (let i = 0; i < 4; i++) {            // soft background light
    g.fillStyle = `hsl(${(hue + i * 37) % 360} 60% ${L + 14}% / .55)`;
    g.beginPath(); g.arc(r() * W, r() * H, m * (.18 + r() * .2), 0, Math.PI * 2); g.fill();
  }
  const skin = `hsl(${18 + Math.floor(r() * 18)} ${30 + Math.floor(r() * 20)}% ${opts.light ? 78 : 46 + Math.floor(r() * 24)}%)`;
  const cloth = `hsl(${(hue2 + 160) % 360} 38% ${opts.light ? 70 : 30 + Math.floor(r() * 16)}%)`;
  const cx = W * (.46 + r() * .08);
  blur(5);
  g.fillStyle = cloth;                      // shoulders
  g.beginPath(); g.ellipse(cx, H * 1.02, m * .46, m * .34, 0, Math.PI, 0); g.fill();
  g.fillStyle = skin;                       // neck
  g.fillRect(cx - m * .07, H * .56, m * .14, m * .14);
  g.fillStyle = `hsl(${12 + Math.floor(r() * 30)} 28% ${14 + Math.floor(r() * 24)}%)`; // hair behind the head
  g.beginPath(); g.ellipse(cx, H * .43, m * .215, m * .235, 0, 0, Math.PI * 2); g.fill();
  g.fillStyle = skin;                       // head
  g.beginPath(); g.ellipse(cx, H * .46, m * .17, m * .205, 0, 0, Math.PI * 2); g.fill();
  blur(0);
  const grain = g.getImageData(0, 0, W, H);   // faint grain keeps it from reading as a flat vector
  for (let i = 0; i < grain.data.length; i += 4) { const n = (r() - .5) * 7; grain.data[i] += n; grain.data[i + 1] += n; grain.data[i + 2] += n; }
  g.putImageData(grain, 0, 0);
  return toBlob(c);
}

/** Deliberately wrong inputs for the drawer: each is rejected before the profile is touched. */
export const BAD_FILES = {
  text: () => new File(['hello, this is not a picture'], 'holiday.png', { type: 'image/png' }),
  gif: () => new File([Uint8Array.from([0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 1, 0, 1, 0, 0, 0, 0x3b])], 'loop.gif', { type: 'image/gif' }),
  heavy: async () => { // a real PNG padded past 2 MiB
    const png = await samplePortrait('heavy', 64);
    return new File([png, new Uint8Array(IMG.maxBytes + 1024)], 'huge.png', { type: 'image/png' });
  },
  wide: async () => new File([await samplePortrait('wide', 0, { w: IMG.maxSide + 1, h: 24 })], 'panorama.png', { type: 'image/png' }),
};
