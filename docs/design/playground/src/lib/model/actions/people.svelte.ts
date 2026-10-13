/* People, devices and settings (PLAN step 8). The state the Contacts tab, Devices, Privacy and Settings share, plus the
   rules they follow. The people and devices themselves stay in the store (db.people, db.devices); this module holds only
   what is not app data: open dialogs, OS permission state, notification switches, the link-a-device flow. Screens read it;
   the Simulation drawer (PeopleSim) drives the "outside" parts (a request arrives, a permission is denied, the new device answers).
   Nothing here talks to a network, camera or system. */
import { db, newId, person } from '../store.svelte';
import { clock, T } from '../clock.svelte';

export type Perm = 'granted' | 'denied';
export interface Request { id: string; who: string; name: string; hue: number; at: number }

export const pp = $state({
  /** Dialogs drawn above the whole app (PeopleLayers): the new-chat picker and the add-contact sheet. */
  dialog: null as null | 'newChat' | 'addContact',
  addTab: 'scan' as 'scan' | 'mine' | 'paste',
  /** OS permissions the app has been given. Android also has battery optimisation. */
  perms: { notifications: 'granted' as Perm, microphone: 'granted' as Perm, camera: 'granted' as Perm, battery: 'ok' as 'ok' | 'restricted' },
  /** Notification switches (mine, per account on this device). */
  notif: { messages: true, calls: true, groups: true },
  /** PROPOSED: what a new contact may do before I say. The core only has the per-contact Allow calls. */
  newContactsCall: false,
  /** People who scanned my code or sent me a request and wait for my answer. */
  requests: [] as Request[],
  /** How I added a contact. Absent = unknown (older contacts). */
  how: {} as Record<string, 'scan' | 'code'>,
});

export function resetPeopleState() {
  pp.dialog = null; pp.addTab = 'scan';
  pp.perms.notifications = 'granted'; pp.perms.microphone = 'granted'; pp.perms.camera = 'granted'; pp.perms.battery = 'ok';
  pp.notif.messages = true; pp.notif.calls = true; pp.notif.groups = true;
  pp.newContactsCall = false; pp.requests = []; pp.how = {};
  link.reset(); linkSim.failure = 'none'; linkSim.name = 'Tablet';
}

export const open = (d: 'newChat' | 'addContact', tab: 'scan' | 'mine' | 'paste' = 'scan') => { pp.addTab = tab; pp.dialog = d; };
export const closeDialog = () => { pp.dialog = null; };

/* ---------- contacts ---------- */
export const MY_CODE = 'TIN-MAYA-7K2Q';
/** People the simulated scanner / pasted codes can resolve to. */
export const DIRECTORY = [
  { id: 'tara', key: 'TARA', name: 'Tara Iyer', hue: 3 },
  { id: 'nikhil', key: 'NIKH', name: 'Nikhil Das', hue: 1 },
  { id: 'isha', key: 'ISHA', name: 'Isha Verma', hue: 5 },
];
export type Found = { ok: true; id: string; name: string; hue: number; already: boolean; blocked: boolean } | { ok: false; error: string };

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
  const c = raw.trim().toUpperCase().replace(/\s+/g, '');
  if (!c) return { ok: false, error: 'Paste or type the code first.' };
  if (!/^TIN-[A-Z0-9]{4}-[A-Z0-9]{4}$/.test(c)) return { ok: false, error: 'That doesn’t look like a Tinline code. Codes look like TIN-7K2M-Q4RX.' };
  if (c === MY_CODE) return { ok: false, error: 'That is your own code. Share it with someone else.' };
  const d = DIRECTORY.find((x) => c.startsWith('TIN-' + x.key + '-'));
  if (d) return found(d);
  const h = [...c].reduce((n, ch) => n + ch.charCodeAt(0), 0);
  return found(DIRECTORY[h % DIRECTORY.length]);
}

/** Add someone to my contacts. They start message-only unless I chose otherwise in Privacy (PROPOSED default). */
export function addPerson(f: { id: string; name: string; hue: number }, how: 'scan' | 'code' | 'request'): void {
  const p = person(f.id);
  if (p) { p.contact = true; }
  else db.people.push({ id: f.id, name: f.name, hue: f.hue, online: true, callsAllowed: pp.newContactsCall, blocked: false, contact: true });
  if (how !== 'request') pp.how[f.id] = how;
}

let rq = 0;
export function incomingRequest(d = DIRECTORY[rq % DIRECTORY.length]) {
  rq++;
  if (person(d.id)?.contact || pp.requests.some((r) => r.who === d.id)) return;
  pp.requests.push({ id: newId('rq'), who: d.id, name: d.name, hue: d.hue, at: clock.now });
}
export function answerRequest(id: string, accept: boolean) {
  const r = pp.requests.find((x) => x.id === id);
  if (!r) return;
  if (accept) addPerson({ id: r.who, name: r.name, hue: r.hue }, 'request');
  pp.requests = pp.requests.filter((x) => x.id !== id);
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
