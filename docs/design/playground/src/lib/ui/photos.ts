/* Sample photos for the playground chat: soft generated landscapes keyed by file name (no real pictures in the repo),
   and the 4x3 colour preview the core sends with a photo (chat/5, crates/core/src/chat/photo.rs), made here the same
   way: the picture averaged into a 4x3 grid. */

const PALETTES: { sky: [string, string]; hills: [string, string, string]; sun: string }[] = [
  { sky: ['#9cc3e8', '#dfeaf2'], hills: ['#5b7fa8', '#3c5e86', '#2a4566'], sun: '#fff3c4' },
  { sky: ['#f4c6c2', '#fbe7dc'], hills: ['#c9776f', '#9a4f4b', '#6e3534'], sun: '#f5c451' },
  { sky: ['#e9e3d3', '#f4f0e6'], hills: ['#b9b08f', '#8f8768', '#6a644c'], sun: '#e0a84a' },
  { sky: ['#b9dccb', '#e6f2ea'], hills: ['#5f9c84', '#3f7a63', '#2b5948'], sun: '#fff1b8' },
];

const hash = (s: string) => [...s].reduce((h, ch) => (h * 31 + ch.charCodeAt(0)) >>> 0, 7);

/** A landscape of `w` x `h` for `name` (same name, same picture). */
export function photoSrc(name: string, w: number, h: number): string {
  const p = PALETTES[hash(name) % PALETTES.length];
  return 'data:image/svg+xml,' + encodeURIComponent(`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" preserveAspectRatio="xMidYMid slice">
    <defs><linearGradient id="s" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${p.sky[0]}"/><stop offset="1" stop-color="${p.sky[1]}"/></linearGradient></defs>
    <rect width="${w}" height="${h}" fill="url(#s)"/><circle cx="${w * .72}" cy="${h * .3}" r="${Math.min(w, h) * .09}" fill="${p.sun}"/>
    <path d="M0 ${h * .7} Q${w * .25} ${h * .5} ${w * .5} ${h * .68} T${w} ${h * .62} V${h} H0Z" fill="${p.hills[0]}"/>
    <path d="M0 ${h * .82} Q${w * .35} ${h * .64} ${w * .7} ${h * .8} T${w} ${h * .78} V${h} H0Z" fill="${p.hills[1]}"/>
    <path d="M0 ${h * .93} Q${w * .5} ${h * .82} ${w} ${h * .92} V${h} H0Z" fill="${p.hills[2]}"/></svg>`);
}

const previews = new Map<string, Promise<string>>();
/** The 4x3 preview of `src` as a tiny PNG data URL (draw it scaled up with smoothing and a blur). */
export function previewOf(src: string): Promise<string> {
  let p = previews.get(src);
  if (!p) {
    p = new Promise((done) => {
      const img = new Image();
      img.onload = () => {
        const c = document.createElement('canvas');
        c.width = 4; c.height = 3;
        const g = c.getContext('2d')!;
        g.imageSmoothingQuality = 'high';
        g.drawImage(img, 0, 0, 4, 3);
        done(c.toDataURL());
      };
      img.onerror = () => done('');
      img.src = src;
    });
    previews.set(src, p);
  }
  return p;
}

/** The photo's box in the chat: its true shape, at most `maxW` x `maxH`, at least 120 wide; 4:3 when unknown. */
export function photoBox(w: number | undefined, h: number | undefined, maxW: number, maxH: number) {
  const [pw, ph] = w && h ? [w, h] : [4, 3];
  const s = Math.min(maxW / pw, maxH / ph);
  return { w: Math.max(120, Math.round(pw * s)), h: Math.round(ph * s) };
}
