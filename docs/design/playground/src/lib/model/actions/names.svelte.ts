/* Names on this device (2026-10-14): several identities online at once, each its own address with its own contacts
   and chats. One is OPEN (what the screens show); the others stay online in the background unless taken offline, so
   their calls and messages still come through. Switching is instant: an online name is already unlocked.
   Safe limits are Proposed (store.NAME_LIMIT): phone 3, desktop 5. */
import { db, NAME_LIMIT, onlineNames } from '../store.svelte';
import type { OtherName } from '../types';
import { prefs } from '../prefs.svelte';
import { nav } from '../../nav/router.svelte';
import { seedOtherName } from '../seed';

export const nameLimit = () => NAME_LIMIT[prefs.form];
export const roomForAnother = () => onlineNames() < nameLimit();

/** The open name, packed the way the others are kept. */
function packOpen(online = true): OtherName {
  return {
    id: db.account.id, name: db.account.name, hue: db.account.hue, profile: db.account.profile,
    online, remembered: false,
    data: { people: db.people, chats: db.chats, devices: db.devices },
  };
}
function unpack(o: OtherName) {
  db.account.id = o.id; db.account.name = o.name; db.account.hue = o.hue; db.account.profile = o.profile;
  db.people = o.data.people; db.chats = o.data.chats; db.devices = o.data.devices;
}

/** Open another name. The one that was open stays online. An offline name comes online if there is room. */
export function switchTo(i: number): boolean {
  const o = db.others[i];
  if (!o) return false;
  if (!o.online && !roomForAnother()) return false;
  const cur = packOpen(true);
  unpack(o);
  db.others[i] = cur;
  nav.reset();   // the open chat belonged to the other name
  return true;
}
/** Kept for the unlock screen: before unlock "switching" only picks which name to open. */
export const switchAccount = (i: number) => { switchTo(i); };

export function setOnline(i: number, on: boolean): boolean {
  const o = db.others[i];
  if (!o) return false;
  if (on && !o.online && !roomForAnother()) return false;
  o.online = on;
  return true;
}
/** Removes the name from this device only: it stays on its other devices and comes back with its 24 words or key. */
export function removeName(i: number) { db.others.splice(i, 1); }

/** A name just added (sign-up / restore / link from Settings): it opens, and the one that was open stays online. */
export function addOpenedName(name: string) {
  db.others.push(packOpen(true));
  db.account.id = 'name-' + Date.now().toString(36);
  db.account.name = name;
  db.account.hue = (db.account.hue + 2) % 6;
  db.account.profile = undefined;
  db.people = []; db.chats = [];
  nav.reset();
}

/** Simulation: put a sample name on this device. */
export function simAddSample(which: 'studio' | 'home') {
  if (db.others.some((o) => o.id === 'name-' + which) || db.account.id === 'name-' + which) return;
  const o = seedOtherName(which);
  o.online = roomForAnother();
  db.others.push(o);
}

/** Test hook for headless captures (not used by the app). */
if (typeof window !== 'undefined') (window as any).__names = { simAddSample, switchTo, setOnline };
