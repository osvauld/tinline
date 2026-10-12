/* Group actions. Rules from the real core are enforced HERE, not only hidden in the UI:
   - posts are text + reply only (chat.ts: canAttach/canEdit/canDelete are direct-only);
   - only the OWNER makes or unmakes admins; owner and admins invite, appoint hosts and flag;
   - the owner cannot be flagged; any 3 counted flags remove a member (the same admin may flag 3 times);
   - hosting is opt-in: an appointment is pending until that device accepts;
   - removal is final; the removed person keeps a read-only history;
   - chat hosts and call hosts are separate permits (U25): nothing here touches callHost. */
import { db, chat, person, pushMessage, newId, displayName, firstName, isGroup } from '../store.svelte';
import type { Group, GroupMember, GroupRole, Id } from '../types';
import { clock, T } from '../clock.svelte';
import { nav } from '../../nav/router.svelte';
import { gx, type RequestOut } from './groups.svelte';
import { receiveText } from './chat';

export const INVITE_MS = 7 * T.DAY;        // PROVISIONAL (core: invites valid 7 days)
export const FLAGS_TO_REMOVE = 3;          // U9/U15
export const REQUEST_MS = 7 * T.DAY;       // chat requests expire after 7 days (design board)
export const ASK_AGAIN_MS = 7 * T.DAY;     // one request per person at a time; after a decline, wait this long (Proposed)

/* ---------- lookups and rules ---------- */
export const group = (id: Id | null | undefined): Group | undefined => { const c = chat(id); return isGroup(c) ? c : undefined; };
export const member = (g: Group, id: Id): GroupMember | undefined => g.members.find((m) => m.id === id);
export const myRole = (g: Group): GroupRole | null => member(g, 'me')?.role ?? null;
export const isAdminRole = (r: GroupRole | null) => r === 'owner' || r === 'admin';
export const iAmAdmin = (g: Group) => g.status === 'active' && isAdminRole(myRole(g));
export const iAmOwner = (g: Group) => g.status === 'active' && myRole(g) === 'owner';
export const flagCount = (g: Group, id: Id) => g.flags[id]?.length ?? 0;
export const hosts = (g: Group) => g.members.filter((m) => m.host);
export const canFlag = (g: Group, target: Id) => {
  const t = member(g, target);
  return iAmAdmin(g) && !!t && t.role !== 'owner' && target !== 'me';
};
export const canSetAdmin = (g: Group, target: Id) => iAmOwner(g) && !!member(g, target) && target !== 'me' && member(g, target)!.role !== 'owner';
export const canHostAppoint = (g: Group, target: Id) => iAmAdmin(g) && !!member(g, target);
export const linkLive = (l: { expiresAt: number }) => l.expiresAt > clock.now;

export const roleLabel = (r: GroupRole) => (r === 'owner' ? 'Owner' : r === 'admin' ? 'Admin' : 'Member');

/** "Oct 9, 21:15" */
export function whenLabel(at: number) {
  const d = new Date(at);
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' }) + ', ' +
    d.getHours().toString().padStart(2, '0') + ':' + d.getMinutes().toString().padStart(2, '0');
}
export const dayOnly = (at: number) => new Date(at).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });

const sys = (g: Group, text: string, from: Id = 'me') => pushMessage(g, { from, text, system: true });

/* ---------- create ---------- */
export function createGroup(name: string): string | null {
  name = name.trim().replace(/\s+/g, ' ');
  if (!name) return null;
  const g: Group = {
    kind: 'group', id: newId('g'), name, status: 'active', unread: 0, muted: false, draft: '',
    // PROVISIONAL (core): the creator's device is the group's first host.
    members: [{ id: 'me', role: 'owner', host: true, callHost: false, since: clock.now }],
    invited: [], flags: {}, flagAt: {}, links: [], messages: [],
  };
  db.chats.push(g);
  const created = db.chats[db.chats.length - 1] as Group;
  sys(created, `You created “${name}”`);
  return created.id;
}

/* ---------- roles and hosts ---------- */
export function setAdmin(g: Group, id: Id, admin: boolean): boolean {
  if (!canSetAdmin(g, id)) return false;
  const m = member(g, id)!;
  if ((m.role === 'admin') === admin) return true;
  m.role = admin ? 'admin' : 'member';
  sys(g, admin ? `${displayName(id)} was made an admin` : `${displayName(id)} is no longer an admin`);
  return true;
}

/** Appointing asks the host's device; it hosts only after it accepts (opt-in). */
export function appointHost(g: Group, id: Id): boolean {
  if (!canHostAppoint(g, id)) return false;
  const m = member(g, id)!;
  if (m.host || m.hostPending) return true;
  m.hostPending = true;
  return true;
}
export function acceptHosting(g: Group, id: Id) {
  const m = member(g, id);
  if (!m || !m.hostPending) return;
  m.hostPending = false; m.host = true;
  sys(g, `${displayName(id)}’s device is now a host`);
}
export function declineHosting(g: Group, id: Id) { const m = member(g, id); if (m) m.hostPending = false; }
export function revokeHost(g: Group, id: Id): boolean {
  if (!canHostAppoint(g, id)) return false;
  const m = member(g, id)!;
  if (m.hostPending) { m.hostPending = false; return true; }
  if (!m.host) return true;
  m.host = false;
  sys(g, `${displayName(id)} is no longer a host`);
  return true;
}

/* ---------- flags and removal (U9/U15) ---------- */
/** One admin flag. Returns false when the rules refuse it (not an admin, owner, self, not a member). */
export function flagMember(g: Group, target: Id, by: Id = 'me'): { ok: boolean; removed: boolean } {
  const t = member(g, target), f = member(g, by);
  if (!t || !f || !isAdminRole(f.role) || t.role === 'owner' || target === by || g.status !== 'active') return { ok: false, removed: false };
  g.flags[target] = [...(g.flags[target] ?? []), by];
  g.flagAt = { ...(g.flagAt ?? {}), [target]: [...(g.flagAt?.[target] ?? []), clock.now] };
  const n = g.flags[target].length;
  if (target === 'me' && n < FLAGS_TO_REMOVE) sys(g, `${displayName(by)} flagged you for removal · ${n} of ${FLAGS_TO_REMOVE}`, by);
  if (n < FLAGS_TO_REMOVE) return { ok: true, removed: false };
  removeMember(g, target);
  return { ok: true, removed: true };
}

function removeMember(g: Group, id: Id) {
  if (id === 'me') {
    g.status = 'removed'; g.removedAt = clock.now; g.unread = 0;
    sys(g, 'You were removed from the group');
    return;
  }
  sys(g, `${displayName(id)} was removed after ${FLAGS_TO_REMOVE} admin flags`);
  const i = g.members.findIndex((m) => m.id === id);
  if (i >= 0) g.members.splice(i, 1);
}

/** Who flagged me and when, for the removal view. Falls back to a name-only list when no times are known. */
export function removalDetails(g: Group) {
  const by = g.flags['me'] ?? [], at = g.flagAt?.['me'] ?? [];
  const sysAt = [...g.messages].reverse().find((m) => m.system && /removed/i.test(m.text))?.at;
  return {
    flags: by.map((id, i) => ({ id, at: at[i] as number | undefined })),
    removedAt: g.removedAt ?? sysAt,
  };
}
/** Admins can see who flagged people who were removed. */
export const removedMembers = (g: Group) =>
  Object.keys(g.flags).filter((id) => id !== 'me' && g.flags[id].length >= FLAGS_TO_REMOVE && !member(g, id));

/* ---------- leave ---------- */
/** Members and admins may leave. The owner cannot leave yet (Proposed: see GroupsSim). */
export function canLeave(g: Group) { return g.status === 'active' && myRole(g) !== 'owner'; }
export function leaveGroup(g: Group): boolean {
  if (!canLeave(g)) return false;
  sys(g, 'You left the group');
  const i = g.members.findIndex((m) => m.id === 'me');
  if (i >= 0) g.members.splice(i, 1);
  g.status = 'left'; g.unread = 0;
  return true;
}

/* ---------- invites ---------- */
export function inviteContacts(g: Group, ids: Id[]) {
  if (!iAmAdmin(g)) return;
  for (const id of ids) {
    if (member(g, id) || g.invited.some((i) => i.id === id) || person(id)?.blocked) continue;
    g.invited.push({ id, expiresAt: clock.now + INVITE_MS });
  }
}
export function cancelInvite(g: Group, id: Id) {
  const i = g.invited.findIndex((x) => x.id === id);
  if (i >= 0) g.invited.splice(i, 1);
}
/** The invitee answers (simulation). */
export function inviteeJoins(g: Group, id: Id) {
  const inv = g.invited.find((i) => i.id === id);
  if (!inv) return;
  cancelInvite(g, id);
  g.members.push({ id, role: 'member', host: false, callHost: false, since: clock.now, invitedBy: 'me' });
  sys(g, `${displayName(id)} joined, invited by You`, id);
}
export function inviteeDeclines(g: Group, id: Id) { cancelInvite(g, id); }

/** A short link names ONE host: the first online host, else the first host. null when the group has none. */
export function linkHost(g: Group): Id | null {
  const hs = hosts(g);
  return hs[0]?.id ?? null;
}
const CODE = 'abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789';
export function createLink(g: Group): boolean {
  const host = linkHost(g);
  if (!iAmAdmin(g) || !host) return false;
  let code = '';
  for (let i = 0; i < 12; i++) code += CODE[Math.floor(Math.random() * CODE.length)];
  g.links = [...(g.links ?? []), { code, host, expiresAt: clock.now + INVITE_MS }];
  return true;
}
export const linkText = (l: { code: string }) => `tinline://g/${l.code}`;
export function cancelLink(g: Group, code: string) { g.links = (g.links ?? []).filter((l) => l.code !== code); }

/* ---------- invitations to me ---------- */
export function receiveInvitation(by: Id, groupName: string) {
  const id = newId('inv');
  gx.invitations.push({ id, by, groupName, at: clock.now, expiresAt: clock.now + INVITE_MS, hostName: firstName(by) });
  return id;
}
export function acceptInvitation(id: Id): string | null {
  const i = gx.invitations.findIndex((x) => x.id === id);
  if (i < 0) return null;
  const inv = gx.invitations[i];
  gx.invitations.splice(i, 1);
  const g: Group = {
    kind: 'group', id: newId('g'), name: inv.groupName, status: 'active', unread: 0, muted: false, draft: '',
    members: [
      { id: inv.by, role: 'owner', host: true, callHost: false, since: clock.now },
      { id: 'me', role: 'member', host: false, callHost: false, since: clock.now, invitedBy: inv.by },
    ],
    invited: [], flags: {}, flagAt: {}, links: [], messages: [],
  };
  db.chats.push(g);
  const made = db.chats[db.chats.length - 1] as Group;
  sys(made, `You joined, invited by ${firstName(inv.by)}`);
  return made.id;
}
export function declineInvitation(id: Id) {
  const i = gx.invitations.findIndex((x) => x.id === id);
  if (i >= 0) gx.invitations.splice(i, 1);
}

/* ---------- chat requests (U3) ---------- */
export const requestFor = (to: Id, groupId?: Id) => gx.requestsOut.find((r) => r.to === to && (!groupId || r.groupId === groupId));
export function canAskPrivately(g: Group, to: Id) {
  const p = person(to);
  return !!p && !p.contact && !p.blocked && to !== 'me' && !!member(g, to);
}
export function sendRequest(g: Group, to: Id, note: string): RequestOut | null {
  const p = person(to);
  if (!p || !canAskPrivately(g, to)) return null;
  const prev = requestFor(to);
  if (prev && prev.state === 'pending' && !requestExpired(prev)) return prev;
  if (prev && askAgainAt(prev) > clock.now) return null;
  if (prev) gx.requestsOut.splice(gx.requestsOut.indexOf(prev), 1);
  const r: RequestOut = { id: newId('rq'), to, groupId: g.id, note: note.trim(), at: clock.now, state: 'pending' };
  gx.requestsOut.push(r);
  return gx.requestsOut[gx.requestsOut.length - 1];
}
export function cancelRequest(id: Id) {
  const i = gx.requestsOut.findIndex((r) => r.id === id);
  if (i >= 0) gx.requestsOut.splice(i, 1);
}
function ensureDirect(pid: Id) {
  let c = db.chats.find((x) => x.kind === 'direct' && x.person === pid);
  if (!c) {
    db.chats.push({ kind: 'direct', id: pid, person: pid, messages: [], unread: 0, muted: false, draft: '' });
    c = db.chats[db.chats.length - 1];
  }
  return c;
}
function becomeContact(pid: Id) {
  const p = person(pid);
  if (p) { p.contact = true; p.callsAllowed = false; } // message-only until one of you allows calls
}
/** The other person answers my request (simulation). */
export function answerRequest(id: Id, state: 'accepted' | 'declined' | 'blocked') {
  const r = gx.requestsOut.find((x) => x.id === id);
  if (!r || r.state !== 'pending') return;
  r.state = state; r.answeredAt = clock.now;
  if (state === 'accepted') {
    becomeContact(r.to);
    const c = ensureDirect(r.to);
    const g = group(r.groupId);
    pushMessage(c, { from: r.to, text: `${firstName(r.to)} accepted your request from ${g?.name ?? 'the group'}`, system: true });
    if (r.note) pushMessage(c, { from: 'me', text: r.note, delivery: 'delivered' });
  }
}
/** A member asks me (simulation or future transport). A request from someone I block is answered automatically and never listed. */
export function receiveRequest(from: Id, groupId: Id, note: string): string | null {
  const p = person(from);
  if (!p || p.blocked) return null;
  if (gx.requestsIn.some((r) => r.from === from)) return null;
  const id = newId('rqi');
  gx.requestsIn.push({ id, from, groupId, note, at: clock.now });
  return id;
}
export function acceptRequestIn(id: Id): string | null {
  const i = gx.requestsIn.findIndex((r) => r.id === id);
  if (i < 0) return null;
  const r = gx.requestsIn[i];
  gx.requestsIn.splice(i, 1);
  becomeContact(r.from);
  const c = ensureDirect(r.from);
  pushMessage(c, { from: r.from, text: `You accepted ${firstName(r.from)}’s request from ${group(r.groupId)?.name ?? 'a group'}`, system: true });
  if (r.note) pushMessage(c, { from: r.from, text: r.note });
  return c.id;
}
export function declineRequestIn(id: Id) {
  const i = gx.requestsIn.findIndex((r) => r.id === id);
  if (i >= 0) gx.requestsIn.splice(i, 1);
}
export function blockRequester(id: Id) {
  const r = gx.requestsIn.find((x) => x.id === id);
  if (!r) return;
  const p = person(r.from);
  if (p) p.blocked = true; // a personal block: group membership is unchanged
  declineRequestIn(id);
}
/** Expired/pending-state label helper for rows. */
export const requestExpired = (r: RequestOut) => r.state === 'pending' && clock.now - r.at > REQUEST_MS;
/** When I may ask this person again after they declined (one request per person at a time). */
export const askAgainAt = (r: RequestOut) => (r.state === 'declined' ? (r.answeredAt ?? r.at) + ASK_AGAIN_MS : 0);

/* ---------- simulation helpers ---------- */
export function setMyRole(g: Group, role: GroupRole) {
  const me = member(g, 'me');
  if (!me || g.status !== 'active') return;
  if (me.role === role) return;
  if (role === 'owner') {
    const o = g.members.find((m) => m.role === 'owner');
    if (o) o.role = 'admin';
    me.role = 'owner';
  } else {
    if (me.role === 'owner') {
      const next = g.members.find((m) => m.id !== 'me' && m.role === 'admin') ?? g.members.find((m) => m.id !== 'me');
      if (next) next.role = 'owner';
    }
    me.role = role;
  }
}
/** Admins flag me, three times; the last flag removes me. */
export function simulateRemoval(g: Group, by: Id[]) {
  if (g.status !== 'active') return;
  const admins = g.members.filter((m) => m.id !== 'me' && isAdminRole(m.role)).map((m) => m.id);
  const pool = by.length ? by : admins;
  const flaggers: Id[] = [];
  for (let k = 0; k < FLAGS_TO_REMOVE; k++) flaggers.push(pool[k % pool.length] ?? admins[0]);
  g.flags['me'] = flaggers;
  const t = clock.now;
  g.flagAt = { ...(g.flagAt ?? {}), me: flaggers.map((_, k) => t - (FLAGS_TO_REMOVE - 1 - k) * 90 * T.MIN) };
  g.status = 'removed'; g.removedAt = t; g.unread = 0;
  sys(g, 'You were removed from the group');
}
export function simulateFlag(g: Group, by: Id, target: Id) { return flagMember(g, target, by); }
export function simulatePost(g: Group, from: Id, text: string) { return receiveText(g.id, from, text); }
export function openGroup(id: Id) {
  nav.push({ name: 'conversation', chat: id });
}
