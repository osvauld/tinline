/* Generated "camera" feeds for the simulation: soft abstract moving light with a portrait-like shape (or a
   back-camera scene). No real camera and no external media. Drawn small and scaled up by CSS, so every
   edge is soft and the cost stays low. One shared loop drives every visible canvas (about 24 fps). */

export type FeedKind = 'peer' | 'self' | 'back';
export interface FeedOpts { kind: FeedKind; hue: number; seed?: number }

const TAU = Math.PI * 2;
const hsl = (h: number, s: number, l: number, a = 1) => `hsl(${((h % 360) + 360) % 360} ${s}% ${l}% / ${a})`;

function blob(c: CanvasRenderingContext2D, x: number, y: number, r: number, col: string) {
  const g = c.createRadialGradient(x, y, 0, x, y, r);
  g.addColorStop(0, col); g.addColorStop(1, col.replace(/ \/ [\d.]+\)/, ' / 0)'));
  c.fillStyle = g; c.fillRect(x - r, y - r, r * 2, r * 2);
}

export function drawFeed(c: CanvasRenderingContext2D, w: number, h: number, t: number, o: FeedOpts) {
  const s = t / 1000, k = o.seed ?? 0, H = o.hue;
  // Room: a dim vertical gradient, then slow drifting pools of light.
  const bg = c.createLinearGradient(0, 0, 0, h);
  if (o.kind === 'back') { bg.addColorStop(0, hsl(H + 150, 38, 62)); bg.addColorStop(0.58, hsl(H + 170, 30, 74)); bg.addColorStop(0.58, hsl(H + 20, 28, 30)); bg.addColorStop(1, hsl(H + 10, 30, 18)); }
  else { bg.addColorStop(0, hsl(H + 20, 34, o.kind === 'peer' ? 30 : 36)); bg.addColorStop(1, hsl(H - 25, 38, o.kind === 'peer' ? 14 : 18)); }
  c.fillStyle = bg; c.fillRect(0, 0, w, h);
  blob(c, w * (0.28 + 0.12 * Math.sin(s * 0.31 + k)), h * (0.3 + 0.08 * Math.cos(s * 0.23 + k)), w * 0.75, hsl(H + 40, 70, 62, 0.34));
  blob(c, w * (0.78 + 0.1 * Math.cos(s * 0.27 + k)), h * (0.7 + 0.08 * Math.sin(s * 0.35)), w * 0.8, hsl(H - 50, 62, 50, 0.3));

  if (o.kind === 'back') {
    // A pan across a simple scene: soft hills and a low sun.
    const pan = (s * 6) % w;
    blob(c, w * 0.7 - pan * 0.15, h * 0.4, w * 0.5, hsl(H + 60, 90, 80, 0.7));
    for (let i = 0; i < 3; i++) {
      c.fillStyle = hsl(H + 130 + i * 14, 22 + i * 4, 40 - i * 9, 0.9);
      c.beginPath(); c.moveTo(0, h);
      for (let x = 0; x <= w; x += 6) c.lineTo(x, h * (0.62 + i * 0.1) + Math.sin(((x + pan * (0.4 + i * 0.3)) / w) * TAU * (1 + i * 0.4) + i) * h * 0.04);
      c.lineTo(w, h); c.closePath(); c.fill();
    }
    return;
  }

  // Portrait: shoulders, neck, head, with a slow sway and a tiny breath. Gradients keep every edge soft.
  const sway = Math.sin(s * 0.6 + k) * w * 0.018, breath = Math.sin(s * 1.1) * h * 0.004;
  const sc = o.kind === 'peer' ? 1 : 0.94;
  const cx = w * 0.5 + sway, headY = h * 0.43 + breath, hr = Math.min(w * 0.16, h * 0.092) * sc;
  const bodyCol = hsl(H + 200, 28, 24), skin = hsl(28 + (H % 30), 38, o.kind === 'peer' ? 62 : 66), hair = hsl(H + 10, 25, 14);
  c.fillStyle = bodyCol;
  c.beginPath(); c.ellipse(cx, h * 0.92 + breath, w * 0.43 * sc, h * 0.26 * sc, 0, Math.PI, TAU); c.fill();
  c.fillStyle = hsl(28 + (H % 30), 32, 50); c.fillRect(cx - hr * 0.38, headY + hr * 0.7, hr * 0.76, hr * 1.1);   // neck
  c.fillStyle = hair; c.beginPath(); c.ellipse(cx, headY - hr * 0.1, hr * 0.94, hr * 1.12, 0, 0, TAU); c.fill();
  const fy = headY + hr * 0.12;
  const hg = c.createRadialGradient(cx - hr * 0.25, fy - hr * 0.3, hr * 0.1, cx, fy, hr * 1.0);
  hg.addColorStop(0, hsl(30 + (H % 30), 44, 76)); hg.addColorStop(0.7, skin); hg.addColorStop(1, hsl(24 + (H % 30), 36, 46));
  c.fillStyle = hg; c.beginPath(); c.ellipse(cx, fy, hr * 0.8, hr * 0.96, 0, 0, TAU); c.fill();
  // A window-light sweep so the picture feels alive.
  const sx = ((s * 0.07 + k) % 1.6 - 0.3) * w;
  const sg = c.createLinearGradient(sx - w * 0.25, 0, sx + w * 0.25, h * 0.4);
  sg.addColorStop(0, 'hsl(0 0% 100% / 0)'); sg.addColorStop(0.5, 'hsl(0 0% 100% / .07)'); sg.addColorStop(1, 'hsl(0 0% 100% / 0)');
  c.fillStyle = sg; c.fillRect(0, 0, w, h);
}

/* ---- shared loop ---- */
interface Sub { draw: (t: number) => void; on: () => boolean }
const subs = new Set<Sub>();
let raf = 0, last = 0;
const STEP = 1000 / 24;
function frame(t: number) {
  raf = 0;
  if (!subs.size || document.hidden) return;
  if (t - last >= STEP - 1) { last = t; subs.forEach((s) => { if (s.on()) s.draw(t); }); }
  raf = requestAnimationFrame(frame);
}
export function subscribe(s: Sub) {
  subs.add(s);
  if (!raf) raf = requestAnimationFrame(frame);
  return () => { subs.delete(s); };
}
if (typeof document !== 'undefined') document.addEventListener('visibilitychange', () => { if (!document.hidden && subs.size && !raf) raf = requestAnimationFrame(frame); });
