/* The one synthesized sound set. Generator ported from refined/call-audio.js (same patterns,
   frequencies and timings, which came from calls.html); driven ONLY by the fx bus, so each cue
   fires on the same event as its visual.
   Rules: no AudioContext before a user gesture; Quiet mode (drawer) never makes one; a hidden
   tab silences the current sound. The message-sent `pluck` exists but is not wired: "no sound on send"
   (chat.md) stands until decided otherwise. */
import { onFx } from './bus';
import { dur } from './motion';
import { motion } from './reduced.svelte';

type Note = [freq: number, at: number, dur: number, peak: number, attack: number];
interface Pattern { label: string; period: number; notes: Note[] }

export const PAT: Record<string, Pattern> = {
  ring: { label: 'Ringtone', period: 3200, notes: [[784, 0, 1, .5, .02], [988, .18, 1, .45, .02], [1175, .36, 1, .4, .02], [1568, .7, 1.4, .3, .02]] },
  back: { label: 'Ringback', period: 4000, notes: [[392, 0, 1.2, .4, .18], [494, 0, 1.2, .35, .18]] },
  connected: { label: 'Connected', period: 0, notes: [[523, 0, .5, .45, .02], [784, .12, .7, .45, .02]] },
  ended: { label: 'Ended', period: 0, notes: [[784, 0, .45, .4, .02], [523, .12, .7, .4, .02]] },
  failed: { label: 'Couldn’t connect', period: 0, notes: [[247, 0, .5, .45, .02], [196, .24, .8, .45, .02]] },
  pulse: { label: 'Reconnecting / joined', period: 0, notes: [[330, 0, .6, .35, .02]] },
  pluck: { label: 'Sent (not wired)', period: 0, notes: [[659, 0, .09, .5, .004]] },
};

/** Offset (s) of the connected cue's second note: the mark's string should reach taut here. */
export const CONNECT_HIT_S = 0.12;

interface Playing { kind: string; g: GainNode; lp: BiquadFilterNode; oscs: OscillatorNode[]; timers: number[]; stopped: boolean }

class Audio {
  ctx: AudioContext | null = null;
  vol = 0.5;
  quiet = true;           // on unless the person turns sound on in the drawer
  armed = false;
  cur: Playing | null = null;
  played: string[] = [];  // for tests

  arm() {
    this.armed = true;
    if (!this.quiet) this.ensure();
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
    if (!this.armed || this.quiet) return false;
    this.stop();
    if (!this.ensure()) return false;
    const c = this.ctx!, g = c.createGain(), lp = c.createBiquadFilter();
    lp.type = 'lowpass'; lp.frequency.value = 3200;
    g.gain.value = this.vol * 0.25 * gain;
    lp.connect(g); g.connect(c.destination);
    const s: Playing = { kind, g, lp, oscs: [], timers: [], stopped: false };
    this.cur = s;
    this.played.push(kind);
    const p = PAT[kind];
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
  tone(s: Playing, [f, at, dur, peak, atk]: Note, t: number) {
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
    if (!s || s.stopped || !this.ctx) return;
    s.stopped = true; s.timers.forEach(clearTimeout);
    const now = this.ctx.currentTime;
    s.g.gain.cancelScheduledValues(now);
    s.g.gain.setTargetAtTime(0, now, 0.012);
    for (const o of s.oscs) { try { o.stop(now + 0.08); } catch { /* not started */ } }
    setTimeout(() => { try { s.g.disconnect(); s.lp.disconnect(); } catch { /* gone */ } }, 200);
    if (this.cur === s) this.cur = null;
  }
  setQuiet(q: boolean) {
    this.quiet = q;
    if (q) this.stop(); else this.arm();
  }
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
      case 'ring': if (e.on) audio.play('back', true); else if (audio.cur?.kind === 'back') audio.stop(); break;
      case 'incoming': if (e.on) audio.play('ring', true); else if (audio.cur?.kind === 'ring') audio.stop(); break;
      // The cue's second note lands when the string goes taut: delay by (hit − second-note offset).
      case 'connect': {
        const lead = motion.reduced ? 0 : Math.max(0, dur('markHit') - CONNECT_HIT_S * 1000);
        if (lead) setTimeout(() => audio.play('connected'), lead); else audio.play('connected');
        break;
      }
      case 'end': audio.play('ended'); break;
      case 'failed': audio.play('failed'); break;
      case 'reconnecting': audio.play('pulse'); break;
      case 'join': audio.play('pulse', false, e.soft ? 0.4 : 1); break;
    }
  });
}
