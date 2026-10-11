/* The one data set. Merged from refined/home.html, chat.html and groups.html, which had drifted
   (different spellings, times, unread counts, member counts). Every screen reads this through the store. */
import type { Account, Chat, Device, Group, Message, Person } from './types';
import { clock, T } from './clock.svelte';

const at = (h: number, m: number, daysAgo = 0) => {
  const d = new Date(clock.ago(0));
  d.setDate(d.getDate() - daysAgo);
  d.setHours(h, m, 0, 0);
  return d.getTime();
};

let seq = 0;
const msg = (m: Omit<Message, 'id'>): Message => ({ id: 'm' + ++seq, ...m });

export function seedAccount(): Account {
  return { id: 'me', name: 'Maya Kurian', hue: 0, availability: 'available', gate: 'ready' };
}

export function seedPeople(): Person[] {
  const p = (id: string, name: string, hue: number, extra: Partial<Person> = {}): Person =>
    ({ id, name, hue, online: true, callsAllowed: true, blocked: false, contact: true, ...extra });
  return [
    p('arjun', 'Arjun Oommen', 5),
    p('meera', 'Meera Nair', 4, { online: false }),
    p('dev', 'Dev Patel', 1, { callsAllowed: false }), // met through a group request: message-only until he allows calls
    p('sana', 'Sana Rao', 1),
    p('sam', 'Sam Rao', 3),
    p('priya', 'Priya Menon', 2),
    p('leo', 'Leo Barreto', 0, { contact: false }),
    p('kavya', 'Kavya Shetty', 2, { contact: false }),
    p('nisha', 'Nisha Pillai', 4, { contact: false }),
    p('amma', 'Amma', 3),
    p('ravi', 'Ravi Menon', 0, { blocked: true }),
  ];
}

export function seedChats(): Chat[] {
  const direct = (person: string, messages: Message[], extra: Partial<Chat> = {}): Chat =>
    ({ kind: 'direct', id: person, person, messages, unread: 0, muted: false, draft: '', ...extra } as Chat);

  const hike: Group = {
    kind: 'group', id: 'hike', name: 'Weekend hike', status: 'active', unread: 3, muted: false, draft: '',
    members: [
      { id: 'meera', role: 'owner', host: false, callHost: false, since: at(9, 0, 59) },
      { id: 'arjun', role: 'member', host: true, callHost: true, since: at(9, 0, 59) },
      { id: 'sam', role: 'admin', host: false, callHost: false, since: at(9, 0, 59) },
      { id: 'me', role: 'admin', host: false, callHost: false, since: at(9, 0, 53) },
      { id: 'priya', role: 'admin', host: false, callHost: false, since: at(9, 0, 52) },
      { id: 'leo', role: 'member', host: false, callHost: false, since: at(9, 0, 40), invitedBy: 'sam' },
      { id: 'dev', role: 'member', host: false, callHost: false, since: at(9, 0, 37), invitedBy: 'sam' },
      { id: 'kavya', role: 'member', host: false, callHost: false, since: at(9, 0, 3), invitedBy: 'priya' },
      { id: 'nisha', role: 'member', host: false, callHost: false, since: at(9, 0, 0), invitedBy: 'priya' },
    ],
    invited: [], flags: { leo: ['sam'] },
    messages: [
      msg({ from: 'kavya', at: at(9, 2), text: 'Kavya joined, invited by Priya', system: true }),
      msg({ from: 'sam', at: at(12, 30), text: 'Saturday 7am at the trailhead.' }),
      msg({ from: 'sam', at: at(12, 31), text: 'Bring rain jackets, the forecast says showers after 10.' }),
      msg({ from: 'sam', at: at(12, 31), text: 'And snacks for the top.' }),
      msg({ from: 'me', at: at(12, 33), text: 'I can bring the stove.', delivery: 'delivered' }),
      msg({ from: 'priya', at: at(12, 38), text: 'Can someone give me a lift from the station?' }),
      msg({ from: 'kavya', at: at(12, 52), text: 'First hike with you all. Is the loop fine for beginners?' }),
      msg({ from: 'sam', at: at(12, 55), text: 'It is. Easy grade, one steep bit near the top.' }),
    ],
  };

  const family: Group = {
    kind: 'group', id: 'family', name: 'Family', status: 'active', unread: 4, muted: true, draft: '',
    members: [
      { id: 'me', role: 'owner', host: true, callHost: true, since: at(10, 0, 280) },
      { id: 'amma', role: 'admin', host: false, callHost: false, since: at(10, 0, 280) },
      { id: 'meera', role: 'member', host: false, callHost: false, since: at(10, 0, 280) },
      { id: 'arjun', role: 'member', host: false, callHost: false, since: at(10, 0, 279) },
      { id: 'priya', role: 'member', host: false, callHost: false, since: at(10, 0, 274) },
    ],
    invited: [], flags: {},
    messages: [
      msg({ from: 'me', at: at(18, 5, 1), text: 'Photos from Sunday', delivery: 'delivered' }),
      msg({ from: 'amma', at: at(19, 40, 1), text: 'So nice! Everyone looks happy' }),
      msg({ from: 'meera', at: at(20, 2, 1), text: 'Send the one by the lake please' }),
      msg({ from: 'amma', at: at(21, 10, 1), text: 'Lunch is at 1 on Sunday' }),
    ],
  };

  const book: Group = {
    kind: 'group', id: 'book', name: 'Book club', status: 'removed', unread: 0, muted: false, draft: '',
    members: [], invited: [], flags: { me: ['sam', 'meera', 'priya'] },
    messages: [
      msg({ from: 'sam', at: at(20, 0, 16), text: 'Chapter 6 for next week.' }),
      msg({ from: 'me', at: at(20, 30, 15), text: 'I’ll bring snacks', delivery: 'delivered' }),
      msg({ from: 'sam', at: at(14, 2, 14), text: 'You were removed from the group', system: true }),
    ],
  };

  const after6 = msg({ from: 'me', at: at(12, 38), text: 'Yes, after 6 works for me', delivery: 'delivered', edited: true });
  return [
    direct('arjun', [
      msg({ from: 'arjun', at: at(21, 12, 1), text: 'Did you get home ok?' }),
      msg({ from: 'me', at: at(21, 14, 1), text: 'Yes, train was on time for once', delivery: 'delivered' }),
      msg({ from: 'me', at: at(21, 15, 1), text: 'Thanks for dinner. That place was great', delivery: 'delivered' }),
      msg({ from: 'arjun', at: at(21, 20, 1), text: 'Anytime 🙂' }),
      msg({ from: 'me', at: at(21, 32, 1), text: '', call: { kind: 'voice', outcome: 'answered', durationS: 742, direction: 'out' } }),
      msg({ from: 'arjun', at: at(12, 36), text: 'Are we still on for the call about the flat?' }),
      after6,
      msg({ from: 'me', at: at(12, 39), text: 'I’ll call from the office, it’s quieter there', delivery: 'delivered' }),
      msg({ from: 'arjun', at: at(12, 41), text: 'I have the landlord’s numbers now', replyTo: after6.id }),
    ], { unread: 2 }),
    hike,
    direct('dev', [
      msg({ from: 'me', at: at(11, 2), text: 'Free this week?', delivery: 'delivered' }),
      msg({ from: 'dev', at: at(11, 5), text: 'Thursday works. The place near the station?' }),
    ], { draft: 'Lunch Thursday?' }),
    direct('sana', [
      msg({ from: 'sana', at: at(8, 20), text: 'Sent the draft to your laptop too' }),
      msg({ from: 'sana', at: at(9, 2), text: '', call: { kind: 'voice', outcome: 'missed', durationS: 0, direction: 'in' } }),
    ], { unread: 1 }),
    family,
    direct('meera', [
      msg({ from: 'meera', at: at(17, 40, 1), text: 'Can you send the scan when you’re back online?' }),
      msg({ from: 'me', at: at(17, 46, 1), text: 'Running 10 minutes late, sorry', delivery: 'waiting' }),
      msg({ from: 'me', at: at(17, 50, 1), text: '', call: { kind: 'voice', outcome: 'failed', durationS: 0, direction: 'out' } }),
    ]),
    book,
  ];
}

export function seedDevices(): Device[] {
  return [
    { id: 'pixel', name: 'Pixel 8', kind: 'phone', self: true, lastSeen: clock.ago(0) },
    { id: 'laptop', name: 'Work laptop', kind: 'desktop', self: false, lastSeen: clock.ago(3 * T.HOUR) },
  ];
}
