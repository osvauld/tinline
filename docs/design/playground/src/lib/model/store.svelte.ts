/* The single store. Deeply reactive ($state), so screens read it directly and mutations
   anywhere update every view in place. Area-specific actions live in ./actions/<area>.ts and
   mutate `db` through the helpers here; screens never keep their own copies of this data. */
import type { Account, OtherName, Chat, Device, DirectChat, Group, Id, Message, Person } from './types';
import { seedAccount, seedChats, seedDevices, seedPeople } from './seed';
import { clock } from './clock.svelte';
import { resetGroupsState } from './actions/groups.svelte';
import { resetProfileState } from './actions/profile.svelte';
import { resetPeopleState } from './actions/people.svelte';

function fresh() {
  return {
    account: seedAccount() as Account,
    /** Other names on this install (restore or "Add another name" ADDS one). Several are online at once. */
    others: [] as OtherName[],
    people: seedPeople() as Person[],
    chats: seedChats() as Chat[],
    devices: seedDevices() as Device[],
    /** Simulated environment, set from the Simulation drawer. */
    env: {
      networkOnline: true,      // this device has a connection
      failNextSend: false,
      hostsOnline: true,        // group hosts reachable
    },
  };
}

export const db = $state(fresh());

export function resetDb() {
  const f = fresh();
  db.account = f.account; db.others = f.others; db.people = f.people; db.chats = f.chats; db.devices = f.devices; db.env = f.env;
  resetGroupsState();
  resetProfileState();
  resetPeopleState();
}

let uid = 1000;
export const newId = (p = 'x') => p + ++uid;

/* ---------- lookups ---------- */
/* Lookups fall back to the other names' data: a call or message can reach a name that is not open (it shows "To Studio"). */
export const person = (id: Id): Person | undefined => db.people.find((p) => p.id === id) ?? ownerOf(id)?.data.people.find((p) => p.id === id);
export const chat = (id: Id | null | undefined): Chat | undefined => (id ? db.chats.find((c) => c.id === id) ?? ownerOf(id)?.data.chats.find((c) => c.id === id) : undefined);
/** The other (not open) name that knows this person or chat, if the open one doesn't. */
export function ownerOf(id: Id): OtherName | undefined {
  if (db.people.some((p) => p.id === id) || db.chats.some((c) => c.id === id)) return undefined;
  return db.others.find((o) => o.data.people.some((p) => p.id === id) || o.data.chats.some((c) => c.id === id));
}
/** Names online at once on this install (Proposed safe limits; the desktop also hosts group calls). */
export const NAME_LIMIT = { android: 3, desktop: 5 } as const;
export const onlineNames = () => 1 + db.others.filter((o) => o.online).length;
/** New messages waiting under another name (muted chats don't count, as on the tab badge). */
export const unreadOf = (o: OtherName) => (o.online ? o.data.chats.reduce((n, c) => n + (c.muted ? 0 : c.unread), 0) : 0);
export const isGroup = (c: Chat | undefined): c is Group => c?.kind === 'group';
export const isDirect = (c: Chat | undefined): c is DirectChat => c?.kind === 'direct';

export function displayName(id: Id) {
  if (id === 'me') return 'You';
  const p = person(id);
  return p?.alias || p?.name || id; // my own nickname for them wins everywhere I see them (their profile name stays on their profile)
}
export const firstName = (id: Id) => (id === 'me' ? 'You' : displayName(id).split(' ')[0]);

export function chatTitle(c: Chat) {
  return c.kind === 'group' ? c.name : displayName(c.person);
}

export const lastMessage = (c: Chat): Message | undefined => c.messages[c.messages.length - 1];

/** Real timestamps, so a message after midnight sorts to the top (audit fix: minutes-of-day sort). */
export function chatsByRecent(list: Chat[] = db.chats) {
  return [...list].sort((a, b) => (lastMessage(b)?.at ?? 0) - (lastMessage(a)?.at ?? 0));
}

export function initials(name: string) {
  const parts = name.trim().split(/\s+/);
  return ((parts[0]?.[0] ?? '') + (parts.length > 1 ? parts[parts.length - 1][0] : '')).toUpperCase();
}

/* ---------- common mutations ---------- */
export function pushMessage(c: Chat, m: Omit<Message, 'id' | 'at'> & { at?: number }) {
  const full: Message = { id: newId('m'), at: clock.now, ...m };
  c.messages.push(full);
  return c.messages[c.messages.length - 1]; // the reactive proxy, not the plain object
}

export function markRead(c: Chat) { c.unread = 0; }
