/* The 1:1 voice call machine (PLAN step 3). One source of truth for the call; it drives the fx bus
   (sound + the logo mark + the cloud field all follow the same events) and writes ONE CallRecord into
   the chat thread when the call ends.

   idle -> outgoing | incoming -> connecting -> active (<-> reconnecting) -> ended | failed -> idle

   Sound comes only from the bus. The simulated "other side" is driven by remote()/drop()/recover()
   from the Calls section of the Simulation drawer. */
import { emitFx, fxLog } from '../fx/bus';
import { audio } from '../fx/audio';
import { dur, setTimeScale } from '../fx/motion';
import { db, person, chat, pushMessage } from '../model/store.svelte';
import type { CallRecord } from '../model/types';
import { nav } from '../nav/router.svelte';
import './cues';

export type CallPhase = 'idle' | 'incoming' | 'outgoing' | 'connecting' | 'active' | 'reconnecting' | 'ended' | 'failed';
export type Outcome = CallRecord['outcome'];

/** Simulated network/people timings in real ms (not motion durations). Mutable so tests and the drawer can shorten them.
    Proposed, not decided: the ring timeout (how long a call rings before "no answer" / "Couldn't connect"). */
export const CALL_MS = {
  ringTimeout: 20_000,      // Proposed
  ringingAfter: 1_100,      // outgoing shows "Calling…" until their device answers the signal, then "Ringing…"
  connect: 700,             // answered -> media path up
  reconnectTimeout: 15_000, // reconnecting -> gives up
  remoteEndHold: 900,       // "Call ended" stays readable when THEY end it
  failedHold: 2_600,        // "Couldn't connect" stays readable
};

export type Refusal = 'busy' | 'blocked' | 'messages-only' | 'unknown';

/** Told after every call record is written (system.svelte.ts turns a missed one into a notification when the app is not in front). */
export const onRecord = new Set<(id: string, direction: 'out' | 'in', outcome: Outcome) => void>();

class Call {
  phase = $state<CallPhase>('idle');
  peer = $state<string | null>(null);
  direction = $state<'out' | 'in' | null>(null);
  startedAt = $state(0);            // sim-clock ms when the call began (ringing)
  muted = $state(false);
  speaker = $state<'earpiece' | 'speaker'>('earpiece');
  /** 'full' = call screen; 'strip' = minimized strip. While incoming, 'strip' means the heads-up banner/card. */
  view = $state<'full' | 'strip'>('full');
  endReason = $state<string | null>(null);
  ringing = $state(false);          // outgoing: their device is ringing (vs. still reaching it)
  fading = $state(false);           // the ended/failed screen is leaving
  elapsedS = $state(0);

  #timers = new Set<number>();
  #connectedMono = 0;
  #recorded = true;
  #tick = 0;

  /* ---------- rules ---------- */
  refusal(id: string): Refusal | null {
    const p = person(id);
    if (!p) return 'unknown';
    if (p.blocked) return 'blocked';
    if (!p.callsAllowed) return 'messages-only';
    if (this.phase !== 'idle') return 'busy';
    return null;
  }
  canCall(id: string) { const r = this.refusal(id); return r === null || r === 'busy'; }

  get active() { return this.phase !== 'idle'; }
  get statusText() {
    switch (this.phase) {
      case 'incoming': return 'Incoming call';
      case 'outgoing': return this.ringing ? 'Ringing…' : 'Calling…';
      case 'connecting': return 'Connecting…';
      case 'reconnecting': return 'Reconnecting…';
      case 'ended': return 'Call ended';
      case 'failed': return 'Couldn\'t connect';
      default: return '';
    }
  }
  /** Which cloud-field state to show. */
  get fieldState() { return this.phase === 'idle' ? 'calm' : this.phase; }

  /* ---------- starting ---------- */
  place(id: string): boolean {
    const r = this.refusal(id);
    if (r === 'busy') { if (this.peer === id) this.view = 'full'; return false; }
    if (r) return false;
    this.#begin(id, 'out');
    this.phase = 'outgoing'; this.view = 'full'; this.ringing = false;
    emitFx({ type: 'ring', on: true });
    const p = person(id)!;
    const reachable = p.online && db.env.networkOnline;
    if (reachable) this.#later(CALL_MS.ringingAfter, () => { if (this.phase === 'outgoing') this.ringing = true; });
    this.#later(CALL_MS.ringTimeout, () => {
      if (this.phase !== 'outgoing') return;
      if (reachable) this.#finish('missed', 'local-timeout', 'no-answer'); else this.#finish('failed', 'failed', 'unreachable');
    });
    return true;
  }

  /** Someone calls me. Blocked callers and message-only contacts never reach the screen (silently dropped). */
  receive(id: string): boolean {
    // The account key is not on this device until it is unlocked: nothing can ring (and nothing is recorded).
    if (db.account.gate !== 'ready') return false;
    const r = this.refusal(id);
    if (r === 'busy') { this.#recordFor(id, 'in', 'missed', 0); return false; }
    if (r) return false;
    this.#begin(id, 'in');
    this.phase = 'incoming'; this.view = 'strip';
    emitFx({ type: 'incoming', on: true });
    this.#later(CALL_MS.ringTimeout, () => { if (this.phase === 'incoming') this.#finish('missed', 'remote', 'no-answer'); });
    return true;
  }

  /* ---------- my actions ---------- */
  answer() {
    if (this.phase !== 'incoming') return;
    this.#leave();
    this.phase = 'connecting'; this.view = 'full';
    this.#later(CALL_MS.connect, () => this.#connected());
  }
  decline() { if (this.phase === 'incoming') this.#finish('declined', 'local', 'declined'); }
  /** The End button: cancels ringing, declines an incoming call, or hangs up. */
  hangup() {
    switch (this.phase) {
      case 'incoming': this.decline(); break;
      case 'outgoing': this.#finish('cancelled', 'local', 'cancelled'); break;
      case 'connecting': case 'active': case 'reconnecting':
        this.#finish(this.#connectedMono ? 'answered' : this.direction === 'in' ? 'declined' : 'cancelled', 'local', 'hangup'); break;
    }
  }
  toggleMute() { this.muted = !this.muted; }
  toggleSpeaker() { this.speaker = this.speaker === 'speaker' ? 'earpiece' : 'speaker'; }
  minimize() { if (this.phase !== 'idle') this.view = 'strip'; }
  restore() { if (this.phase !== 'idle') this.view = 'full'; }

  /* ---------- the simulated other side ---------- */
  remote(action: 'answer' | 'decline' | 'noanswer' | 'busy' | 'hangup' | 'cancel') {
    switch (action) {
      case 'answer':
        if (this.phase !== 'outgoing') return;
        this.#leave(); this.phase = 'connecting';
        this.#later(CALL_MS.connect, () => this.#connected());
        break;
      case 'decline': if (this.phase === 'outgoing') this.#finish('declined', 'remote', 'declined'); break;
      case 'busy': if (this.phase === 'outgoing') this.#finish('declined', 'remote', 'busy'); break;
      case 'noanswer': if (this.phase === 'outgoing') this.#finish('missed', 'remote', 'no-answer'); break;
      case 'hangup':
        if (this.phase === 'active' || this.phase === 'reconnecting' || this.phase === 'connecting') this.#finish(this.#connectedMono ? 'answered' : 'cancelled', 'remote', 'hangup');
        break;
      case 'cancel': if (this.phase === 'incoming') this.#finish('cancelled', 'remote', 'cancelled'); break;
    }
  }
  drop() {
    if (this.phase !== 'active') return;
    this.phase = 'reconnecting';
    emitFx({ type: 'reconnecting' });
    this.#later(CALL_MS.reconnectTimeout, () => { if (this.phase === 'reconnecting') this.#finish('answered', 'failed', 'dropped'); });
  }
  recover() {
    if (this.phase !== 'reconnecting') return;
    this.phase = 'active';
    emitFx({ type: 'connect' });
  }
  /** Give up while reconnecting (the network never came back). */
  dropForGood() { if (this.phase === 'reconnecting') this.#finish('answered', 'failed', 'dropped'); }

  /** Compatibility with the earlier Core drawer buttons ("Call signal"). */
  set(next: CallPhase, peer: string | null = this.peer) {
    const id = peer ?? 'arjun';
    if (next === 'idle') this.hangup();
    else if (next === 'incoming') this.receive(id);
    else if (next === 'outgoing') this.place(id);
    else if (next === 'active') { if (this.phase === 'incoming') this.answer(); else this.remote('answer'); }
  }

  /* ---------- internals ---------- */
  #later(ms: number, fn: () => void) {
    const id = window.setTimeout(() => { this.#timers.delete(id); fn(); }, ms);
    this.#timers.add(id);
  }
  #clearTimers() { this.#timers.forEach(clearTimeout); this.#timers.clear(); clearInterval(this.#tick); }
  #begin(id: string, dir: 'out' | 'in') {
    this.#clearTimers();
    this.peer = id; this.direction = dir; this.endReason = null; this.muted = false; this.speaker = 'earpiece';
    this.fading = false; this.elapsedS = 0; this.#connectedMono = 0; this.#recorded = false;
    this.startedAt = Date.now();
  }
  /** Leaving ringing: stop the loop that was playing for it. */
  #leave() {
    if (this.phase === 'incoming') emitFx({ type: 'incoming', on: false });
    if (this.phase === 'outgoing') emitFx({ type: 'ring', on: false });
  }
  #connected() {
    if (this.phase !== 'connecting') return;
    this.phase = 'active';
    this.#connectedMono = performance.now();
    this.elapsedS = 0;
    clearInterval(this.#tick);
    this.#tick = window.setInterval(() => {
      const s = Math.floor((performance.now() - this.#connectedMono) / 1000);
      if (s !== this.elapsedS) this.elapsedS = s;
    }, 250);
    emitFx({ type: 'connect' });
  }
  #finish(outcome: Outcome, who: 'local' | 'remote' | 'local-timeout' | 'failed', reason: string) {
    if (this.phase === 'idle' || this.phase === 'ended' || this.phase === 'failed') return;
    const id = this.peer!, dir = this.direction!;
    const seconds = this.#connectedMono ? Math.floor((performance.now() - this.#connectedMono) / 1000) : 0;
    this.#clearTimers();
    this.#leave();
    if (!this.#recorded) { this.#recorded = true; this.#recordFor(id, dir, outcome, seconds); }
    this.endReason = reason;
    this.ringing = false;
    if (who === 'failed') { this.phase = 'failed'; emitFx({ type: 'failed' }); }
    else { this.phase = 'ended'; emitFx({ type: 'end' }); }
    const hold = who === 'local' ? 0 : who === 'failed' ? CALL_MS.failedHold : CALL_MS.remoteEndHold;
    if (hold) this.#later(hold, () => (this.fading = true)); else this.fading = true;
    this.#later(hold + dur('callEnd'), () => this.#reset());
  }
  #reset() {
    this.#clearTimers();
    this.phase = 'idle'; this.peer = null; this.direction = null; this.view = 'full'; this.fading = false;
    this.endReason = null; this.ringing = false; this.muted = false; this.speaker = 'earpiece'; this.elapsedS = 0;
  }
  /** The ONE record per call. Unread only for a missed incoming call (Proposed). */
  #recordFor(id: string, direction: 'out' | 'in', outcome: Outcome, durationS: number) {
    const c = chat(id);
    if (!c || c.kind !== 'direct') return;
    pushMessage(c, { from: direction === 'out' ? 'me' : c.person, text: '', call: { kind: 'voice', outcome, durationS, direction } });
    if (direction === 'in' && (outcome === 'missed' || outcome === 'cancelled') && nav.openChat !== c.id) c.unread++;
    onRecord.forEach((fn) => fn(id, direction, outcome));
  }
}

export const call = new Call();

export const fmtDuration = (s: number) => {
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), ss = s % 60;
  return (h ? h + ':' + String(m).padStart(2, '0') : String(m).padStart(2, '0')) + ':' + String(ss).padStart(2, '0');
};

/** Compact m:ss (h:mm:ss) for records and lists; the call screen timer uses mm:ss. */
export const fmtShort = (s: number) => {
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), ss = String(s % 60).padStart(2, '0');
  return h ? `${h}:${String(m).padStart(2, '0')}:${ss}` : `${m}:${ss}`;
};

/** Test hook: lets the Playwright flows read the machine, the store and the cue log. Not used by the app. */
if (typeof window !== 'undefined') (window as any).__pg = { call, db, fxLog, audio, CALL_MS, setTimeScale };
