/* Profile actions: the simulated core adapter (profile-sync contract). Each function mirrors one core call and mutates
   the one store; screens keep only drafts. Limits are the packet's: name <= 256 UTF-8 bytes without control or bidi
   characters, status/about <= 4096 bytes, 64 fields, each JSON value <= 16 KiB, all fields <= 64 KiB. */
import { db, person } from '../store.svelte';
import { clock, T } from '../clock.svelte';
import type { DirectChat, Id, Person, Profile } from '../types';
import { newHash, note, putBlob, px, profileOf } from './profile.svelte';
import { IMG, sniff } from '../images';

export const LIMITS = { name: 256, text: 4096, fields: 64, value: 16 * 1024, all: 64 * 1024, key: 256 } as const;
export type Result = { ok: true } | { ok: false; error: string };
const fail = (error: string): Result => ({ ok: false, error });
const OK: Result = { ok: true };

export const utf8 = (s: string) => new TextEncoder().encode(s).length;
const CONTROL = /[\u0000-\u001f\u007f-\u009f]/;
const BIDI = /[‪-‮⁦-⁩]/;

/** Keys the app edits through their own controls; they are not "additional information". */
export const RESERVED = ['name', 'status', 'about', 'availability', 'picture'] as const;
export const isReserved = (k: string) => (RESERVED as readonly string[]).includes(k);

/* ---------- validation ---------- */
export function checkName(raw: string, what = 'name'): Result {
  const s = raw.trim();
  if (!s) return fail(`The ${what} can’t be empty.`);
  if (CONTROL.test(s) || BIDI.test(s)) return fail(`The ${what} can’t contain control or text-direction characters.`);
  if (utf8(s) > LIMITS.name) return fail(`That ${what} is too long. Up to ${LIMITS.name} bytes; some characters take more than one.`);
  return OK;
}
export function checkText(s: string, what: string): Result {
  if (utf8(s) > LIMITS.text) return fail(`That is too long for ${what}: up to ${LIMITS.text} bytes, and some characters take more than one.`);
  if (/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f]/.test(s) || BIDI.test(s)) return fail(`${what[0].toUpperCase() + what.slice(1)} can’t contain control or text-direction characters.`);
  return OK;
}
export function checkKey(key: string): Result {
  if (!key.trim()) return fail('Give the information a name.');
  if (CONTROL.test(key) || utf8(key) > LIMITS.key) return fail('That name can’t be used. Keep it short, without control characters.');
  return OK;
}
function checkValue(key: string, value: unknown, p: Profile): Result {
  const size = utf8(JSON.stringify(value));
  if (size > LIMITS.value) return fail('That is too long. Each piece of information can be up to 16 KiB.');
  const others = Object.entries(p.fields).filter(([k]) => k !== key);
  if (others.length >= LIMITS.fields && !(key in p.fields)) return fail('You’ve reached 64 pieces of information. Remove one to add another.');
  const total = others.reduce((n, [k, v]) => n + utf8(k) + utf8(JSON.stringify(v)), 0) + utf8(key) + size;
  if (total > LIMITS.all) return fail('Your profile is full. Remove some information to add more.');
  return OK;
}

/* ---------- syncing ---------- */
const SAVE_FAILED = 'Couldn’t save that. Your profile is unchanged and what you typed is still here.';
/** A write the core accepted: it is saved here; if the network is down it waits to sync. No per-contact delivery state exists. */
function accepted(what: string) {
  if (!db.env.networkOnline) px.pending++;
  note(`${what}${db.env.networkOnline ? '' : ' (saved on this device, waiting for the network)'}.`);
}
function failingSave(): boolean {
  if (!px.sim.failNextSave) return false;
  px.sim.failNextSave = false;
  note('A save failed (simulated). Nothing was changed.');
  return true;
}
function mine(): Profile {
  return (db.account.profile ??= { fields: {} });
}

/* ---------- my profile: core calls ---------- */
/** set_name */
export function setName(raw: string): Result {
  const c = checkName(raw);
  if (!c.ok) return c;
  if (failingSave()) return fail(SAVE_FAILED);
  db.account.name = raw.trim();
  accepted('Name saved');
  return OK;
}

/** set_profile_field(key, json_value?): patches ONE key; undefined removes it. The object is never replaced. */
export function setField(key: string, value: unknown | undefined): Result {
  const k = checkKey(key);
  if (!k.ok) return k;
  if (key === 'name') return typeof value === 'string' ? setName(value) : fail(SAVE_FAILED);
  if (key === 'picture') return fail('Use the picture controls for that.');
  const p = mine();
  if (value === undefined) {
    if (key === 'availability') return fail('Availability can be changed but not removed.');
    if (!(key in p.fields)) return OK;
    if (failingSave()) return fail(SAVE_FAILED);
    delete p.fields[key];
    accepted(`Removed “${key}”`);
    return OK;
  }
  if (key === 'status' || key === 'about') {
    if (typeof value !== 'string') return fail(SAVE_FAILED);
    const c = checkText(value, key === 'status' ? 'a status' : 'about');
    if (!c.ok) return c;
  }
  const v = checkValue(key, value, p);
  if (!v.ok) return v;
  if (failingSave()) return fail(SAVE_FAILED);
  p.fields[key] = value;
  accepted(`Saved “${key}”`);
  return OK;
}

export type Avail = { available: boolean; until?: number };
/** set_available */
export function setAvailability(a: Avail): Result {
  if (!a.available && a.until !== undefined && a.until <= clock.now) return fail('Choose a time that hasn’t passed yet.');
  if (failingSave()) return fail(SAVE_FAILED);
  const v: Avail = a.available ? { available: true } : a.until === undefined ? { available: false } : { available: false, until: a.until };
  mine().fields.availability = v;
  db.account.availability = a.available ? 'available' : 'busy';
  accepted(a.available ? 'Availability: available' : 'Availability: busy');
  return OK;
}

/** What an availability field means at this moment: a deadline that has passed means Available again. */
export function effectiveAvailability(owner: Id, now = clock.now): { available: boolean; until?: number } {
  const a = profileOf(owner)?.fields.availability as Avail | undefined;
  if (!a || a.available) return { available: true };
  if (a.until !== undefined && a.until <= now) return { available: true };
  return { available: false, until: a.until };
}

/** replace_profile_picture(bytes, true): the previous hash is requested for deletion automatically. Never optional. */
export async function replacePicture(bytes: Blob): Promise<Result> {
  if (bytes.size > IMG.outMaxBytes) return fail('That picture is too detailed to save. Try a different one.');
  if ((await sniff(bytes)) !== 'png') return fail('Tinline can only save PNG, JPEG or WebP pictures.');
  if (failingSave()) return fail('Couldn’t save your picture. Your current picture is unchanged.');
  const p = mine();
  const prev = p.picture?.hash;
  const hash = newHash();
  putBlob(hash, bytes);
  p.picture = { hash, ready: true };               // one atomic swap: the old image stops being "selected" immediately
  if (prev) requestDeletion('me', prev);
  accepted('Picture replaced');
  return OK;
}
/** remove_profile_picture(true) */
export function removePicture(): Result {
  const p = mine();
  const prev = p.picture?.hash;
  if (!prev) return OK;
  if (failingSave()) return fail('Couldn’t remove your picture. It is unchanged.');
  delete p.picture;
  requestDeletion('me', prev);
  accepted('Picture removed');
  return OK;
}

/** The owner-signed request, bound to ONE previous hash. Only ever recorded as "requested": there is no completion state. */
function requestDeletion(owner: Id, hash: string) {
  px.deletions.unshift({ owner, hash, at: clock.now });
  note(`Deletion of the previous cached picture (${hash.slice(0, 10)}) requested for ${owner === 'me' ? 'my' : 'their'} linked devices and contacts.`);
}
/** Requested, or deferred while a concurrent edit has that same hash selected again. */
export const deletionState = (d: { owner: Id; hash: string }) => (profileOf(d.owner)?.picture?.hash === d.hash ? 'deferred' : 'requested');

/* ---------- my local alias for someone (never sent anywhere) ---------- */
export function setAlias(id: Id, raw: string): Result {
  const p = person(id);
  if (!p) return fail('That person isn’t here any more.');
  const s = raw.trim();
  if (!s) { delete p.alias; return OK; }
  const c = checkName(s, 'nickname');
  if (!c.ok) return c;
  p.alias = s;
  return OK;
}

/** Open (creating if needed) the 1:1 chat with a contact. Viewing a profile never calls this. */
export function ensureChat(id: Id): DirectChat | undefined {
  const p = person(id);
  if (!p || !p.contact) return undefined;
  let c = db.chats.find((x): x is DirectChat => x.kind === 'direct' && x.person === id);
  if (!c) { db.chats.push({ kind: 'direct', id, person: id, messages: [], unread: 0, muted: false, draft: '' }); c = db.chats[db.chats.length - 1] as DirectChat; }
  return c;
}

/* ---------- other people: what arrives from the network ---------- */
export interface RemotePatch {
  name?: string;
  fields?: Record<string, unknown | undefined>;       // undefined removes
  picture?: { hash: string; ready: boolean } | null;  // null clears
}

/** Updates reach me through the network and only from people I haven't blocked. Never a message, unread count or notification. */
function arrive(owner: Id, label: string, run: () => void) {
  const p = person(owner);
  if (!p) return;
  if (p.blocked) { note(`Dropped ${label}: ${p.name} is blocked, so their updates stop.`); return; }
  if (!db.env.networkOnline) { px.inbound.push({ label, run }); note(`Queued ${label} until the network is back.`); return; }
  run();
}

/** on_profile_changed(did): a contact's profile changed. Patches apply per key, like the shared document. */
export function remoteUpdate(owner: Id, patch: RemotePatch, label = 'a profile update') {
  arrive(owner, `${label} from ${person(owner)?.name ?? owner}`, () => {
    const p = person(owner);
    if (!p) return;
    const prof = (p.profile ??= { fields: {} });
    if (patch.name !== undefined) p.name = patch.name;
    for (const [k, v] of Object.entries(patch.fields ?? {})) { if (v === undefined) delete prof.fields[k]; else prof.fields[k] = v; }
    if (patch.picture !== undefined) {
      const prev = prof.picture?.hash;
      if (patch.picture === null) delete prof.picture; else prof.picture = { ...patch.picture };
      /* The owner's automatic request for the previous hash: honoured here, scoped to this owner's cached copy. */
      if (prev && prev !== prof.picture?.hash) note(`Honoured ${p.name}’s request to delete their previous cached picture (${prev.slice(0, 10)}).`);
    }
    note(`Profile changed: ${p.name}.`);
  });
}

/** Their picture bytes finished arriving. Ignored unless that hash is still the selected one. */
export function remotePictureReady(owner: Id, hash: string, label = 'a picture') {
  arrive(owner, `${label} from ${person(owner)?.name ?? owner}`, () => {
    const prof = person(owner)?.profile;
    if (!prof?.picture || prof.picture.hash !== hash) { note(`Ignored ${label} for ${person(owner)?.name ?? owner}: it is no longer the selected picture.`); return; }
    prof.picture = { hash, ready: true };
    note(`Picture ready: ${person(owner)?.name}.`);
  });
}

/** Linked-device edits to MY profile (another install of my account). Same per-key merge; waits for the network too. */
export function siblingUpdate(patch: RemotePatch & { availability?: Avail }, label: string) {
  const run = () => {
    const p = mine();
    if (patch.name !== undefined) db.account.name = patch.name;
    for (const [k, v] of Object.entries(patch.fields ?? {})) { if (v === undefined) delete p.fields[k]; else p.fields[k] = v; }
    if (patch.availability) { p.fields.availability = patch.availability; db.account.availability = patch.availability.available ? 'available' : 'busy'; }
    if (patch.picture !== undefined) {
      const prev = p.picture?.hash;
      if (patch.picture === null) delete p.picture; else p.picture = { ...patch.picture };
      if (prev && prev !== p.picture?.hash) requestDeletion('me', prev);
    }
    note(`My other device: ${label}.`);
  };
  if (!db.env.networkOnline) { px.inbound.push({ label, run }); note(`Queued my other device’s change (${label}) until the network is back.`); return; }
  run();
}

/* ---------- helpers for screens ---------- */
export const BUSY_CHOICES: { id: string; label: string; until: (now: number) => number | undefined }[] = [
  { id: '30m', label: '30 minutes', until: (n) => n + 30 * T.MIN },
  { id: '1h', label: '1 hour', until: (n) => n + T.HOUR },
  { id: '4h', label: '4 hours', until: (n) => n + 4 * T.HOUR },
  { id: 'tom', label: 'Tomorrow morning', until: (n) => { const d = new Date(n); d.setDate(d.getDate() + 1); d.setHours(8, 0, 0, 0); return d.getTime(); } },
  { id: 'open', label: 'No end time', until: () => undefined },
];

/** Called by the drawer to add a person the chat list can show (new contact scenario). */
export function addContact(id: Id, name: string, hue: number): Person {
  const existing = person(id);
  if (existing) return existing;
  db.people.push({ id, name, hue, online: true, callsAllowed: true, blocked: false, contact: true });
  ensureChat(id);
  note(`New contact: ${name}. No profile has arrived yet.`);
  return db.people[db.people.length - 1];
}
