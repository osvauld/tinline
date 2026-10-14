/* The group call machine (PLAN step 6). Mirrors call.svelte.ts: one source of truth, drives the fx bus
   (sound, the logo mark and the cloud field follow the same events), writes ONE record into the group's
   thread when my part in the call ends. Spec: docs/design/group-call-spec.md §2, §5, §10-§11; decision U25.

   What the model carries from the spec
   - A call runs on a CALL HOST: a device holding its own permit (separate from chat hosting), accepted by that
     device. The STARTER picks one: candidates with a fresh status are ranked (tiers 0-4: desktop on mains first,
     phone on low battery or any metered host last; busy, thermal >= moderate, a phone under 30 % on battery are
     excluded), ties go to the starter's own device. A candidate may refuse (paused, busy, or a phone asked by
     someone else: Q11 "a phone hosts only calls its own user starts"); the starter then asks the next one.
     Nobody left: StartFailed, "No call host available".
   - The host forwards the K loudest speakers (K = 3 on a desktop, 2 on a phone, 1 when limited); the screen
     highlights those (hysteresis: a newcomer replaces one only if clearly louder for 200 ms).
   - Phone host caps: 8 people on Wi-Fi, 4 on mobile data; a desktop 12 (the launch cap). Over the cap: refused.
   - Host conditions (§2.4): battery < 30 % / thermal moderate / mobile data: stop advertising (running call goes on);
     battery < 15 % / thermal severe: K = 1 and new joins refused ("hosting limited"); battery < 8 % / thermal critical /
     the owner ends hosting: End{host_leaving, next_host} and everyone redials the backup at once.
   - Host lost (crash, network): ~3 s of missed heartbeats, then each client redials backup 1, backup 2. None
     answers: "Call ended — host lost" with Rejoin. A rotation due: End{host_rotating}, redial the same host.
   - 1:1 and group calls exclude each other: a 1:1 call arriving during a group call is turned away Busy (left as a
     missed call); placing or joining one while on the other is InCall.
   - No ringing: a started call is a quiet notice ("Sam started a call · Join"), and a soft cue. */
import { emitFx } from '../fx/bus';
import { dur } from '../fx/motion';
import { db, person, chat, pushMessage, firstName, displayName, isGroup } from '../model/store.svelte';
import { prefs } from '../model/prefs.svelte';
import type { Group, Id } from '../model/types';
import { call, setGroupBusy } from './call.svelte';
import './cues';

export type GPhase = 'idle' | 'starting' | 'connecting' | 'active' | 'reconnecting' | 'ended' | 'failed';
export type EndReason = 'hangup' | 'host-lost' | 'no-host' | 'cancelled' | 'removed';

/** Simulated timings in real ms (not motion durations). Mutable so tests and the drawer can shorten them. */
export const GCALL_MS = {
  ask: 700,            // starter -> candidate: request answered (spec: 5 s silence moves on; shortened)
  refuse: 450,         // between a refusal and the next ask
  connect: 700,        // CallStart seen -> media path up
  redialPlanned: 1200, // End{host_leaving}: redial at once
  redialCrash: 3000,   // 3 missed 1 s heartbeats
  redialRotate: 1400,
  bannerHold: 9000,    // Proposed: how long the "started a call" banner / heads-up stays before it settles into the notification
  toast: 3200,
  endedHold: 900,
  failedHold: 3200,
};

export interface HostCond {
  class: 'desktop' | 'phone';
  device: string;                                    // "laptop", "phone"
  power: 'mains' | 'battery';
  pct: number;
  metered: boolean;
  thermal: 'ok' | 'moderate' | 'severe' | 'critical';
  busy: boolean;                                     // already carries another call (Calls hosted at once per device: 1)
  refuses: boolean;                                  // drawer: this host declines requests (its user said no / paused)
}
export interface LiveCall {
  callId: string;
  by: Id;                    // starter
  startedAt: number;         // performance.now()
  hostId: Id;                // the call host's member id
  parts: Id[];               // join order
  gen: number;               // host_gen
  limited: boolean;          // host degraded: K = 1, joins refused
  everJoinedByMe: boolean;
}
export interface Part { id: Id; name: string; hue: number; me: boolean; muted: boolean; speaking: boolean; level: number }

const DEFAULT_COND: Record<string, Partial<HostCond>> = {
  arjun: { class: 'desktop', device: 'laptop' },
};
const mk = (id: string): HostCond => ({ class: 'phone', device: 'phone', power: 'battery', pct: 80, metered: false, thermal: 'ok', busy: false, refuses: false, ...(id === 'me' ? { device: 'this device' } : {}), ...(DEFAULT_COND[id] ?? {}) });

const THERMAL_RANK = { ok: 0, moderate: 1, severe: 2, critical: 3 } as const;
const CAP = { desktop: 12, phone: 8, phoneMetered: 4 } as const;

function mulberry(a: number) { return () => { a |= 0; a = (a + 0x6d2b79f5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }

class GroupCall {
  phase = $state<GPhase>('idle');
  groupId = $state<Id | null>(null);
  callId = $state('');
  startedByMe = $state(false);
  /** 'full' = call screen; 'strip' = minimised. */
  view = $state<'full' | 'strip'>('full');
  muted = $state(false);
  speaker = $state<'earpiece' | 'speaker'>('earpiece');
  elapsedS = $state(0);
  endReason = $state<EndReason | null>(null);
  fading = $state(false);
  asking = $state<string>('');            // "Arjun’s laptop" while the starter waits for an answer
  /** Calls running in groups (mine or not). Keyed by group id. */
  live = $state<Record<Id, LiveCall>>({});
  conds = $state<Record<string, HostCond>>({});
  levels = $state<Record<Id, number>>({});
  remoteMuted = $state<Record<Id, boolean>>({});
  loud = $state<Id[]>([]);
  /** Drawer: false = nobody speaks on their own (tests / screenshots set speakers by hand). */
  chatter = $state(true);
  forced = $state<Record<Id, boolean>>({});
  toast = $state<{ id: number; text: string } | null>(null);
  /** A quiet "started a call" notice for a call I am not in (banner, heads-up, then the notification). */
  announce = $state<{ gid: Id; callId: string; by: Id; bannerOn: boolean; dismissed: boolean } | null>(null);
  /** A note for the host's own user (me hosting): battery low, limited, on mobile data. */
  hostNote = $state('');
  /** The roster as it was when my part ended, so the ended screen keeps its people (dimmed) instead of emptying. */
  frozen = $state<Part[]>([]);

  #timers = new Set<number>();
  #tick = 0;
  #sim = 0;
  #connectedMono = 0;
  #recorded = true;
  #rand = mulberry(7);
  #talk: Record<Id, { on: boolean; until: number; amp: number }> = {};
  #cand: Record<Id, number> = {};
  #uid = 0;
  #bannerTimer = 0;
  #toastTimer = 0;

  constructor() {
    setGroupBusy(() => this.holdsLine);
    // Conditions exist up front (never created while a template reads them).
    for (const p of db.people) this.conds[p.id] = mk(p.id);
    this.conds['me'] = mk('me');
  }

  /* ---------- rules ---------- */
  /** This device's user is in (or setting up) a group call: the one line is taken. */
  get holdsLine() { return this.phase === 'starting' || this.phase === 'connecting' || this.phase === 'active' || this.phase === 'reconnecting'; }
  get active() { return this.phase !== 'idle'; }
  get group(): Group | undefined { const c = chat(this.groupId); return isGroup(c) ? c : undefined; }
  get current(): LiveCall | undefined { return this.groupId ? this.live[this.groupId] : undefined; }
  liveFor(gid: Id): LiveCall | undefined { return this.live[gid]; }
  /** I am in this group's running call. */
  inCall(gid: Id) { return this.groupId === gid && this.holdsLine; }

  cond(id: Id): HostCond { return this.conds[id] ?? mk(id); }
  hostClass(id: Id): 'desktop' | 'phone' { return id === 'me' ? (prefs.form === 'android' ? 'phone' : 'desktop') : this.cond(id).class; }
  hostLabel(id: Id) { return id === 'me' ? 'this device' : `${firstName(id)}’s ${this.cond(id).device}`; }
  hostLabelCap(id: Id) { const s = this.hostLabel(id); return s[0].toUpperCase() + s.slice(1); }

  /** Spec §2.3 tiers. null = excluded. */
  tierOf(id: Id): number | null {
    const c = this.cond(id), cls = this.hostClass(id);
    if (c.busy || THERMAL_RANK[c.thermal] >= 1) return null;
    if (cls === 'phone' && c.power === 'battery' && c.pct < 30) return null;
    if (c.metered) return 4;
    if (c.power === 'mains') return cls === 'phone' ? 1 : 0;
    if (cls === 'phone') return c.pct >= 50 ? 2 : 3;
    return c.pct >= 30 ? 2 : 3;
  }
  /** Call hosts with a fresh status, best first. Ties: the starter's own device, then a stable order. */
  rank(g: Group, starter: Id, skip: Id[] = []): Id[] {
    const cands = g.members.filter((m) => m.callHost && !m.callHostPaused && !skip.includes(m.id)
      && (m.id === 'me' ? db.env.networkOnline : person(m.id)?.online !== false));
    const scored = cands.map((m) => ({ id: m.id, t: this.tierOf(m.id) })).filter((x): x is { id: Id; t: number } => x.t !== null);
    const best = Math.min(...scored.map((x) => x.t));
    return scored.sort((a, b) => a.t - b.t || (a.t === best && a.id === starter ? -1 : b.t === best && b.id === starter ? 1 : 0) || (a.id < b.id ? -1 : 1)).map((x) => x.id);
  }
  /** Members who could take over if the host goes (a phone only if its user is in the call). */
  backups(gid: Id): Id[] {
    const g = chat(gid), lc = this.live[gid];
    if (!isGroup(g) || !lc) return [];
    return this.rank(g, lc.by, [lc.hostId]).filter((id) => this.hostClass(id) === 'desktop' || lc.parts.includes(id)).slice(0, 2);
  }
  capFor(lc: LiveCall) {
    const c = this.cond(lc.hostId), cls = this.hostClass(lc.hostId);
    return cls === 'desktop' ? CAP.desktop : c.metered ? CAP.phoneMetered : CAP.phone;
  }
  kFor(lc: LiveCall) { return lc.limited ? 1 : this.hostClass(lc.hostId) === 'desktop' ? 3 : 2; }
  /** Why a join cannot happen right now (null = fine). */
  joinBlock(gid: Id): 'none' | 'full' | 'limited' | 'inCall' | null {
    const lc = this.live[gid];
    if (!lc) return 'none';
    if (call.active || (this.holdsLine && this.groupId !== gid)) return 'inCall';
    if (lc.limited) return 'limited';
    if (lc.parts.length >= this.capFor(lc)) return 'full';
    return null;
  }

  /* ---------- text ---------- */
  get statusText() {
    switch (this.phase) {
      case 'starting': return 'Finding a call host…';
      case 'connecting': return 'Connecting…';
      case 'reconnecting': return 'Reconnecting…';
      case 'ended': return this.endReason === 'host-lost' ? 'Call ended — host lost' : 'Call ended';
      case 'failed': return this.endReason === 'no-host' ? 'No call host available' : 'Couldn’t connect';
      default: return '';
    }
  }
  get fieldState() {
    switch (this.phase) {
      case 'idle': return 'calm';
      case 'starting': case 'connecting': return 'connecting';
      default: return this.phase;
    }
  }
  /** The people in the call as the screen draws them (me first when I started it). */
  get participants(): Part[] {
    const lc = this.current;
    if (!lc) {
      if (this.frozen.length) return this.frozen;
      // Setting up (or failed before anyone could be reached): just me, so the screen is never empty.
      return this.phase === 'idle' ? [] : [{ id: 'me', me: true, name: 'You', hue: db.account.hue, muted: this.muted, speaking: false, level: 0 }];
    }
    return lc.parts.map((id) => ({
      id, me: id === 'me', name: id === 'me' ? 'You' : displayName(id), hue: id === 'me' ? db.account.hue : (person(id)?.hue ?? 0),
      muted: id === 'me' ? this.muted : !!this.remoteMuted[id], level: this.levels[id] ?? 0, speaking: this.loud.includes(id),
    }));
  }

  /* ---------- starting and joining ---------- */
  say(text: string) {
    window.clearTimeout(this.#toastTimer);
    this.toast = { id: ++this.#uid, text };
    this.#toastTimer = window.setTimeout(() => (this.toast = null), GCALL_MS.toast);
  }
  /** The toast the 1:1 side shows when a call is placed while a group call holds the line (InCall). */
  inCallToast() { this.say('You’re in a group call. Leave it to place a call.'); }

  /** Start a call in a group, or return to / join the one already running there. */
  start(gid: Id): boolean {
    const g = chat(gid);
    if (!isGroup(g) || g.status !== 'active') return false;
    if (this.inCall(gid)) { this.view = 'full'; return false; }
    if (this.live[gid]) return this.join(gid);
    if (call.active) { this.say('You’re on a call. End it to start a group call.'); return false; }
    if (this.holdsLine) { this.say('You’re already in a group call.'); return false; }
    if (db.account.gate !== 'ready') return false;
    if (!g.members.some((m) => m.callHost && !m.callHostPaused)) {
      this.say(g.members.some((m) => m.callHost) ? 'No call host is accepting calls right now.' : 'This group has no call host. An admin can appoint one in Group info.');
      return false;
    }
    this.#begin(gid, '');
    this.startedByMe = true;
    this.phase = 'starting'; this.view = 'full';
    const order = this.rank(g, 'me');
    const refused: Id[] = [];
    const ask = (i: number) => {
      if (this.phase !== 'starting') return;
      const hid = order[i];
      if (!hid) { this.#fail('no-host'); return; }
      this.asking = this.hostLabelCap(hid);
      this.#later(GCALL_MS.ask, () => {
        if (this.phase !== 'starting') return;
        const c = this.cond(hid);
        // The asked device re-checks: its user said no, it is busy, or it is a phone being asked by someone else (Q11).
        const no = hid !== 'me' && (c.refuses || c.busy || (this.hostClass(hid) === 'phone'));
        if (no) { refused.push(hid); this.#later(GCALL_MS.refuse, () => ask(i + 1)); return; }
        this.asking = '';
        this.#hosted(gid, hid);
      });
    };
    ask(0);
    return true;
  }
  #hosted(gid: Id, hostId: Id) {
    this.callId = 'c' + ++this.#uid;
    this.live[gid] = { callId: this.callId, by: 'me', startedAt: performance.now(), hostId, parts: ['me'], gen: 0, limited: false, everJoinedByMe: true };
    this.#note(gid, 'me');
    this.phase = 'connecting';
    this.#later(GCALL_MS.connect, () => this.#connected());
  }
  /** Join the call running in this group. */
  join(gid: Id): boolean {
    const lc = this.live[gid];
    if (!lc) return false;
    if (this.inCall(gid)) { this.view = 'full'; return false; }
    const b = this.joinBlock(gid);
    if (b === 'inCall') { call.active ? this.say('You’re on a call. End it to join a group call.') : this.say('You’re already in a group call.'); return false; }
    if (b === 'full') { this.say(`This call is full (${this.capFor(lc)} people).`); return false; }
    if (b === 'limited') { this.say('The call host isn’t taking new people right now.'); return false; }
    this.#begin(gid, lc.callId);
    this.startedByMe = false;
    this.phase = 'connecting'; this.view = 'full';
    if (this.announce?.gid === gid) this.announce = null;
    this.#later(GCALL_MS.connect, () => {
      if (this.phase !== 'connecting') return;
      if (!this.live[gid]) { this.#finish('failed', 'cancelled'); return; }
      if (!lc.parts.includes('me')) lc.parts.push('me');
      lc.everJoinedByMe = true;
      this.#connected();
    });
    return true;
  }
  /** After "Call ended — host lost": a fresh start in the same group (the old call is gone). */
  rejoin() {
    const gid = this.groupId;
    if (!gid) return;
    this.#reset();
    this.start(gid);
  }

  /* ---------- my actions ---------- */
  leave() {
    switch (this.phase) {
      case 'starting': this.#finish('ended', 'cancelled', true); break;
      case 'connecting': case 'active': case 'reconnecting': this.#finish('ended', 'hangup', true); break;
      case 'ended': case 'failed': this.dismiss(); break;
    }
  }
  toggleMute() { this.muted = !this.muted; if (this.muted) delete this.forced['me']; }
  toggleSpeaker() { this.speaker = this.speaker === 'speaker' ? 'earpiece' : 'speaker'; }
  minimize() { if (this.phase !== 'idle') this.view = 'strip'; }
  restore() { if (this.phase !== 'idle') this.view = 'full'; }
  /** Leave the ended / failed screen now. */
  dismiss() { if (this.phase === 'ended' || this.phase === 'failed') this.fading = true; this.#later(dur('callEnd'), () => this.#reset()); }
  dismissAnnounce() { if (this.announce) this.announce.dismissed = true; }
  muteHostNote() { this.hostNote = ''; }

  /* ---------- the simulated other side ---------- */
  /** Someone else starts a call in a group (no ring: a notice, and a soft cue). */
  remoteStart(gid: Id, by: Id): boolean {
    const g = chat(gid);
    if (!isGroup(g) || g.status !== 'active' || this.live[gid] || !g.members.some((m) => m.id === by)) return false;
    const order = this.rank(g, by);
    const hostId = g.members.find((m) => m.id === by)?.callHost && order.includes(by) ? by : order[0];
    if (!hostId) return false;
    const callId = 'c' + ++this.#uid;
    this.live[gid] = { callId, by, startedAt: performance.now(), hostId, parts: [by], gen: 0, limited: false, everJoinedByMe: false };
    this.#note(gid, by);
    if (!this.holdsLine || this.groupId !== gid) {
      this.announce = { gid, callId, by, bannerOn: true, dismissed: false };
      window.clearTimeout(this.#bannerTimer);
      this.#bannerTimer = window.setTimeout(() => { if (this.announce?.callId === callId) this.announce.bannerOn = false; }, GCALL_MS.bannerHold);
      if (!g.muted && !this.holdsLine) emitFx({ type: 'gcall', cue: 'started' });
    }
    return true;
  }
  remoteJoin(gid: Id, id: Id): boolean {
    const lc = this.live[gid], g = chat(gid);
    if (!lc || !isGroup(g) || lc.parts.includes(id) || id === 'me' || !g.members.some((m) => m.id === id)) return false;
    if (lc.limited || lc.parts.length >= this.capFor(lc)) return false;
    lc.parts.push(id);
    if (this.inCall(gid) && this.phase === 'active') emitFx({ type: 'gcall', cue: 'joined' });
    return true;
  }
  remoteLeave(gid: Id, id: Id) {
    const lc = this.live[gid];
    if (!lc || !lc.parts.includes(id) || id === 'me') return;
    lc.parts = lc.parts.filter((x) => x !== id);
    delete this.levels[id]; delete this.#talk[id]; delete this.forced[id];
    this.loud = this.loud.filter((x) => x !== id);
    if (this.inCall(gid) && this.phase === 'active') emitFx({ type: 'gcall', cue: 'left' });
    // The last person gone from a call I am not in: it is over.
    if (!lc.parts.length) this.#worldEnd(gid);
  }
  remoteMute(id: Id, on: boolean) { this.remoteMuted[id] = on; }
  /** Make someone speak (true) or stay quiet (false); undefined hands them back to the chatter. */
  setSpeaking(id: Id, on: boolean | undefined) { if (on === undefined) delete this.forced[id]; else this.forced[id] = on; }

  /** A change on the call host's device. kind: lowBattery | limited | critical | metered | recover | lost | rotate. */
  hostEvent(gid: Id, kind: 'lowBattery' | 'limited' | 'critical' | 'metered' | 'recover' | 'lost' | 'rotate') {
    const lc = this.live[gid];
    if (!lc) return;
    const c = this.cond(lc.hostId), mine = lc.hostId === 'me';
    switch (kind) {
      case 'lowBattery':
        Object.assign(c, { power: 'battery', pct: 25 });
        if (mine) this.hostNote = 'Battery is low. This device won’t take new calls.';
        break;
      case 'limited':
        Object.assign(c, { power: 'battery', pct: 12 }); lc.limited = true;
        if (mine) this.hostNote = 'Battery under 15 %. Hosting is limited: only one person is heard at a time, and no one new can join.';
        break;
      case 'metered':
        c.metered = true;
        if (mine) this.hostNote = 'You’re hosting on mobile data. This call uses data, and at most 4 people can be in it.';
        break;
      case 'recover':
        Object.assign(c, { power: 'mains', pct: 100, metered: false, thermal: 'ok', busy: false }); lc.limited = false; this.hostNote = '';
        break;
      case 'critical':
        Object.assign(c, { power: 'battery', pct: 6 });
        this.hostNote = '';
        this.#redial(gid, 'planned');
        break;
      case 'lost': this.#redial(gid, 'crash'); break;
      case 'rotate': this.#redial(gid, 'rotate'); break;
    }
  }
  /** The host's own user ends hosting (not the call): the others redial the backup at once. */
  hostEnds(gid: Id) { this.#redial(gid, 'planned'); }

  /* ---------- internals ---------- */
  #later(ms: number, fn: () => void) {
    const id = window.setTimeout(() => { this.#timers.delete(id); fn(); }, ms);
    this.#timers.add(id);
  }
  #clearTimers() { this.#timers.forEach(clearTimeout); this.#timers.clear(); clearInterval(this.#tick); clearInterval(this.#sim); }
  #begin(gid: Id, callId: string) {
    this.#clearTimers();
    this.groupId = gid; this.callId = callId; this.endReason = null; this.muted = false; this.speaker = 'earpiece';
    this.fading = false; this.elapsedS = 0; this.#connectedMono = 0; this.#recorded = false; this.asking = ''; this.hostNote = '';
    this.loud = []; this.levels = {}; this.#talk = {}; this.#cand = {}; this.#rand = mulberry(7); this.frozen = [];
  }
  #connected() {
    if (this.phase !== 'connecting') return;
    this.phase = 'active';
    this.#connectedMono = performance.now();
    this.elapsedS = 0;
    this.#tick = window.setInterval(() => {
      const s = Math.floor((performance.now() - this.#connectedMono) / 1000);
      if (s !== this.elapsedS) this.elapsedS = s;
    }, 250);
    this.#sim = window.setInterval(() => this.#simulate(), 120);
    emitFx({ type: 'connect' });
  }
  #fail(reason: EndReason) {
    this.#clearTimers();
    this.endReason = reason; this.asking = '';
    this.phase = 'failed'; this.#recorded = true;
    emitFx({ type: 'failed' });
    this.#later(GCALL_MS.failedHold, () => { this.fading = true; });
    this.#later(GCALL_MS.failedHold + dur('callEnd'), () => this.#reset());
  }
  /** My part in the call is over. `mine` = I left (the screen goes at once); otherwise it stays readable first. */
  #finish(kind: 'ended' | 'failed', reason: EndReason, mine = false) {
    if (this.phase === 'idle' || this.phase === 'ended' || this.phase === 'failed') return;
    const gid = this.groupId!, lc = this.live[gid];
    const seconds = this.#connectedMono ? Math.floor((performance.now() - this.#connectedMono) / 1000) : 0;
    // Most people in it at once that this device saw (me included), for "Group call · 12 min · 3 people".
    const people = Math.max(this.participants.length, lc?.parts.length ?? 1, 1);
    this.#clearTimers();
    const wasHost = lc?.hostId === 'me';
    if (lc && !mine) this.frozen = this.participants.map((p) => ({ ...p, speaking: false, level: 0 }));
    if (lc && mine) {
      lc.parts = lc.parts.filter((x) => x !== 'me');
      if (!lc.parts.length) this.#worldEnd(gid, true);
    }
    if (!this.#recorded && this.#connectedMono) { this.#recorded = true; this.#record(gid, lc?.by ?? 'me', seconds, people); }
    this.endReason = reason; this.asking = '';
    this.phase = kind === 'failed' ? 'failed' : 'ended';
    emitFx({ type: kind === 'failed' ? 'failed' : 'end' });
    // I was the host and left: the others redial the backup (the call goes on without me).
    if (mine && wasHost && lc && this.live[gid]) this.#redial(gid, 'planned');
    if (mine) { this.fading = true; this.#later(dur('callEnd'), () => this.#reset()); return; }
    const hold = reason === 'host-lost' ? 0 : GCALL_MS.endedHold;
    if (hold) { this.#later(hold, () => { this.fading = true; }); this.#later(hold + dur('callEnd'), () => this.#reset()); }
  }
  #reset() {
    this.#clearTimers();
    this.phase = 'idle'; this.groupId = null; this.callId = ''; this.view = 'full'; this.fading = false; this.endReason = null;
    this.muted = false; this.speaker = 'earpiece'; this.elapsedS = 0; this.asking = ''; this.loud = []; this.levels = {}; this.hostNote = ''; this.frozen = [];
  }
  /** One record per call per device, written when my part ends: "Group call · 12 min · 3 people". */
  #record(gid: Id, by: Id, durationS: number, people: number) {
    const g = chat(gid);
    if (!g) return;
    pushMessage(g, { from: by, text: '', call: { kind: 'voice', outcome: 'answered', durationS, direction: by === 'me' ? 'out' : 'in', group: true, people } });
  }
  #note(gid: Id, by: Id) {
    const g = chat(gid);
    if (g) pushMessage(g, { from: by, text: by === 'me' ? 'You started a call' : `Call started by ${firstName(by)}`, system: true });
  }
  /** The call is over for everyone (nobody left, or no host could be reached). */
  #worldEnd(gid: Id, quiet = false) {
    const lc = this.live[gid];
    if (!lc) return;
    delete this.live[gid];
    if (this.announce?.gid === gid) this.announce = null;
    const g = chat(gid);
    if (g && !quiet && !lc.everJoinedByMe) {
      // A call that happened without me: a call record too (the thread and the Calls tab), calm, not red.
      const s = Math.max(1, Math.round((performance.now() - lc.startedAt) / 1000));
      pushMessage(g, { from: lc.by, text: '', call: { kind: 'voice', outcome: 'missed', durationS: s, direction: 'in', group: true, people: Math.max(lc.parts.length, 1) } });
    }
  }
  /** Host gone: everyone redials the backups. kind: planned (End{host_leaving}), crash (heartbeats), rotate (End{host_rotating}). */
  #redial(gid: Id, kind: 'planned' | 'crash' | 'rotate') {
    const lc = this.live[gid];
    if (!lc) return;
    const lost = lc.hostId, mineIn = this.inCall(gid) && this.phase === 'active';
    lc.gen++;
    if (mineIn) { this.phase = 'reconnecting'; emitFx({ type: 'reconnecting' }); }
    const ms = kind === 'crash' ? GCALL_MS.redialCrash : kind === 'rotate' ? GCALL_MS.redialRotate : GCALL_MS.redialPlanned;
    const done = () => {
      const l = this.live[gid];
      if (!l || l.callId !== lc.callId) return;
      const next = kind === 'rotate' ? lost : this.backups(gid).find((b) => b !== lost);
      if (next) {
        l.hostId = next; l.limited = this.cond(next).pct < 15 && this.cond(next).power === 'battery';
        if (this.inCall(gid) && this.phase === 'reconnecting') { this.phase = 'active'; emitFx({ type: 'connect' }); }
        if (next === 'me') this.hostNote = '';
        return;
      }
      // No backup answered.
      if (this.inCall(gid)) this.#finish('ended', 'host-lost');
      this.#worldEnd(gid);
    };
    if (this.groupId === gid && this.holdsLine) this.#later(ms, done); else window.setTimeout(done, ms);
  }

  /* ---------- speaking (simulated) ---------- */
  #simulate() {
    const gid = this.groupId, lc = gid ? this.live[gid] : undefined;
    if (!lc || (this.phase !== 'active' && this.phase !== 'reconnecting')) return;
    const now = performance.now();
    const quiet = this.phase === 'reconnecting';
    const talkers = lc.parts.filter((id) => this.#talk[id]?.on).length;
    let n = talkers;
    for (const id of lc.parts) {
      const t = (this.#talk[id] ??= { on: false, until: now + 400 + this.#rand() * 2500, amp: .55 + this.#rand() * .45 });
      const f = this.forced[id];
      const canTalk = id === 'me' ? !this.muted : !this.remoteMuted[id];
      if (quiet) { t.on = false; t.until = now + 800; }
      else if (f !== undefined) { t.on = f && canTalk; t.until = now + 1000; }
      else if (!this.chatter || !canTalk) { t.on = false; t.until = now + 800; }
      else if (now > t.until) {
        if (t.on) { t.on = false; n--; t.until = now + 1400 + this.#rand() * 4200; }
        else if (n < 2 && this.#rand() < .5) { t.on = true; n++; t.until = now + 1300 + this.#rand() * 2800; t.amp = .5 + this.#rand() * .5; }
        else t.until = now + 500;
      }
      const target = t.on ? t.amp * (.62 + .38 * Math.abs(Math.sin(now / 170 + id.length * 7))) : 0;
      const cur = this.levels[id] ?? 0;
      const nv = cur + (target - cur) * .45;
      this.levels[id] = nv < .02 ? 0 : +nv.toFixed(2);
    }
    // The host forwards the K loudest above the speech threshold; a newcomer needs 2 ticks (~200 ms) of being clearly louder.
    const K = this.kFor(lc);
    const others = lc.parts.filter((id) => id !== 'me');
    let keep = this.loud.filter((id) => id !== 'me' && others.includes(id) && (this.levels[id] ?? 0) > .16);
    const cands = others.filter((id) => !keep.includes(id) && (this.levels[id] ?? 0) > .3).sort((a, b) => this.levels[b] - this.levels[a]);
    for (const c of cands) {
      if (keep.length < K) { keep.push(c); continue; }
      const weakest = keep.reduce((w, id) => (this.levels[id] < this.levels[w] ? id : w), keep[0]);
      if (this.levels[c] > this.levels[weakest] + .2) {
        this.#cand[c] = (this.#cand[c] ?? 0) + 1;
        if (this.#cand[c] >= 2) { keep = keep.filter((id) => id !== weakest); keep.push(c); this.#cand[c] = 0; }
      } else this.#cand[c] = 0;
    }
    if (!this.muted && (this.levels['me'] ?? 0) > .2) keep.push('me');   // my own voice is shown from the local level, not from the host
    if (keep.length !== this.loud.length || keep.some((id, i) => id !== this.loud[i])) this.loud = keep;
  }
}

export const gcall = new GroupCall();

export const speakingNames = (ps: Part[], max = 3) => {
  const names = ps.filter((p) => p.speaking).map((p) => (p.me ? 'You' : firstName(p.id)));
  if (!names.length) return '';
  const head = names.slice(0, max), rest = names.length - head.length;
  const list = head.length > 1 ? head.slice(0, -1).join(', ') + ' and ' + head[head.length - 1] : head[0];
  return rest ? `${list} and ${rest} more` : list;
};

/** Test hook: lets the Playwright flows read the machine and shorten its timings. Not used by the app. */
if (typeof window !== 'undefined') { (window as any).__gc = gcall; (window as any).__gcms = GCALL_MS; }
