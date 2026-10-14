/* Chat actions: sending, editing, deleting, files, incoming messages, and the simulated transport.
   Screens call these; they mutate `db` (the one store). Core rules enforced here, not only in the UI:
   group posts are text + reply only (no attach, no edit, no delete); a removed/left group is read-only. */
import { db, chat, person, pushMessage, markRead, isGroup, isDirect } from '../store.svelte';
import type { Chat, FileInfo, Id, Message, CallRecord } from '../types';
import { clock } from '../clock.svelte';
import { emitFx } from '../../fx/bus';
import { timeScale } from '../../fx/motion';
import { nav } from '../../nav/router.svelte';

/** Simulated transport timing (sim parameters, not animation timings). */
export const SIM = {
  tickMs: 250,           // transport loop period; also the progress bar's linear transition
  sendMs: 700,           // 'sending' → delivered/waiting/failed
  fileMinMs: 3000,       // a file never transfers faster than this
  fileMaxMs: 12000,
  undoMs: 5000,          // how long "Undo" is offered after a delete
} as const;

/* ---------- rules ---------- */
export const canPost = (c: Chat) => (isGroup(c) ? c.status === 'active' : true);
export const canAttach = (c: Chat) => isDirect(c);
export const canEdit = (c: Chat, m: Message) =>
  isDirect(c) && m.from === 'me' && !!m.text && !m.file && !m.call && !m.deleted && !m.system && m.delivery !== 'sending';
export const canDelete = (c: Chat, m: Message) => isDirect(c) && m.from === 'me' && !m.call && !m.deleted && !m.system;

/** Can a message from me reach its destination right now? Direct: this device AND the peer online. Group: a host online. */
export function reachable(c: Chat): boolean {
  if (!db.env.networkOnline) return false;
  if (isGroup(c)) return db.env.hostsOnline;
  return !!person(c.person)?.online;
}

/** The reason a message is waiting, for status lines and Message info. */
export function waitingReason(c: Chat): string {
  if (!db.env.networkOnline) return 'This device is offline';
  if (isGroup(c)) return 'Waiting for a host';
  return `Waiting for ${person(c.person)?.name.split(' ')[0] ?? 'them'} to be online`;
}

/* ---------- sending ---------- */
const sentAt = new Map<Id, number>(); // message id → performance.now() when it entered 'sending'
const stamp = (id: Id) => sentAt.set(id, performance.now());

export function sendText(c: Chat, text: string, replyTo?: Id): Message | null {
  text = text.trim();
  if (!text || !canPost(c)) return null;
  const m = pushMessage(c, { from: 'me', text, delivery: 'sending', replyTo });
  stamp(m.id);
  c.draft = '';
  emitFx({ type: 'sent' });
  return m;
}

export function editMessage(c: Chat, m: Message, text: string) {
  text = text.trim();
  if (!text || !canEdit(c, m)) return false;
  if (text !== m.text) { m.text = text; m.edited = true; m.editedAt = clock.now; }
  return true;
}

export function retryMessage(c: Chat, m: Message) {
  if (m.delivery !== 'failed') return;
  m.delivery = 'sending';
  stamp(m.id);
  if (m.file) { m.file.progress = 0; m.file.state = reachable(c) ? 'sending' : 'waiting'; }
}

/* ---------- files (1:1 only) ---------- */
export const SAMPLE_FILES: { name: string; size: number; kind: FileInfo['kind']; w?: number; h?: number }[] = [
  { name: 'holiday-coast.jpg', size: 2.4e6, kind: 'image', w: 4000, h: 3000 },
  { name: 'flat-lease.pdf', size: 6.4e5, kind: 'doc' },
  { name: 'wedding-clip.mp4', size: 48e6, kind: 'video' },
];

export function attachFile(c: Chat, f: { name: string; size: number; kind: FileInfo['kind'] }, replyTo?: Id): Message | null {
  if (!canAttach(c) || !canPost(c)) return null;
  const m = pushMessage(c, {
    from: 'me', text: '', delivery: 'sending', replyTo,
    file: { ...f, progress: 0, state: reachable(c) ? 'sending' : 'waiting' },
  });
  stamp(m.id);
  emitFx({ type: 'sent' });
  return m;
}

export function cancelFile(m: Message) {
  if (!m.file || (m.file.state !== 'sending' && m.file.state !== 'waiting')) return;
  m.file.state = 'cancelled';
  m.delivery = 'failed';
}

/* ---------- delete (1:1 only) with undo ---------- */
export interface Undo { undo(): void; commit(): void }

/** 'me' removes the row from this device; 'everyone' leaves a tombstone. Undo restores until commit(). */
export function deleteMessage(c: Chat, m: Message, scope: 'me' | 'everyone'): Undo | null {
  if (!canDelete(c, m)) return null;
  if (scope === 'me') {
    const i = c.messages.findIndex((x) => x.id === m.id);
    if (i < 0) return null;
    const [removed] = c.messages.splice(i, 1);
    let done = false;
    return {
      undo() { if (!done) { done = true; c.messages.splice(Math.min(i, c.messages.length), 0, removed); } },
      commit() { done = true; },
    };
  }
  const keep = { text: m.text, file: m.file ? { ...m.file } : undefined, edited: m.edited };
  m.deleted = true;
  let done = false;
  return {
    undo() { if (!done) { done = true; m.deleted = false; m.text = keep.text; m.file = keep.file; m.edited = keep.edited; } },
    commit() { if (!done) { done = true; m.text = ''; m.file = undefined; } },
  };
}

/* ---------- incoming (simulation + future real transport) ---------- */
export function receiveText(chatId: Id, from: Id, text: string): Message | null {
  const c = chat(chatId);
  if (!c) return null;
  const m = pushMessage(c, { from, text });
  if (nav.openChat !== c.id) c.unread++;
  emitFx({ type: 'received' });
  return m;
}

export function receiveFile(chatId: Id, from: Id, f: { name: string; size: number; kind: FileInfo['kind'] }): Message | null {
  const c = chat(chatId);
  if (!c || !isDirect(c)) return null;
  const m = pushMessage(c, { from, text: '', file: { ...f, progress: 1, state: 'done' } });
  emitFx({ type: 'received' });
  if (nav.openChat !== c.id) c.unread++;
  return m;
}

/** Visual-only call record, until the call machine (PLAN step 3) writes real ones. */
export function addCallRecord(chatId: Id, rec: CallRecord) {
  const c = chat(chatId);
  if (!c) return;
  const from = rec.direction === 'out' ? 'me' : isDirect(c) ? c.person : 'me';
  pushMessage(c, { from, text: '', call: rec });
  if (rec.direction === 'in' && rec.outcome === 'missed' && nav.openChat !== c.id) c.unread++;
}

export const openChatRead = (c: Chat) => markRead(c);

/** The message the "N new messages" divider sits above: the Nth newest incoming message. */
export function firstUnreadId(c: Chat, n: number): Id | null {
  if (n <= 0) return null;
  let seen = 0;
  for (let i = c.messages.length - 1; i >= 0; i--) {
    const m = c.messages[i];
    if (m.from === 'me' || m.system) continue;
    if (++seen === n) return m.id;
  }
  return null;
}

/* ---------- simulated transport ---------- */
function stepMessage(c: Chat, m: Message, now: number) {
  if (m.from === 'me' && m.file && (m.file.state === 'sending' || m.file.state === 'waiting')) {
    const f = m.file;
    if (!reachable(c)) { f.state = 'waiting'; return; }
    f.state = 'sending';
    const total = Math.min(SIM.fileMaxMs, Math.max(SIM.fileMinMs, (f.size / 1.2e6) * 1000)) * timeScale();
    f.progress = Math.min(1, f.progress + SIM.tickMs / total);
    if (db.env.failNextSend && f.progress > 0.5) {
      db.env.failNextSend = false; f.state = 'failed'; m.delivery = 'failed';
    } else if (f.progress >= 1) {
      f.state = 'done'; m.delivery = 'delivered'; m.deliveredAt = clock.now;
    }
    return;
  }
  if (m.from !== 'me' || m.file) return;
  if (m.delivery === 'waiting' && reachable(c)) { m.delivery = 'sending'; stamp(m.id); return; }
  if (m.delivery !== 'sending') return;
  const t0 = sentAt.get(m.id) ?? (stamp(m.id), now);
  if (now - t0 < SIM.sendMs * timeScale()) return;
  sentAt.delete(m.id);
  if (!reachable(c)) m.delivery = 'waiting';
  else if (db.env.failNextSend) { db.env.failNextSend = false; m.delivery = 'failed'; }
  else { m.delivery = 'delivered'; m.deliveredAt = clock.now; }
}

function tick() {
  const now = performance.now();
  for (const c of db.chats) for (const m of c.messages) stepMessage(c, m, now);
}

const g = globalThis as unknown as { __chatTransport?: number };
if (typeof window !== 'undefined') {
  if (g.__chatTransport) clearInterval(g.__chatTransport); // HMR: one loop only
  g.__chatTransport = window.setInterval(tick, SIM.tickMs);
}
