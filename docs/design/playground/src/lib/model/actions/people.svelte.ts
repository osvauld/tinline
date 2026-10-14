/* People, devices and settings (PLAN step 8). The state the Contacts tab, Devices, Privacy and Settings share, plus the
   rules they follow. The people and devices themselves stay in the store (db.people, db.devices); this module holds only
   what is not app data: open dialogs, OS permission state, notification switches, the link-a-device flow. Screens read it;
   the Simulation drawer (PeopleSim) drives the "outside" parts (a request arrives, a permission is denied, the new device answers).
   Nothing here talks to a network, camera or system. */
import { db, newId, person } from '../store.svelte';
import { clock, T } from '../clock.svelte';
import { emitFx } from '../../fx/bus';

export type Perm = 'granted' | 'denied';
export interface Request { id: string; who: string; name: string; hue: number; at: number }
/** A request I sent with someone's code, waiting for them to accept. */
export interface Sent { id: string; name: string; hue: number; at: number }

export const pp = $state({
  /** Dialogs drawn above the whole app (PeopleLayers): the new-chat picker and the add-contact sheet. */
  dialog: null as null | 'newChat' | 'addContact',
  addTab: 'code' as 'person' | 'code',
  /** OS permissions the app has been given. Android also has battery optimisation. */
  perms: { notifications: 'granted' as Perm, microphone: 'granted' as Perm, camera: 'granted' as Perm, battery: 'ok' as 'ok' | 'restricted' },
  /** Notification switches (mine, per account on this device). */
  notif: { messages: true, calls: true, groups: true },
  /** PROPOSED: what a new contact may do before I say. The core only has the per-contact Allow calls. */
  newContactsCall: false,
  /** People who scanned my code or sent me a request and wait for my answer. */
  requests: [] as Request[],
  /** How a contact came: I scanned / pasted their code, or `them`: they used MY code (core adds them with no
      question asked, see protocol.md "Add contact"). Absent = unknown (older contacts). */
  how: {} as Record<string, 'scan' | 'code' | 'them'>,
  /** When they added me (for "Added you just now"). */
  addedAt: {} as Record<string, number>,
  /** Someone is using my code right now (core: a ContactHello arrived). `joining` while the handshake runs, `joined`
      once it is complete. Shown in the Add contact sheet as "New contact" (it opens on its own, except during a call). */
  /** Who the open Add contact sheet is showing (its found face), so an acceptance knows whether it lands in place. */
  facing: null as null | string,
  incoming: null as null | { id: string; name: string; hue: number; phase: 'asking' | 'joined'; kind: 'request' | 'accepted' },
  /** Requests I sent (I used their code), waiting for them to accept. */
  sent: [] as Sent[],
  /** My current contact code. DECIDED by the user 2026-10-13 (PROPOSED core change, see SIGNUP-HANDOFF § 6f): a code
      works for anyone who has it until it expires (7 days) or I make a new one. Core today: single use. */
  myCode: '',
  myCodeExp: 0,
});

export function resetPeopleState() {
  pp.dialog = null; pp.addTab = 'code';
  pp.perms.notifications = 'granted'; pp.perms.microphone = 'granted'; pp.perms.camera = 'granted'; pp.perms.battery = 'ok';
  pp.notif.messages = true; pp.notif.calls = true; pp.notif.groups = true;
  pp.newContactsCall = false; pp.requests = []; pp.sent = []; pp.incoming = null; pp.how = {}; pp.addedAt = {}; pp.myCode = MY_CODE; pp.myCodeExp = clock.now + CODE_DAYS * T.DAY;
  link.reset(); linkSim.failure = 'none'; linkSim.name = 'Tablet';
}

/** Add contact opens on Code everywhere (decided 2026-10-13): your code first; QR (in person) is one tap away. */
export const open = (d: 'newChat' | 'addContact', tab: 'person' | 'code' = 'code') => { pp.addTab = tab; pp.dialog = d; };
export const closeDialog = () => { pp.dialog = null; pp.incoming = null; };

/* ---------- contacts ---------- */
/* A contact code is the core's ticket: `OSVC2:` + ~250 base32 characters (crates/core node.rs `my_ticket`), not
   something to read or type. These are synthetic, same shape and length. */
const B32 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
export function sampleCard(seed: string, len = 248): string {
  let h = [...seed].reduce((n, c) => (n * 31 + c.charCodeAt(0)) >>> 0, 2166136261), out = 'OSVC2:';
  for (let i = 0; i < len; i++) { h = (Math.imul(h ^ (h >>> 15), 2246822519) + i) >>> 0; out += B32[h % 32]; }
  return out;
}
export const MY_CODE = sampleCard('maya');
export const CODE_DAYS = 7;
pp.myCode = MY_CODE; pp.myCodeExp = clock.now + CODE_DAYS * T.DAY;
/** "New code" (core `reset_ticket`): the old code stops working at once, the new one runs 7 days. The two-phone lab
    replaces this so the other window learns the new code. */
let renewals = 0;
export const codeHooks = { renew: () => { pp.myCode = sampleCard('me#' + ++renewals); pp.myCodeExp = clock.now + CODE_DAYS * T.DAY; } };
export const codeUntil = () => new Date(pp.myCodeExp).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric', month: 'short' });
/** People the simulated scanner / pasted codes can resolve to. */
export const DIRECTORY = [
  { id: 'tara', key: 'TARA', name: 'Tara Iyer', hue: 3, card: sampleCard('tara') },
  { id: 'nikhil', key: 'NIKH', name: 'Nikhil Das', hue: 1, card: sampleCard('nikhil') },
  { id: 'isha', key: 'ISHA', name: 'Isha Verma', hue: 5, card: sampleCard('isha') },
];
/** The first word in `text` that looks like a contact code, like core `find_card`: a code pasted inside a
    message ("Add me on Tinline: OSVC2:…") still works. */
export function findCard(text: string): string | null {
  return text.split(/[\s<>"'()]+/).map((w) => w.replace(/[.,;!?]+$/, '')).find((w) => /^osvc2:/i.test(w) && w.length > 20) ?? null;
}
export type Found = { ok: true; id: string; name: string; hue: number; already: boolean; blocked: boolean; code?: string } | { ok: false; error: string };

const found = (d: { id: string; name: string; hue: number }): Found => {
  const p = person(d.id);
  return { ok: true, id: d.id, name: p?.name ?? d.name, hue: p?.hue ?? d.hue, already: !!p?.contact, blocked: !!p?.blocked };
};

/** The camera found a code: the next person in the directory who is not a contact yet (wraps around). */
export function scanResult(): Found {
  const next = DIRECTORY.find((d) => !person(d.id)?.contact) ?? DIRECTORY[0];
  return found(next);
}

export function resolveCode(raw: string): Found {
  if (!raw.trim()) return { ok: false, error: 'Paste their code first.' };
  const c = findCard(raw)?.toUpperCase();
  if (!c) return { ok: false, error: 'That doesn’t look like a Tinline code. Could they copy it again for you?' };
  const owner = CARD_OWNER.get(c);
  if (c === pp.myCode || owner?.id === 'me') return { ok: false, error: 'That’s your own code. Send it to them, and paste theirs here.' };
  // Expired (7 days): core `peek_card` returns None today, so the app can't name them or say why. PROPOSED core change:
  // return the reason (and the signed name) so this message can be specific.
  if (owner?.expired) return { ok: false, error: `That code from ${owner.name.split(' ')[0]} has expired. Codes last 7 days, so ask them for a fresh one.` };
  if (owner) return { ...(found(owner) as Found & { ok: true }), code: c };
  const signed = pairing.owner?.(c);
  if (signed?.id === 'me') return { ok: false, error: 'That’s your own code. Send it to them, and paste theirs here.' };
  if (signed) return { ...(found(signed) as Found & { ok: true }), code: c };
  const d = DIRECTORY.find((x) => x.card === c);
  if (d) return found(d);
  const h = [...c].reduce((n, ch) => n + ch.charCodeAt(0), 0);
  return found(DIRECTORY[h % DIRECTORY.length]);
}

/** Codes the simulation knows the owner of, current or spent (like core `peek_card`, which reads the signed name
    from the code itself without dialling). Filled by the two-phone lab (lib/sim/pair.ts). */
export const CARD_OWNER = new Map<string, { id: string; name: string; hue: number; expired?: boolean }>();

/** Adding someone by their code dials them (core `add_contact`): their device must be on, and the code must be the
    one they hand out now. The two-phone lab replaces `pairing.reach` with a real round trip to the other window. */
/* Outcomes of core `add_contact` (node.rs `add_contact_from`), as the joiner sees them:
   ok        Welcome received; both sides now have each other (and may call).
   timeout   Error::Timeout: the dial (up to 30 s) or the answer (up to 15 s) never came. Their device is off/offline.
   refused   Error::Rejected("refused"): their device answered but the code is not the one it hands out now. Core can't
             say which: someone else used it, they made a new one, or they changed their nickname (set_name drops it).
   blocked   Error::Rejected("blocked"): they blocked me (their notice is the answer).
   nonet     My own device is offline (checked before dialling). */
export type Reach = 'ok' | 'pending' | 'timeout' | 'refused' | 'blocked' | 'nonet';
/* DECIDED by the user 2026-10-13: adding by code is a REQUEST. Their device takes it (pending = delivered) and they
   choose Accept or Not now. PROPOSED core change (SIGNUP-HANDOFF § 6f): today the owner's device accepts on its own. */
type Dial = { id: string; name: string; hue: number; code?: string };
export const pairing = {
  reach: null as null | ((f: Dial) => Promise<Reach>),
  /** My answer to a request, so the lab can tell the other window (a "Not now" sends nothing). */
  answered: null as null | ((r: Request, yes: boolean) => void),
  /** Like core `peek_card` reading the signed name inside a code: the lab's codes carry their owner, so a window
      recognises its own code (and a peer's old one) even after a reload, without having seen it announced. */
  owner: null as null | ((code: string) => { id: string; name: string; hue: number } | undefined),
};
/** My request was delivered: remember it ("Waiting for Tara"). */
export function requestSent(f: { id: string; name: string; hue: number }) {
  if (!pp.sent.some((x) => x.id === f.id)) pp.sent.push({ ...f, at: clock.now });
}
/** They accepted my request: they become a contact, and I see the line open (the sheet, unless I'm in a call). */
export function accepted(f: { id: string; name: string; hue: number }, inCall = false) {
  if (!pp.sent.some((x) => x.id === f.id) || person(f.id)?.contact) return;
  pp.sent = pp.sent.filter((x) => x.id !== f.id);
  addPerson(f, 'code');
  // If my Add contact sheet is on them, it turns taut by itself. Otherwise (closed, or open on someone else / the
  // other tab) the moment takes the sheet. Never over a call or another dialog.
  if (inCall || (pp.dialog === 'addContact' && pp.facing === f.id) || (pp.dialog && pp.dialog !== 'addContact')) return;
  pp.incoming = { ...f, phase: 'joined', kind: 'accepted' }; pp.dialog = 'addContact';
}
export async function reach(f: Dial): Promise<Reach> {
  if (!db.env.networkOnline) { await new Promise((r) => setTimeout(r, 300)); return 'nonet'; }
  if (pairing.reach) return pairing.reach(f);
  // Single window: they get it at once and accept a moment later (the two-phone lab, ?lab=pair, lets you answer).
  await new Promise((r) => setTimeout(r, 700));
  setTimeout(() => accepted(f), 2500);
  return 'pending';
}

/** Add someone to my contacts. Core (caps.rs `policy_for`): a contact made from a ticket can call both ways from the
    start; one met through a group request is message-only. `pp.newContactsCall` stays a PROPOSAL for requests. */
export function addPerson(f: { id: string; name: string; hue: number }, how: 'scan' | 'code' | 'request' | 'them'): void {
  const p = person(f.id), wasContact = !!p?.contact;
  const calls = how === 'request' ? pp.newContactsCall : true;
  if (p) { p.contact = true; p.callsAllowed = calls; }
  else db.people.push({ id: f.id, name: f.name, hue: f.hue, online: true, callsAllowed: calls, blocked: false, contact: true });
  if (how !== 'request') pp.how[f.id] = how;
  if (how === 'them') pp.addedAt[f.id] = clock.now;
  if (!wasContact) emitFx({ type: 'line' });   // a new line opened: the same two tugs as Done
}

let rq = 0;
/** Someone used my code: their request arrives (the knock). Returns false if they're already a contact. */
export function incomingRequest(d: { id: string; name: string; hue: number } = DIRECTORY[rq % DIRECTORY.length], open = true) {
  rq++;
  if (person(d.id)?.contact) return false;
  if (!pp.requests.some((r) => r.who === d.id)) pp.requests.push({ id: newId('rq'), who: d.id, name: d.name, hue: d.hue, at: clock.now });
  emitFx({ type: 'request' });
  // The sheet opens over what I'm doing (my own Add contact sheet turns into it); never over a call or another dialog.
  if (open && (pp.dialog === null || pp.dialog === 'addContact')) {
    pp.incoming = { id: d.id, name: d.name, hue: d.hue, phase: 'asking', kind: 'request' }; pp.dialog = 'addContact';
  }
  return true;
}
/** Accept / Not now. Accepting makes them a contact who may message and call (ticket policy); the requester is told.
    "Not now" tells them nothing: their request just stays unanswered (kind to both). */
export function answerRequest(id: string, accept: boolean) {
  const r = pp.requests.find((x) => x.id === id);
  if (!r) return;
  if (accept) addPerson({ id: r.who, name: r.name, hue: r.hue }, 'them');
  // Not now keeps the request in Contacts › "Waiting for you" to answer later (§ 6f); only Accept removes it.
  if (accept) pp.requests = pp.requests.filter((x) => x.id !== id);
  if (pp.incoming?.id === r.who) { if (accept) pp.incoming.phase = 'joined'; else closeDialog(); }
  pairing.answered?.(r, accept);
}

/* ---------- blocking (visible, U5) ---------- */
export const block = (id: string) => { const p = person(id); if (p) p.blocked = true; };
export const unblock = (id: string) => { const p = person(id); if (p) p.blocked = false; };

/* ---------- devices ---------- */
export type NameCheck = { ok: true; name: string } | { ok: false; error: string };
export function checkDeviceName(raw: string, except?: string): NameCheck {
  const name = raw.trim().replace(/\s+/g, ' ');
  if (!name) return { ok: false, error: 'Give the device a name.' };
  if (Array.from(name).length > 32) return { ok: false, error: 'Keep it to 32 characters or fewer.' };
  if (db.devices.some((d) => d.id !== except && d.name.toLowerCase() === name.toLowerCase())) return { ok: false, error: 'Another device already has that name.' };
  return { ok: true, name };
}
export function renameDevice(id: string, raw: string): NameCheck {
  const r = checkDeviceName(raw, id);
  if (r.ok) { const d = db.devices.find((x) => x.id === id); if (d) d.name = r.name; }
  return r;
}
export const unlinkDevice = (id: string) => { db.devices = db.devices.filter((d) => d.id !== id || d.self); };
/** Not seen for this long reads as a nudge (P9 of the devices study, PROPOSED). */
export const STALE_DAYS = 30;
export function lastSeenText(at: number, self: boolean, now = clock.now) {
  if (self) return 'This device';
  const m = Math.max(0, Math.round((now - at) / T.MIN));
  if (m < 2) return 'Last seen just now';
  if (m < 60) return `Last seen ${m} minutes ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `Last seen ${h} ${h === 1 ? 'hour' : 'hours'} ago`;
  const d = Math.round(h / 24);
  return `Last seen ${d} ${d === 1 ? 'day' : 'days'} ago`;
}
export const isStale = (at: number, now = clock.now) => now - at >= STALE_DAYS * T.DAY;

/* ---------- link a new device (this is the EXISTING device; the new one shows the QR and code, per the decided model) ---------- */
/** What the new device displays, and how the other side behaves. Drawer controls. */
export const NEW_DEVICE_CODE = 'OSVL1-7K2M-Q4RX';
export const linkSim = $state({ name: 'Tablet', kind: 'phone' as 'phone' | 'desktop', failure: 'none' as 'none' | 'expired' | 'declined' | 'dropped' });
export type LinkStep = 'scan' | 'verify' | 'auth' | 'transfer' | 'done' | 'failed';
export type LinkFail = 'expired' | 'mismatch' | 'declined' | 'dropped' | 'cancelled';
/** What was and was not shared for each reason, so a failure never leaves the wrong impression. */
export const LINK_FAIL: Record<LinkFail, { title: string; body: string }> = {
  expired: { title: 'The code expired', body: 'Codes last 5 minutes and work once. Nothing was shared. Show a new code on the other device and try again.' },
  mismatch: { title: 'The codes didn’t match', body: 'Nothing was shared. Someone else may have scanned the code first, so that code is now void. Start again on the other device.' },
  declined: { title: 'The other device didn’t accept', body: 'Nothing was shared. Open Tinline on the other device and ask it to link again.' },
  dropped: { title: 'The connection dropped', body: 'The other device may have received your account, but it hasn’t finished setting up. If it appears in your device list later, it linked. Otherwise try again.' },
  cancelled: { title: 'Linking was cancelled', body: 'Nothing was shared.' },
};
export const TRANSFER_STEPS = ['Checking the other device', 'Sending your recovery phrase', 'Setting up the account there', 'Adding it to your devices'];

class LinkFlow {
  step = $state<LinkStep>('scan');
  fail = $state<LinkFail>('cancelled');
  /** Camera viewfinder is open (simulated). */
  scanning = $state(false);
  error = $state('');
  tstep = $state(0);
  name = $state('');
  confirmCode = '482 913';
  #timers: number[] = [];

  reset() { this.#clear(); this.step = 'scan'; this.scanning = false; this.error = ''; this.tstep = 0; this.name = ''; }
  #clear() { this.#timers.forEach(clearTimeout); this.#timers = []; }
  #later(fn: () => void, ms: number) { this.#timers.push(window.setTimeout(fn, ms)); }
  #failed(r: LinkFail) { this.#clear(); this.scanning = false; this.fail = r; this.step = 'failed'; }

  begin() { this.reset(); }
  /** The camera sees the new device's QR (after a beat), or the typed code is accepted. */
  scan() {
    this.error = ''; this.scanning = true;
    this.#later(() => { this.scanning = false; this.#got(); }, 1300);
  }
  stopScan() { this.#clear(); this.scanning = false; }
  typed(raw: string) {
    const c = raw.trim().toUpperCase().replace(/\s+/g, '');
    if (!c) { this.error = 'Type the code shown on the new device.'; return; }
    if (!/^OSVL1-[A-Z0-9]{4}-[A-Z0-9]{4}$/.test(c)) { this.error = 'That isn’t a link code. It starts with OSVL1 and is shown under the QR on the new device.'; return; }
    if (c !== NEW_DEVICE_CODE) { this.error = 'That code isn’t valid. Make a fresh one on the new device.'; return; }
    this.error = ''; this.#got();
  }
  #got() {
    if (linkSim.failure === 'expired') return this.#failed('expired');
    this.name = linkSim.name; this.step = 'verify';
  }
  /** The person says the two six-digit codes are (not) the same. Never automatic. */
  verdict(match: boolean) {
    if (!match) return this.#failed('mismatch');
    if (linkSim.failure === 'declined') return this.#failed('declined');
    this.step = 'auth';
  }
  /** Passphrase accepted (checked by the screen): the transfer plays out. */
  authorized() {
    this.step = 'transfer'; this.tstep = 0;
    const next = () => {
      if (linkSim.failure === 'dropped' && this.tstep === 2) return this.#failed('dropped');
      if (this.tstep >= TRANSFER_STEPS.length - 1) {
        this.tstep = TRANSFER_STEPS.length;
        db.devices.push({ id: newId('dev'), name: this.name, kind: linkSim.kind, self: false, lastSeen: clock.now });
        this.step = 'done';
        return;
      }
      this.tstep++; this.#later(next, 650);
    };
    this.#later(next, 650);
  }
  cancel() { if (this.step === 'transfer' || this.step === 'auth' || this.step === 'verify') this.#failed('cancelled'); else this.reset(); }
}
export const link = new LinkFlow();

/* ---------- things that happen on the other side (drawer) ---------- */
export function otherDeviceUnlinks(id: string) { unlinkDevice(id); }
export function setLastSeen(id: string, ago: number) { const d = db.devices.find((x) => x.id === id); if (d) d.lastSeen = clock.now - ago; }

if (typeof window !== 'undefined') (window as any).__pgPeople = { pp, link, linkSim };
