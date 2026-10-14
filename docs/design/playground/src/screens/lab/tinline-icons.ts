/* Icon lab geometry (?lab=icons / icons.html). Option 4 study: Tinline's own line icons, built from the mark's maths
   (lib/brand/MARK-MATH.md) and the corner construction (lib/geom/GEOMETRY.md). NOT the shipped icon set: icons.ts stays
   the source until a set is chosen. Every glyph is a list of parts [d, cls]; cls 'str' = the string, 'f' = a filled mouth/dot. */
import { roundedRectPath } from '../../lib/geom/shape';
import { geo } from '../../lib/geom/geo.svelte';

export type Part = [d: string, cls: string];
export type Glyph = { why: string; parts: () => Part[]; kind: 'new' | 'ours' | 'signup' };

const SIN = Math.sin(30.6 * Math.PI / 180);          // can view angle from the mark: a = b·sinθ
const f = (n: number) => +n.toFixed(2);
const P = (d: string, c = ''): Part => [d, c];

/* Rounded rect with the app's corner smoothing (geo.smooth, 0.6 by default), so icon corners match bubbles and buttons. */
const rr = (x: number, y: number, w: number, h: number, r: number) => roundedRectPath(w, h, [r, r, r, r], geo.smooth, x, y);

const rimV = (cx: number, cy: number, b: number) => { const a = b * SIN;
  return `M${f(cx)} ${f(cy - b)}A${f(a)} ${f(b)} 0 1 1 ${f(cx)} ${f(cy + b)}A${f(a)} ${f(b)} 0 1 1 ${f(cx)} ${f(cy - b)}`; };
/* A can on its side, mouth at cx facing dir (+1 right, -1 left), body L behind it, back cap of the same ellipse. */
const canSide = (cx: number, cy: number, b: number, L: number, dir: 1 | -1) => { const a = b * SIN, bx = cx - dir * L;
  return rimV(cx, cy, b) + `M${f(cx)} ${f(cy - b)}H${f(bx)}A${f(a)} ${f(b)} 0 0 ${dir > 0 ? 0 : 1} ${f(bx)} ${f(cy + b)}H${f(cx)}`; };
const rimH = (cx: number, cy: number, b: number) => { const a = b * SIN;
  return `M${f(cx - b)} ${f(cy)}A${f(b)} ${f(a)} 0 1 1 ${f(cx + b)} ${f(cy)}A${f(b)} ${f(a)} 0 1 1 ${f(cx - b)} ${f(cy)}`; };
/* A standing can, mouth up. */
const canUp = (cx: number, cy: number, b: number, L: number) => { const a = b * SIN;
  return rimH(cx, cy, b) + `M${f(cx - b)} ${f(cy)}V${f(cy + L)}A${f(b)} ${f(a)} 0 0 0 ${f(cx + b)} ${f(cy + L)}V${f(cy)}`; };
/* The mouth: the rim scaled by mu, filled. It is what makes a rim read as a can and not a ring. */
const mouthV = (cx: number, cy: number, b: number, mu = .5) => { const a = b * SIN * mu, bb = b * mu;
  return `M${f(cx)} ${f(cy - bb)}A${f(a)} ${f(bb)} 0 1 1 ${f(cx)} ${f(cy + bb)}A${f(a)} ${f(bb)} 0 1 1 ${f(cx)} ${f(cy - bb)}z`; };
const mouthH = (cx: number, cy: number, b: number, mu = .5) => { const a = b * SIN * mu, bb = b * mu;
  return `M${f(cx - bb)} ${f(cy)}A${f(bb)} ${f(a)} 0 1 1 ${f(cx + bb)} ${f(cy)}A${f(bb)} ${f(a)} 0 1 1 ${f(cx - bb)} ${f(cy)}z`; };
/* The string: one quadratic Bézier, control 2s below the chord midpoint, so the midpoint sags exactly s (MARK-MATH §2). */
const sag = (x1: number, y1: number, x2: number, y2: number, s: number) =>
  `M${f(x1)} ${f(y1)}Q${f((x1 + x2) / 2)} ${f((y1 + y2) / 2 + 2 * s)} ${f(x2)} ${f(y2)}`;
/* Arrowhead at the tip, (dx, dy) = direction of travel, so heads sit on the curve's true tangent. */
const head = (tx: number, ty: number, dx: number, dy: number, len = 5, deg = 40) => {
  const m = Math.hypot(dx, dy), ux = dx / m, uy = dy / m, r = deg * Math.PI / 180, c = Math.cos(r);
  const pt = (s: number) => { const sn = Math.sin(s * r); return `${f(tx - len * (ux * c - uy * sn))} ${f(ty - len * (uy * c + ux * sn))}`; };
  return `M${pt(1)}L${f(tx)} ${f(ty)}L${pt(-1)}`; };
const qArrow = (x1: number, y1: number, cx: number, cy: number, x2: number, y2: number, len?: number) =>
  `M${f(x1)} ${f(y1)}Q${f(cx)} ${f(cy)} ${f(x2)} ${f(y2)}` + head(x2, y2, x2 - cx, y2 - cy, len);
const arcArrow = (cx: number, cy: number, r: number, a0: number, a1: number, len = 4.5) => {
  const p = (a: number) => [cx + r * Math.cos(a * Math.PI / 180), cy + r * Math.sin(a * Math.PI / 180)];
  const [x0, y0] = p(a0), [x1, y1] = p(a1), t = a1 * Math.PI / 180;
  return `M${f(x0)} ${f(y0)}A${r} ${r} 0 ${(a1 - a0) % 360 > 180 ? 1 : 0} 1 ${f(x1)} ${f(y1)}` + head(x1, y1, -Math.sin(t), Math.cos(t), len); };
const circ = (x: number, y: number, r: number) => `M${f(x - r)} ${y}a${r} ${r} 0 1 0 ${2 * r} 0a${r} ${r} 0 1 0 ${-2 * r} 0z`;

const CAN = { b: 4.4, L: 3.6 };
const M = (cx: number, cy: number, b = CAN.b) => P(mouthV(cx, cy, b), 'f');
const MIC = () => [P(canUp(12, 4.9, 3.5, 6.4)), P(mouthH(12, 4.9, 3.5), 'f'), P('M5.5 11A6.5 6.5 0 0 0 18.5 11M12 17.5V21M9 21h6')];
const BELL = () => [P('M6 16V11a6 6 0 0 1 12 0v5'), P(`M4.8 16.2A7.2 ${f(7.2 * SIN)} 0 0 0 19.2 16.2`), P('M4.8 16.2h14.4'), P('M10.2 20.2a2 2 0 0 0 3.6 0')];
const CAM = () => [P(rr(3, 7, 11, 10, 2.5)), P('M14 10.2l4.6-2.2M14 13.8l4.6 2.2'), P(rimV(18.6, 12, 4)), P(mouthV(18.6, 12, 4), 'f')];
const PERSON = (x: number) => [P(circ(x, 8, 3.6)), P(`M${x - 7} 20.5A7 6.3 0 0 1 ${x + 7} 20.5`)];
const OFF = P('M3.5 3.5l17 17');

export const OWN: Record<string, Glyph> = {
  call: { kind: 'new', why: 'The mark in line: two cans, slack string. Replaces the handset wherever a call starts or shows.',
    parts: () => [P(canSide(7.8, 11, CAN.b, CAN.L, 1)), M(7.8, 11), P(canSide(16.2, 11, CAN.b, CAN.L, -1)), M(16.2, 11), P(sag(7.8, 11, 16.2, 11, 3.4), 'str')] },
  callend: { kind: 'new', why: 'Cans apart, string dropped. Says "line down" calmly, without a red handset.',
    parts: () => [P(canSide(6, 8.5, CAN.b, CAN.L, 1)), M(6, 8.5), P(canSide(18, 8.5, CAN.b, CAN.L, -1)), M(18, 8.5),
      P('M6 8.5Q9.6 12.5 9.2 19.5', 'str'), P('M18 8.5Q14.4 12.5 14.8 19.5', 'str')] },
  phone: { kind: 'new', why: 'One can, string running off to someone. For small inline places: call history, a menu item.',
    parts: () => [P(canSide(8.6, 8.6, 4.4, 3.6, 1)), M(8.6, 8.6), P('M8.6 8.6Q15 22 21.5 14.5', 'str')] },
  mic: { kind: 'new', why: 'A standing can on a stand, same rim angle as the mark.', parts: MIC },
  micoff: { kind: 'new', why: 'Mic with the one 45° slash every "off" uses.', parts: () => [...MIC(), OFF] },
  vol: { kind: 'new', why: 'The can is the speaker: mouth right, two sound arcs.',
    parts: () => [P(canSide(9.4, 12, 4.8, 3.4, 1)), M(9.4, 12, 4.8), P('M15 9a4.2 4.2 0 0 1 0 6M17.8 6.4a7.9 7.9 0 0 1 0 11.2')] },
  voloff: { kind: 'new', why: 'Speaker can, a small cross where the sound would be.',
    parts: () => [P(canSide(9.4, 12, 4.8, 3.4, 1)), M(9.4, 12, 4.8), P('M15.5 9.5l5 5M20.5 9.5l-5 5')] },
  video: { kind: 'new', why: 'Smoothed body plus a lens drawn as a can mouth.', parts: CAM },
  camoff: { kind: 'signup', why: 'Sign-up uses it. Camera with the shared slash.', parts: () => [...CAM(), OFF] },
  link: { kind: 'new', why: 'Linked devices: two can mouths face on, a short sagging string between them.',
    parts: () => [P(rimV(5.6, 12, 4.6)), M(5.6, 12, 4.6), P(rimV(18.4, 12, 4.6)), M(18.4, 12, 4.6), P(sag(5.6 + 4.6 * SIN, 12, 18.4 - 4.6 * SIN, 12, 2.2), 'str')] },
  direct: { kind: 'ours', why: 'Already ours on Android: two ends, taut string.',
    parts: () => [P(circ(4, 12, 2.4), 'f'), P(circ(20, 12, 2.4), 'f'), P('M6.4 12h11.2', 'str')] },
  relayed: { kind: 'ours', why: 'Already ours: the string passes through a sealed box.',
    parts: () => [P(circ(4, 12, 2.4), 'f'), P(circ(20, 12, 2.4), 'f'), P('M6.4 12h3M14.6 12h3', 'str'), P(rr(9.5, 9, 5, 6, 1.4))] },
  chat: { kind: 'new', why: 'Smoothed bubble; the tail is a short curve, not a hard triangle.',
    parts: () => [P(rr(3, 4, 18, 13, 3)), P('M8 17Q7.6 19.4 5.4 20.8Q9.6 20.6 12.4 17')] },
  send: { kind: 'new', why: 'Sent down the line: a thrown curve, head on its tangent. No paper plane.',
    parts: () => [P(qArrow(4.5, 19.5, 13, 18.5, 19.5, 5.5, 6))] },
  reply: { kind: 'new', why: 'Arrow on a sagging curve.', parts: () => [P(qArrow(20, 19, 20, 9.5, 5, 9.5, 5))] },
  forward: { kind: 'new', why: 'Reply, mirrored.', parts: () => [P(qArrow(4, 19, 4, 9.5, 19, 9.5, 5))] },
  retry: { kind: 'new', why: 'Arc arrow, head on the tangent.', parts: () => [P(arcArrow(12, 12, 7.5, -30, 250))] },
  user: { kind: 'new', why: 'Head and half-ellipse shoulders.', parts: () => PERSON(12) },
  users: { kind: 'new', why: 'Two people, same shapes as user.',
    parts: () => [P(circ(9, 8.3, 3.3)), P('M2.8 20.5A6.2 5.8 0 0 1 15.2 20.5'), P('M15.6 5.4a3 3 0 0 1 0 5.8M17.8 14.6a6 6 0 0 1 3.6 5.9')] },
  userplus: { kind: 'signup', why: 'Sign-up "Add my first contact". One person plus the standard plus.',
    parts: () => [...PERSON(9.5), P('M19 7.5v6M16 10.5h6')] },
  bell: { kind: 'new', why: 'A bell whose mouth is a can rim, for the knock.', parts: BELL },
  mute: { kind: 'new', why: 'Bell with the shared slash.', parts: () => [...BELL(), OFF] },
  // Sign-up's own icons, redrawn to the same rules.
  back: { kind: 'signup', why: 'Plain on purpose. Same as today.', parts: () => [P('M15 5l-7 7 7 7')] },
  next: { kind: 'signup', why: 'The Android round "Continue" arrow: a plain arrow, the circle is the button.', parts: () => [P('M5 12h13.5'), P(head(19, 12, 1, 0, 6, 42))] },
  enter: { kind: 'signup', why: 'A drawn ↵ for the keycap, if the text glyph renders unevenly across fonts. The keycap may keep the character.',
    parts: () => [P(qArrow(19, 5, 19, 15, 6, 15, 4.5))] },
  sun: { kind: 'signup', why: 'Theme switch. Short rays with round ends.',
    parts: () => [P(circ(12, 12, 4)), P('M12 2.5v1.8M12 19.7v1.8M2.5 12h1.8M19.7 12h1.8M5.3 5.3l1.3 1.3M17.4 17.4l1.3 1.3M5.3 18.7l1.3-1.3M17.4 6.6l1.3-1.3')] },
  moon: { kind: 'signup', why: 'Theme switch. Unchanged.', parts: () => [P('M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z')] },
  copy: { kind: 'signup', why: '"Copy my words". Two smoothed cards.', parts: () => [P(rr(9, 9, 11, 11, 2.5)), P('M5 15V7.5A2.5 2.5 0 0 1 7.5 5H15')] },
  scan: { kind: 'signup', why: '"Scan their code". Corner brackets around a smoothed square.',
    parts: () => [P('M4 8.5V6.5A2.5 2.5 0 0 1 6.5 4h2M15.5 4h2A2.5 2.5 0 0 1 20 6.5v2M20 15.5v2a2.5 2.5 0 0 1-2.5 2.5h-2M8.5 20h-2A2.5 2.5 0 0 1 4 17.5v-2'), P(rr(8.5, 8.5, 7, 7, 1.5))] },
  qr: { kind: 'signup', why: '"Show my code". Smoothed finder squares.',
    parts: () => [P(rr(4, 4, 6, 6, 1.5)), P(rr(14, 4, 6, 6, 1.5)), P(rr(4, 14, 6, 6, 1.5)), P('M14 14h2v2h-2zM18 14h2v2M14 18h2v2h4v-2')] },
};

/* Today's playground icons (icons.ts), for comparison; read-only copies so this lab never touches the shared file. */
export const CUR: Record<string, string> = {
  phone: '<path d="M5 4h3.5l1.5 4.5-2.2 1.4a11 11 0 0 0 6.3 6.3l1.4-2.2L20 15.5V19a1.5 1.5 0 0 1-1.6 1.5A16 16 0 0 1 3.5 5.6 1.5 1.5 0 0 1 5 4z"/>',
  mic: '<rect x="9" y="3" width="6" height="11" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3"/>',
  micoff: '<path d="M15 9.3V6a3 3 0 0 0-5.7-1.3M9 9v2a3 3 0 0 0 4.6 2.5M5 11a7 7 0 0 0 11.5 5.4M19 11a7 7 0 0 1-.6 2.8M12 18v3M3 3l18 18"/>',
  vol: '<path d="M4 9v6h4l5 4V5L8 9zM16.5 9a4 4 0 0 1 0 6"/>',
  video: '<rect x="3" y="6.5" width="12.5" height="11" rx="3"/><path d="M15.5 10.6l4.4-2.5a.6.6 0 0 1 .9.5v6.8a.6.6 0 0 1-.9.5l-4.4-2.5"/>',
  camoff: '<path d="M10 6.5H6.5A3.5 3.5 0 0 0 3 10v4a3.5 3.5 0 0 0 3.5 3.5h6A3 3 0 0 0 15.5 14.5v-4M15.5 10.6l4.4-2.5a.6.6 0 0 1 .9.5v6.8a.6.6 0 0 1-.9.5l-.4-.2M4 4l16 16"/>',
  link: '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
  chat: '<path d="M4 5h16v11H9l-5 4z"/>',
  send: '<path d="M22 2L11 13M22 2l-7 20-4-9-9-4 20-7z"/>',
  reply: '<path d="M9 14L4 9l5-5M4 9h10a6 6 0 0 1 6 6v3"/>',
  forward: '<path d="M5 12h14M13 6l6 6-6 6"/>',
  next: '<path d="M5 12h14M13 6l6 6-6 6"/>',
  enter: '<path d="M19 5v6a4 4 0 0 1-4 4H6M10 11l-4 4 4 4"/>',
  retry: '<path d="M20 12a8 8 0 1 1-2.6-5.9M20 4v5h-5"/>',
  users: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5"/><circle cx="17" cy="9" r="2.5"/><path d="M16.5 14.6c2.6.2 4.5 2 5 4.9"/>',
  userplus: '<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5M19 8v6M16 11h6"/>',
  user: '<circle cx="12" cy="8" r="3.5"/><path d="M5.5 20c.5-3.5 3.2-5.5 6.5-5.5s6 2 6.5 5.5"/>',
  bell: '<path d="M6 16v-5a6 6 0 0 1 12 0v5l2 2H4zM10 21h4"/>',
  mute: '<path d="M8.5 5.3A6 6 0 0 1 18 11v4M6 11v5l-2 2h12M10 21h4M3 3l18 18"/>',
  direct: '<path class="f" d="M1.5,12a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0z"/><path class="f" d="M17.5,12a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0z"/><path d="M6.5,12h11"/>',
  relayed: '<path class="f" d="M1.5,12a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0z"/><path class="f" d="M17.5,12a2.5,2.5 0 1 0 5,0a2.5,2.5 0 1 0 -5,0z"/><path d="M6.5,12h3M14.5,12h3M10.7,9h2.6a1.2,1.2 0 0 1 1.2,1.2v3.6a1.2,1.2 0 0 1 -1.2,1.2h-2.6a1.2,1.2 0 0 1 -1.2,-1.2v-3.6a1.2,1.2 0 0 1 1.2,-1.2z"/>',
  back: '<path d="M15 5l-7 7 7 7"/>', chevr: '<path d="M10 7l5 5-5 5"/>', chev: '<path d="M7 10l5 5 5-5"/>',
  x: '<path d="M6 6l12 12M18 6L6 18"/>', plus: '<path d="M12 5v14M5 12h14"/>', check: '<path d="M5 12.5l4.5 4.5L19 7"/>',
  search: '<circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/>',
  clip: '<path d="M21 11l-9 9a5 5 0 0 1-7-7l9-9a3.5 3.5 0 0 1 5 5l-9 9a2 2 0 0 1-3-3l8-8"/>',
  info: '<circle cx="12" cy="12" r="9"/><path d="M12 11v6M12 7.5v.01"/>', clock: '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
  edit: '<path d="M4 20h4L19 9l-4-4L4 16v4zM13 7l4 4"/>', trash: '<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13"/>',
  copy: '<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h8"/>',
  shield: '<path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"/>', key: '<circle cx="8" cy="15" r="4"/><path d="M11 12l9-9M16 7l3 3M14 9l2 2"/>',
  lock: '<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>', warning: '<path d="M12 3l10 18H2zM12 10v5M12 18v.01"/>',
  off: '<circle cx="12" cy="12" r="9"/><path d="M5.6 5.6l12.8 12.8"/>',
  qr: '<path d="M4 4h6v6H4zM14 4h6v6h-6zM4 14h6v6H4zM14 14h2v2h-2zM18 14h2v2M14 18h2v2h4v-2"/>',
  scan: '<path d="M4 8V6a2 2 0 0 1 2-2h2M16 4h2a2 2 0 0 1 2 2v2M20 16v2a2 2 0 0 1-2 2h-2M8 20H6a2 2 0 0 1-2-2v-2M8 12h8"/>',
  sun: '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M5 5l1.5 1.5M17.5 17.5L19 19M5 19l1.5-1.5M17.5 6.5L19 5"/>',
  moon: '<path d="M20 14.5A8 8 0 0 1 9.5 4 8 8 0 1 0 20 14.5z"/>',
  leave: '<path d="M10 4H5v16h5M15 8l4 4-4 4M19 12H9"/>', devices: '<path d="M3 6h13v9H3zM1 18h17M18 9h4v9h-4z"/>',
};
CUR.call = CUR.phone;
CUR.callend = `<g transform="rotate(135 12 12)">${CUR.phone}</g>`;
CUR.voloff = CUR.vol.replace('M16.5 9a4 4 0 0 1 0 6', 'M16 10l4 4M20 10l-4 4');

/* Kept primitives (no redraw), and the alarm glyphs this direction retires (sign-up mood: mellow, no alarms). */
export const KEPT = ['chevr', 'chev', 'x', 'plus', 'check', 'search', 'clip', 'info', 'clock', 'edit', 'trash', 'key', 'lock', 'off', 'leave', 'devices'];
export const RETIRE: Record<string, string> = {
  warning: 'Retire. Hints and warnings speak in words (VOICE.md); no triangle.',
  shield: 'Retire. The empty chat gets the mark instead.',
};

export type Set = 'cur' | 'own' | 'gold';
export const body = (n: string, set: Set) =>
  set !== 'cur' && OWN[n] ? OWN[n].parts().map(([d, c]) => `<path${c ? ` class="${c}"` : ''} d="${d}"/>`).join('') : (CUR[n] ?? '');
