/* The single store. Deeply reactive ($state), so screens read it directly and mutations
   anywhere update every view in place. Area-specific actions live in ./actions/<area>.ts and
   mutate `db` through the helpers here; screens never keep their own copies of this data. */
import type { Account, Chat, Device, DirectChat, Group, Id, Message, Person } from './types';
import { seedAccount, seedChats, seedDevices, seedPeople } from './seed';
import { clock } from './clock.svelte';
import { resetGroupsState } from './actions/groups.svelte';

function fresh() {
  return {
    account: seedAccount() as Account,
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
  db.account = f.account; db.people = f.people; db.chats = f.chats; db.devices = f.devices; db.env = f.env;
  resetGroupsState();
}

let uid = 1000;
export const newId = (p = 'x') => p + ++uid;

/* ---------- lookups ---------- */
export const person = (id: Id): Person | undefined => db.people.find((p) => p.id === id);
export const chat = (id: Id | null | undefined): Chat | undefined => (id ? db.chats.find((c) => c.id === id) : undefined);
export const isGroup = (c: Chat | undefined): c is Group => c?.kind === 'group';
export const isDirect = (c: Chat | undefined): c is DirectChat => c?.kind === 'direct';

export function displayName(id: Id) {
  if (id === 'me') return 'You';
  return person(id)?.name ?? id;
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
