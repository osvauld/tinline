/* The one synthesized sound set (a MOCK: the apps generate the same sounds natively from these tables; see
   SIGNUP-HANDOFF.md § Sounds). Generator ported from refined/call-audio.js; driven ONLY by the fx bus, so each
   cue fires on the same event as its visual.
   Character: Tinline is a thoughtful toy. Three instruments, all made in code, no sample files:
   `sine` (the call chimes), `pluck` (the line: a soft plucked string, Karplus-Strong) and `tok` (the can:
   a small struck tin, a few inharmonic partials). Each pluck/tok is a little different every time.
   Sparse on purpose: a sound only where it tells you something (calls, the line opening, someone asking to
   add you, a message arriving). Sending is a faint pluck; typing, steps and taps are silent.
   Rules: no AudioContext before a user gesture; Quiet mode (drawer) never makes one; a hidden
   tab silences the current sound. One-shot cues (`cue()`) layer over a ringing call instead of stopping it.
   Palettes: `classic` is the set above. `slack` and `triad` are the two signature candidates from the
   sound-identity branch (docs/sound/ringtone-studio.html), same G-major note set; they replace only
   ring/back/connected. Proposed, undecided: the drawer lets you audition them in the real flows. */
import { onFx } from './bus';
import { dur } from './motion';
import { motion } from './reduced.svelte';

type Inst = 'sine' | 'pluck' | 'tok';
type Note = [freq: number, at: number, dur: number, peak: number, attack: number, inst?: Inst];
interface Pattern { label: string; period: number; notes: Note[] }

export const PAT: Record<string, Pattern> = {
  ring: { label: 'Ringtone', period: 3200, notes: [[784, 0, 1, .5, .02], [988, .18, 1, .45, .02], [1175, .36, 1, .4, .02], [1568, .7, 1.4, .3, .02]] },
  back: { label: 'Ringback', period: 4000, notes: [[392, 0, 1.2, .4, .18], [494, 0, 1.2, .35, .18]] },
  connected: { label: 'Connected', period: 0, notes: [[523, 0, .5, .45, .02], [784, .12, .7, .45, .02]] },
  ended: { label: 'Ended', period: 0, notes: [[784, 0, .45, .4, .02], [523, .12, .7, .4, .02]] },
  failed: { label: 'Couldn’t connect', period: 0, notes: [[247, 0, .5, .45, .02], [196, .24, .8, .45, .02]] },
  pulse: { label: 'Reconnecting / joined', period: 0, notes: [[330, 0, .6, .35, .02]] },
  waiting: { label: 'Call waiting', period: 3000, notes: [[784, 0, .16, .22, .012], [784, .22, .2, .22, .012]] },
  // Group calls (step 6): soft single/dual tones, much quieter than any ring; they only ever play as cues, never loop.
  gstart: { label: 'Group call started', period: 0, notes: [[587, 0, .3, .3, .02], [784, .14, .5, .3, .02]] },
  gjoin: { label: 'Joined (group)', period: 0, notes: [[659, 0, .32, .3, .02]] },
  gleft: { label: 'Left (group)', period: 0, notes: [[494, 0, .36, .26, .02]] },
  // The toy set. sent: one faint high pluck. received: the can, tapped once. line: the line pulls tight in two
  // tugs (Done, contact added), D5 then G5 with a soft G4 under the second. knock: someone asks to add you.
  sent: { label: 'Message sent', period: 0, notes: [[1175, 0, .5, .18, .003, 'pluck']] },
  received: { label: 'Message received', period: 0, notes: [[880, 0, .45, .32, .002, 'tok']] },
  line: { label: 'Line open', period: 0, notes: [[587, 0, 1.4, .55, .003, 'pluck'], [784, .12, 1.8, .55, .003, 'pluck'], [392, .12, 1.6, .12, .06]] },
  knock: { label: 'Asks to add you', period: 0, notes: [[640, 0, .35, .34, .002, 'tok'], [600, .17, .45, .3, .002, 'tok']] },
};

export type Palette = 'classic' | 'slack' | 'triad';
export const PALETTES: Record<Palette, { label: string; hit: number; cues: Partial<Record<string, Pattern>> }> = {
  classic: { label: 'Classic chime', hit: 0.12, cues: {} },
  // Slack → taut: two G4s 4 Hz apart beat like a slack string; D5 is the string pulled tight.
  slack: { label: 'Slack → taut', hit: 0, cues: {
    ring: { label: 'Ringtone', period: 2600, notes: [[392, 0, .9, .35, .08], [396, 0, .9, .3, .08], [587, 1.05, .45, .5, .015]] },
    back: { label: 'Ringback', period: 2200, notes: [[392, 0, 1.1, .32, .12], [396, 0, 1.1, .28, .12]] },
    connected: { label: 'Connected', period: 0, notes: [[587, 0, .5, .5, .015]] },
    waiting: { label: 'Call waiting', period: 3000, notes: [[587, 0, .16, .2, .012], [587, .22, .2, .22, .012]] },
    gstart: { label: 'Group call started', period: 0, notes: [[392, 0, .4, .28, .03], [587, .16, .55, .3, .03]] },
    gjoin: { label: 'Joined (group)', period: 0, notes: [[587, 0, .34, .26, .02]] },
    gleft: { label: 'Left (group)', period: 0, notes: [[392, 0, .4, .24, .03]] },
  } },
  triad: { label: 'Triad knock', hit: 0.14, cues: {
    ring: { label: 'Ringtone', period: 2400, notes: [[392, 0, .32, .45, .015], [494, .16, .32, .45, .015], [587, .32, .5, .5, .015]] },
    back: { label: 'Ringback', period: 3400, notes: [[392, 0, .4, .3, .1], [494, .22, .4, .3, .1], [587, .44, .55, .32, .1]] },
    connected: { label: 'Connected', period: 0, notes: [[392, 0, .3, .4, .02], [587, .14, .5, .5, .02]] },
    waiting: { label: 'Call waiting', period: 3000, notes: [[494, 0, .16, .2, .012], [587, .2, .2, .22, .012]] },
    gstart: { label: 'Group call started', period: 0, notes: [[494, 0, .28, .26, .02], [587, .12, .5, .3, .02]] },
    gjoin: { label: 'Joined (group)', period: 0, notes: [[587, 0, .3, .26, .02]] },
    gleft: { label: 'Left (group)', period: 0, notes: [[494, 0, .34, .24, .02]] },
  } },
};

const PALETTE_KEY = 'tinline.pg.palette';
const QUIET_KEY = 'tinline.pg.quiet';
const savedQuiet = (() => { try { return localStorage.getItem(QUIET_KEY) === '1'; } catch { return false; } })();
const savedPalette = (() => { try { return localStorage.getItem(PALETTE_KEY) as Palette | null; } catch { return null; } })();

interface Playing { kind: string; g: GainNode; lp: BiquadFilterNode; oscs: OscillatorNode[]; timers: number[]; stopped: boolean }

class Audio {
  ctx: AudioContext | null = null;
  vol = 0.5;
  quiet = savedQuiet;     // sound is on unless the person picks Quiet in the drawer (remembered)
  armed = false;
  pending: { kind: string; gain: number } | null = null; // a looping cue asked for before the first gesture
  cur: Playing | null = null;
  played: string[] = [];  // for tests
  palette: Palette = savedPalette && savedPalette in PALETTES ? savedPalette : 'classic';

  /** The pattern a cue plays in the current palette. Visuals (call background) read the same notes. */
  pattern(kind: string): Pattern { return PALETTES[this.palette].cues[kind] ?? PAT[kind]; }
  /** Offset (s) of the connect cue's landing note: the mark's string reaches taut here. */
  get hitS() { return PALETTES[this.palette].hit; }
  setPalette(p: Palette) {
    this.palette = p;
    try { localStorage.setItem(PALETTE_KEY, p); } catch { /* private mode */ }
  }

  arm() {
    const first = !this.armed;
    this.armed = true;
    if (this.quiet) return;
    this.ensure();
    // A ring that started before the browser allowed sound picks up at the first tap.
    if (first && this.pending && !this.cur) { const p = this.pending; this.pending = null; this.play(p.kind, true, p.gain); }
  }
  ensure() {
    if (!this.ctx) {
      const C = window.AudioContext || (window as any).webkitAudioContext;
      if (!C) return false;
      try { this.ctx = new C(); } catch { return false; }
    }
    if (this.ctx.state === 'suspended') this.ctx.resume();
    return true;
  }
  play(kind: string, loop = false, gain = 1) {
    if (!this.armed || this.quiet) { this.pending = loop && !this.quiet ? { kind, gain } : null; return false; }
    this.pending = null;
    this.stop();
    if (!this.ensure()) return false;
    const c = this.ctx!, g = c.createGain(), lp = c.createBiquadFilter();
    lp.type = 'lowpass'; lp.frequency.value = 3200;
    g.gain.value = this.vol * 0.25 * gain;
    lp.connect(g); g.connect(c.destination);
    const s: Playing = { kind, g, lp, oscs: [], timers: [], stopped: false };
    this.cur = s;
    this.played.push(kind);
    const p = this.pattern(kind);
    const cycle = () => {
      if (s.stopped) return;
      const t = c.currentTime + 0.03;
      for (const n of p.notes) this.tone(s, n, t);
      const wait = p.period || Math.max(...p.notes.map((n) => (n[1] + n[2]) * 1000)) + 80;
      s.timers.push(window.setTimeout(() => {
        if (s.stopped) return;
        if (loop && p.period) cycle(); else this.stop(s);
      }, wait));
    };
    cycle();
    return true;
  }
  /** A one-shot that layers over whatever is playing (a message arriving mid-ring doesn't cut the ring). */
  cue(kind: string, gain = 1) {
    if (!this.armed || this.quiet || !this.ensure()) return false;
    const c = this.ctx!, g = c.createGain(), lp = c.createBiquadFilter();
    lp.type = 'lowpass'; lp.frequency.value = 5000;
    g.gain.value = this.vol * 0.25 * gain;
    lp.connect(g); g.connect(c.destination);
    const s: Playing = { kind, g, lp, oscs: [], timers: [], stopped: false };
    this.played.push(kind);
    const p = this.pattern(kind), t = c.currentTime + 0.02;
    for (const n of p.notes) this.tone(s, n, t);
    const end = Math.max(...p.notes.map((n) => n[1] + n[2])) * 1000 + 200;
    setTimeout(() => { try { g.disconnect(); lp.disconnect(); } catch { /* gone */ } }, end);
    return true;
  }
  /** pluck / tok: rendered into a buffer at play time (cheap: well under a millisecond of work per note). */
  voice(s: Playing, [f, at, dur, peak, atk, inst]: Note, t: number) {
    const c = this.ctx!, t0 = t + at, human = Math.pow(2, (Math.random() - 0.5) * 8 / 1200);  // ±4 cents, never twice the same
    const buf = inst === 'pluck' ? pluckBuf(c, f * human, dur) : tokBuf(c, f * human, dur);
    const src = c.createBufferSource(), e = c.createGain();
    src.buffer = buf;
    e.gain.setValueAtTime(0, t0); e.gain.linearRampToValueAtTime(peak, t0 + atk);
    src.connect(e); e.connect(s.lp);
    src.onended = () => { try { e.disconnect(); } catch { /* gone */ } };
    src.start(t0); s.oscs.push(src as unknown as OscillatorNode);
  }
  tone(s: Playing, n: Note, t: number) {
    if (n[5] && n[5] !== 'sine') { this.voice(s, n, t); return; }
    const [f, at, dur, peak, atk] = n;
    const c = this.ctx!, t0 = t + at;
    for (const [freq, amt] of [[f, 1], [f * 2, 0.18]]) {
      const o = c.createOscillator(), e = c.createGain();
      o.type = 'sine'; o.frequency.value = freq;
      e.gain.setValueAtTime(0, t0);
      e.gain.linearRampToValueAtTime(peak * amt, t0 + atk);
      e.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
      o.connect(e); e.connect(s.lp);
      o.onended = () => { try { e.disconnect(); } catch { /* already gone */ } };
      o.start(t0); o.stop(t0 + dur + 0.05);
      s.oscs.push(o);
    }
  }
  stop(s: Playing | null = this.cur) {
    if (s === this.cur) this.pending = null;
    if (!s || s.stopped || !this.ctx) return;
    s.stopped = true; s.timers.forEach(clearTimeout);
    const now = this.ctx.currentTime;
    s.g.gain.cancelScheduledValues(now);
    s.g.gain.setTargetAtTime(0, now, 0.012);
    for (const o of s.oscs) { try { o.stop(now + 0.08); } catch { /* not started */ } }
    setTimeout(() => { try { s.g.disconnect(); s.lp.disconnect(); } catch { /* gone */ } }, 200);
    if (this.cur === s) this.cur = null;
  }
  /** A looping cue's state ended: stop it if it's playing, or forget it if it was waiting for the first tap. */
  end(kind: string) {
    if (this.pending?.kind === kind) this.pending = null;
    if (this.cur?.kind === kind) this.stop();
  }
  setQuiet(q: boolean) {
    this.quiet = q;
    try { localStorage.setItem(QUIET_KEY, q ? '1' : '0'); } catch { /* private mode */ }
    if (q) this.stop(); else this.arm();
  }
}

/* The line: Karplus-Strong. A burst of softened noise circulates in a delay one period long; averaging each pass
   is the string losing its highs. `soft` low-passes the burst (a fingertip, not a pick). Delay = m + frac + 0.5. */
function pluckBuf(c: BaseAudioContext, f: number, dur: number, soft = 0.45) {
  const sr = c.sampleRate, n = Math.ceil(sr * dur), b = c.createBuffer(1, n, sr), y = b.getChannelData(0);
  const N = sr / f - 0.5, m = Math.floor(N), g = N - m, decay = Math.pow(0.001, 1 / (f * dur));
  let lp = 0;
  for (let i = 0; i < Math.min(m + 2, n); i++) { lp += soft * (Math.random() * 2 - 1 - lp); y[i] = lp; }
  for (let i = m + 2; i < n; i++)
    y[i] = decay * 0.5 * ((1 - g) * (y[i - m] + y[i - m - 1]) + g * (y[i - m - 1] + y[i - m - 2]));
  return norm(b, y);
}
/* The can: a few inharmonic partials of a small struck tin (higher ones die first) and a 4ms tap. */
function tokBuf(c: BaseAudioContext, f: number, dur: number) {
  const sr = c.sampleRate, n = Math.ceil(sr * dur), b = c.createBuffer(1, n, sr), y = b.getChannelData(0);
  const P: [number, number, number][] = [[1, 1, 1], [1.51, .55, .55], [2.03, .35, .4], [2.47, .22, .28], [3.31, .12, .18]];
  for (const [r, a, d] of P) { const w = 2 * Math.PI * f * r / sr, k = 1 / (sr * dur * d / 6.9), ph = Math.random() * 6.28;
    for (let i = 0; i < n; i++) y[i] += a * Math.exp(-i * k) * Math.sin(w * i + ph); }
  let lp = 0; const tap = Math.floor(sr * 0.004);
  for (let i = 0; i < tap; i++) { lp += 0.3 * (Math.random() * 2 - 1 - lp); y[i] += 0.6 * lp * (1 - i / tap); }
  return norm(b, y);
}
function norm(b: AudioBuffer, y: Float32Array) {
  let mx = 0; for (let i = 0; i < y.length; i++) mx = Math.max(mx, Math.abs(y[i]));
  if (mx > 0) for (let i = 0; i < y.length; i++) y[i] /= mx;
  const fade = Math.min(256, y.length); for (let i = 0; i < fade; i++) y[y.length - 1 - i] *= i / fade;   // no click at the end
  return b;
}

export const audio = new Audio();

/** Wire the bus once at startup. Any pointer/key gesture arms audio (browsers need one). */
export function startAudio() {
  const arm = () => audio.arm();
  addEventListener('pointerdown', arm, { capture: true });
  addEventListener('keydown', arm, { capture: true });
  document.addEventListener('visibilitychange', () => { if (document.hidden) audio.stop(); });
  onFx((e) => {
    switch (e.type) {
      case 'ring': if (e.on) audio.play('back', true); else audio.end('back'); break;
      case 'incoming': if (e.on) audio.play('ring', true); else audio.end('ring'); break;
      // The cue's second note lands when the string goes taut: delay by (hit − second-note offset).
      case 'connect': {
        const lead = motion.reduced ? 0 : Math.max(0, dur('markHit') - audio.hitS * 1000);
        if (lead) setTimeout(() => audio.play('connected'), lead); else audio.play('connected');
        break;
      }
      // Soft and quieter than the ring; it loops over the call until the waiting call is resolved.
      case 'waiting': if (e.on) audio.play('waiting', true, 0.6); else audio.end('waiting'); break;
      case 'end': audio.play('ended'); break;
      case 'failed': audio.play('failed'); break;
      case 'reconnecting': audio.play('pulse'); break;
      case 'gcall': audio.play(e.cue === 'started' ? 'gstart' : e.cue === 'joined' ? 'gjoin' : 'gleft'); break;
      case 'join': audio.play('pulse', false, e.soft ? 0.4 : 1); break;
      case 'sent': audio.cue('sent'); break;
      case 'received': audio.cue('received'); break;
      case 'line': audio.cue('line'); break;
      case 'request': audio.cue('knock'); break;
    }
  });
}
