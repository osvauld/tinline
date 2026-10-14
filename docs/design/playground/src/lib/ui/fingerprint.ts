/* Fingerprint avatars ("sacred geometry"): TypeScript port of crates/core/src/avatar.rs. Spec: docs/design/FINGERPRINT-AVATARS.md.
   Must reproduce crates/core/src/avatar_golden.txt (checked by tools/avatar/golden.mjs). No dependencies: a small BLAKE3
   (derive_key for inputs up to one 1024-byte chunk, which is all we need) lives at the bottom.
   Recognition, not verification. */

export interface Stroke { points: number[]; closed: boolean; gold: boolean; alpha: number }
export interface Colours { bg: string; fg: string; bgArgb: number; fgArgb: number }
export interface AvatarArt { strokes: Stroke[]; strokeWidth: number; light: Colours; dark: Colours }

const CTX = 'tinline/avatar/v1';
const C = 50, R = 42, CIRCLE_SEGS = 48, ARC_SEGS = 24;
export const SMALL_BELOW_PX = 40;
const STROKE = 1.8, STROKE_SMALL = 3.4;

type Path = [number, number][];
type Piece = { layer: number; alpha: number; pts: Path; closed: boolean };

const gcd = (a: number, b: number): number => (b ? gcd(b, a % b) : a);
const pt = (r: number, i: number, m: number, off: number): [number, number] => {
  const a = ((i / m) * 360 + off - 90) * (Math.PI / 180);
  return [C + r * Math.cos(a), C + r * Math.sin(a)];
};
const poly = (r: number, m: number, step: number, off: number): Path[] => {
  const g = gcd(m, step), out: Path[] = [];
  for (let s = 0; s < g; s++) { const p: Path = []; for (let j = 0; j < m / g; j++) p.push(pt(r, (s + j * step) % m, m, off)); out.push(p); }
  return out;
};
const circle = (cx: number, cy: number, r: number): Path =>
  Array.from({ length: CIRCLE_SEGS }, (_, i) => { const a = (i / CIRCLE_SEGS) * 2 * Math.PI; return [cx + r * Math.cos(a), cy + r * Math.sin(a)] as [number, number]; });
/** SVG `A r r 0 0 1` from p0 to p1, sampled, end point excluded. */
function arc(p0: [number, number], p1: [number, number], r: number): Path {
  const dx = p1[0] - p0[0], dy = p1[1] - p0[1], d = Math.sqrt(dx * dx + dy * dy);
  const h = Math.sqrt(Math.max(0, r * r - (d * d) / 4)), ux = dx / d, uy = dy / d;
  const c = [(p0[0] + p1[0]) / 2 - h * uy, (p0[1] + p1[1]) / 2 + h * ux];
  const t0 = Math.atan2(p0[1] - c[1], p0[0] - c[0]), t1 = Math.atan2(p1[1] - c[1], p1[0] - c[0]);
  let dt = t1 - t0;
  while (dt < 0) dt += 2 * Math.PI;
  return Array.from({ length: ARC_SEGS }, (_, i) => { const t = t0 + (dt * i) / ARC_SEGS; return [c[0] + r * Math.cos(t), c[1] + r * Math.sin(t)] as [number, number]; });
}

export function params(b: ArrayLike<number>) {
  const n = 5 + (b[1] % 12), kmax = Math.floor((n - 1) / 2);
  return {
    hue: (b[0] / 256) * 360, n, k: 2 + (b[2] % (kmax - 1)), cr: 8 + (b[3] % 8), cd: 12 + (b[4] % 10),
    innerKind: b[5] % 3, ir: 6 + ((b[5] >> 2) % 4) * 2, extra: b[6] % 4, gold: b[7] % 4, rot: (b[8] / 256) * (360 / n),
  };
}

function layers(p: ReturnType<typeof params>, small: boolean): Piece[] {
  const { n } = p, phi = (1 + Math.sqrt(5)) / 2, half = 180 / n, out: Piece[] = [];
  const add = (layer: number, alpha: number, paths: Path[], closed = true) => { for (const pts of paths) out.push({ layer, alpha, pts, closed }); };
  add(0, 1, poly(R, n, p.k, 0));
  const ring = Array.from({ length: n }, (_, i) => pt(p.cd, i, n, half));
  if (!small) add(1, 0.75, ring.map((c) => circle(c[0], c[1], p.cr)));
  if (p.innerKind === 0) add(2, 1, [circle(C, C, p.ir)]);
  else if (p.innerKind === 1) add(2, 1, poly(p.ir, n, 1, half));
  else add(2, 1, poly(p.ir, n, Math.max(2, Math.floor((n - 1) / 2)), half));
  if (p.extra === 1) {
    add(3, 0.7, poly(p.cd, n, 1, half));
    add(3, 0.7, ring.map((c) => [[C, C], c] as Path), false);
  } else if (p.extra === 2) {
    const pr = p.cd * 0.9;
    for (let i = 0; i < n; i++) { const tip = pt(pr, i, n, 0); add(3, 0.7, [[...arc([C, C], tip, pr * 0.62), ...arc(tip, [C, C], pr * 0.62)]]); }
  } else if (p.extra === 3) {
    add(3, 0.7, [circle(C, C, R / phi), circle(C, C, R / (phi * phi))]);
  }
  return out;
}

function oklch(l: number, c: number, hDeg: number): number {
  const h = (hDeg * Math.PI) / 180, a = c * Math.cos(h), b = c * Math.sin(h);
  const l_ = l + 0.3963377774 * a + 0.2158037573 * b, m_ = l - 0.1055613458 * a - 0.0638541728 * b, s_ = l - 0.0894841775 * a - 1.291485548 * b;
  const l3 = l_ ** 3, m3 = m_ ** 3, s3 = s_ ** 3;
  const lin = [4.0767416621 * l3 - 3.3077115913 * m3 + 0.2309699292 * s3, -1.2684380046 * l3 + 2.6097574011 * m3 - 0.3413193965 * s3, -0.0041960863 * l3 - 0.7034186147 * m3 + 1.707614701 * s3];
  const ch = (x: number) => { x = Math.min(1, Math.max(0, x)); const v = x <= 0.0031308 ? 12.92 * x : 1.055 * Math.pow(x, 1 / 2.4) - 0.055; return Math.round(v * 255); };
  return (0xff000000 | (ch(lin[0]) << 16) | (ch(lin[1]) << 8) | ch(lin[2])) >>> 0;
}
const css = (argb: number) => '#' + (argb & 0xffffff).toString(16).padStart(6, '0');
function colour(l1: number, c1: number, l2: number, c2: number, h: number): Colours {
  const bgArgb = oklch(l1, c1, h), fgArgb = oklch(l2, c2, h);
  return { bg: css(bgArgb), fg: css(fgArgb), bgArgb, fgArgb };
}

export function artFromFingerprint(b: ArrayLike<number>, small: boolean): AvatarArt {
  const p = params(b), rad = (p.rot * Math.PI) / 180, s = Math.sin(rad), c = Math.cos(rad);
  const strokes = layers(p, small).map((q) => ({
    points: q.pts.flatMap(([x, y]) => { const dx = x - C, dy = y - C; return [C + dx * c - dy * s, C + dx * s + dy * c]; }),
    closed: q.closed, gold: q.layer === p.gold, alpha: q.alpha,
  }));
  return { strokes, strokeWidth: small ? STROKE_SMALL : STROKE, light: colour(0.94, 0.035, 0.42, 0.09, p.hue), dark: colour(0.34, 0.05, 0.88, 0.07, p.hue) };
}

/** SVG path data for a stroke (the playground draws with <path>). */
export const strokePath = (s: Stroke): string => {
  let d = '';
  for (let i = 0; i < s.points.length; i += 2) d += (i ? 'L' : 'M') + s.points[i].toFixed(2) + ',' + s.points[i + 1].toFixed(2);
  return s.closed ? d + 'Z' : d;
};

/* ---- DID <-> key ---- */
const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';
function b58decode(s: string): number[] | null {
  const out: number[] = [];
  for (const ch of s) {
    let carry = B58.indexOf(ch);
    if (carry < 0) return null;
    for (let i = 0; i < out.length; i++) { carry += out[i] * 58; out[i] = carry & 255; carry >>= 8; }
    while (carry) { out.push(carry & 255); carry >>= 8; }
  }
  for (const ch of s) { if (ch !== '1') break; out.push(0); }
  return out.reverse();
}
export function b58encode(bytes: ArrayLike<number>): string {
  const digits: number[] = [];
  for (let k = 0; k < bytes.length; k++) {
    let carry = bytes[k];
    for (let i = 0; i < digits.length; i++) { carry += digits[i] << 8; digits[i] = carry % 58; carry = (carry / 58) | 0; }
    while (carry) { digits.push(carry % 58); carry = (carry / 58) | 0; }
  }
  let s = '';
  for (let k = 0; k < bytes.length && bytes[k] === 0; k++) s += '1';
  return s + digits.reverse().map((d) => B58[d]).join('');
}
/** The 32-byte Ed25519 key inside `did:key:z6Mk…`, or null. */
export function publicKeyFromDid(did: string): number[] | null {
  if (!did.startsWith('did:key:z')) return null;
  const b = b58decode(did.slice('did:key:z'.length));
  return b && b.length === 34 && b[0] === 0xed && b[1] === 0x01 ? b.slice(2) : null;
}
export const didFromPublicKey = (pk: ArrayLike<number>): string => 'did:key:z' + b58encode([0xed, 0x01, ...Array.from(pk)]);
/** Synthetic DID for a seed person id (playground only: a stand-in key derived from the id). */
export const syntheticDid = (id: string): string => didFromPublicKey(blake3Hash(new TextEncoder().encode('tinline/playground/did/' + id)));

export function fingerprint(did: string): number[] | null {
  const pk = publicKeyFromDid(did);
  return pk && blake3DeriveKey(CTX, pk);
}
const cache = new Map<string, AvatarArt | null>();
/** Art for a DID (cached per DID and size class). null for a malformed DID. */
export function avatarArt(did: string, small: boolean): AvatarArt | null {
  const key = (small ? 's' : 'l') + did;
  if (!cache.has(key)) { const f = fingerprint(did); cache.set(key, f && artFromFingerprint(f, small)); }
  return cache.get(key) ?? null;
}

/* ---- BLAKE3 (single chunk, <= 1024 bytes) ---- */
const IV = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
const PERM = [2, 6, 3, 10, 7, 0, 4, 13, 1, 11, 12, 5, 9, 14, 15, 8];
const START = 1, END = 2, ROOT = 8, KEY_CONTEXT = 32, KEY_MATERIAL = 64;
const rotr = (x: number, n: number) => ((x >>> n) | (x << (32 - n))) >>> 0;
function compress(cv: number[], block: number[], blockLen: number, flags: number): number[] {
  const v = [...cv, IV[0], IV[1], IV[2], IV[3], 0, 0, blockLen, flags];
  let m = block.slice();
  const g = (a: number, b: number, c: number, d: number, x: number, y: number) => {
    v[a] = (v[a] + v[b] + x) >>> 0; v[d] = rotr(v[d] ^ v[a], 16);
    v[c] = (v[c] + v[d]) >>> 0; v[b] = rotr(v[b] ^ v[c], 12);
    v[a] = (v[a] + v[b] + y) >>> 0; v[d] = rotr(v[d] ^ v[a], 8);
    v[c] = (v[c] + v[d]) >>> 0; v[b] = rotr(v[b] ^ v[c], 7);
  };
  for (let r = 0; r < 7; r++) {
    g(0, 4, 8, 12, m[0], m[1]); g(1, 5, 9, 13, m[2], m[3]); g(2, 6, 10, 14, m[4], m[5]); g(3, 7, 11, 15, m[6], m[7]);
    g(0, 5, 10, 15, m[8], m[9]); g(1, 6, 11, 12, m[10], m[11]); g(2, 7, 8, 13, m[12], m[13]); g(3, 4, 9, 14, m[14], m[15]);
    if (r < 6) m = PERM.map((i) => m[i]);
  }
  return Array.from({ length: 8 }, (_, i) => (v[i] ^ v[i + 8]) >>> 0);
}
function chunk(input: ArrayLike<number>, key: number[], flags: number): number[] {
  if (input.length > 1024) throw new Error('blake3: single chunk only');
  let cv = key;
  const blocks = Math.max(1, Math.ceil(input.length / 64));
  for (let j = 0; j < blocks; j++) {
    const len = Math.min(64, input.length - j * 64), words: number[] = new Array(16).fill(0);
    for (let i = 0; i < len; i++) words[i >> 2] |= input[j * 64 + i] << (8 * (i & 3));
    cv = compress(cv, words.map((w) => w >>> 0), len, flags | (j === 0 ? START : 0) | (j === blocks - 1 ? END | ROOT : 0));
  }
  return cv;
}
const toBytes = (w: number[]) => w.flatMap((x) => [x & 255, (x >>> 8) & 255, (x >>> 16) & 255, x >>> 24]);
export const blake3Hash = (input: ArrayLike<number>): number[] => toBytes(chunk(input, IV, 0));
export function blake3DeriveKey(context: string, material: ArrayLike<number>): number[] {
  const ctx = chunk(new TextEncoder().encode(context), IV, KEY_CONTEXT);
  return toBytes(chunk(material, ctx, KEY_MATERIAL));
}
