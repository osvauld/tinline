/* Simulated core search (docs/search.md on branch `search`, `Node::search`). It reads the ONE store, so results are live:
   call it inside $derived / $effect and they re-run when a message arrives, is edited or deleted.

   What it follows from the core design (Tokenizing + Query):
   - NFKC, lowercase; Latin letters fold diacritics (é -> e) by dropping Mn after a Latin base; Indic and other marks stay.
   - Words are runs of letters/digits/marks. Runs of CJK, Thai, Lao, Khmer, Myanmar (no spaces) match as substrings (the core indexes bigrams).
   - Every other word matches a query term that is a PREFIX of it; a 1-char word is indexed as itself (so a 1-char term only finds 1-char words).
   - AND of all terms; messages need a query of >= 2 characters, names match from 1 (case-folded substring over the whole query).
   - Newest first, cursor `{at}/{id}`, `limit` clamped to 1..100, names with the first page only.
   - Snippet ~80 chars around the first match; highlight ranges in CHARACTER (code point) offsets, the app converts (see `utf16Ranges`).
   - Only posts are indexed: no system lines, no call records, no deleted messages. A file message indexes its file name.
   - DM hits only for conversations the chat list shows; group hits for every group this device holds (active, left and removed: their
     history stays readable). Names: contacts, kept histories, groups and "You" (also our own profile name).

   Simulation-only: the "index is getting ready" state (drawer) limits messages to the last HOT_DAYS days, like a reconcile that has not
   reached the older shards yet. The playground does not model the sealed file, watermarks or segments. */
import { db, chatTitle, displayName, isGroup, person } from '../store.svelte';
import { clock } from '../clock.svelte';
import { searchSim } from './search.svelte';
import type { Chat, Id, Message } from '../types';

export const SEARCH = {
  limit: 20,           // page size of the apps
  minChars: 2,         // messages need 2 characters; names from 1
  debounceMs: 150,     // docs/search.md: debounce 150 ms
  snippetChars: 80,
  hotDays: 8,          // sim: messages older than this are missing while the index is "getting ready"
  foundMs: 2200,       // how long a jumped-to message stays highlighted (a UX dwell, not an animation length)
} as const;

export { searchSim } from './search.svelte';

export interface HitRange { start: number; end: number }   // character (code point) offsets, end exclusive
export interface NameHit { kind: 'contact' | 'group' | 'me'; id: Id; name: string; highlights: HitRange[]; status?: 'removed' | 'left' }
export interface MessageHit {
  kind: 'dm' | 'group'; conv: Id; conv_name: string; day: string; id: Id; at: number;
  author: Id; author_name: string; snippet: string; highlights: HitRange[]; status?: 'removed' | 'left';
}
export interface SearchResults { names: NameHit[]; messages: MessageHit[]; next: string | null }

/* ---------------------------------------------------------------- tokenizing */
const LATIN = /\p{Script=Latin}/u;
const MN = /\p{Mn}/u;
const WORDCH = /[\p{L}\p{N}\p{M}]/u;
const BIGRAM = /[\p{Script=Han}\p{Script=Hiragana}\p{Script=Katakana}\p{Script=Thai}\p{Script=Lao}\p{Script=Khmer}\p{Script=Myanmar}]/u;

interface Unit { n: string; o0: number; o1: number }   // one original code point -> its normalized characters (maybe none), original span
function fold(text: string): Unit[] {
  const out: Unit[] = [];
  let prevLatin = false, o = 0;
  for (const ch of text) {
    const o0 = o; o += 1;
    const lower = ch.normalize('NFKC').toLowerCase();
    let n: string;
    if (MN.test(ch)) n = prevLatin ? '' : lower;                        // an accent after a Latin base folds away; other marks (Indic vowel signs) stay
    else {
      const d = lower.normalize('NFD');
      if (d && LATIN.test(d[0])) { n = d.replace(/\p{Mn}/gu, ''); prevLatin = true; } else { n = lower; prevLatin = false; }
    }
    out.push({ n, o0, o1: o });
  }
  return out;
}

/** A query or name, normalized the way messages are. */
export function normalize(s: string): string { return fold(s).map((u) => u.n).join(''); }

export interface Word { norm: string[]; ends: number[]; start: number; end: number; bigram: boolean }  // norm chars, original end of each, original span
export function words(text: string): Word[] {
  const units = fold(text);
  const out: Word[] = [];
  let cur: Word | null = null;
  const chars = Array.from(text);
  for (const u of units) {
    const ch = chars[u.o0];
    // UAX #29: an apostrophe between two letters stays inside the word (grandma’s); only letters and digits are indexed, so it adds no character.
    if (cur && !cur.bigram && (ch === "'" || ch === '’') && WORDCH.test(chars[u.o0 + 1] ?? '') && !BIGRAM.test(chars[u.o0 + 1] ?? '')) continue;
    const isWord = WORDCH.test(ch);
    const big = isWord && BIGRAM.test(ch);
    if (!isWord || (cur && cur.bigram !== big)) { if (cur) out.push(cur); cur = null; if (!isWord) continue; }
    if (!cur) cur = { norm: [], ends: [], start: u.o0, end: u.o1, bigram: big };
    for (const c of u.n) { cur.norm.push(c); cur.ends.push(u.o1); }
    cur.end = u.o1;
  }
  if (cur) out.push(cur);
  return out.filter((w) => w.norm.length > 0);
}

export function terms(q: string): string[] {
  return words(q).map((w) => w.norm.join('')).filter(Boolean);
}

/** Where `term` matches inside `w` (in normalized characters), or -1. A prefix for ordinary words, a substring for bigram scripts. */
function at(w: Word, term: string): number {
  const t = Array.from(term);
  if (w.bigram) {
    const s = w.norm.join('');
    const i = s.indexOf(term);
    return i < 0 ? -1 : Array.from(s.slice(0, i)).length;
  }
  if (t.length === 1) return w.norm.length === 1 && w.norm[0] === t[0] ? 0 : -1;       // a 1-char word is indexed as itself
  if (t.length > w.norm.length) return -1;
  for (let k = 0; k < t.length; k++) if (w.norm[k] !== t[k]) return -1;
  return 0;
}

/** AND of terms over the words of `text`; returns the highlight ranges (code point offsets, merged), or null if a term has no word. */
export function matchText(text: string, qs: string[]): HitRange[] | null {
  if (!qs.length) return null;
  const ws = words(text);
  const ranges: HitRange[] = [];
  for (const t of qs) {
    let found = false;
    const len = Array.from(t).length;
    for (const w of ws) {
      const i = at(w, t);
      if (i < 0) continue;
      found = true;
      const start = i === 0 ? w.start : (w.ends[i - 1] ?? w.start);
      const end = w.ends[Math.min(w.norm.length, i + len) - 1] ?? w.end;
      ranges.push({ start, end });
    }
    if (!found) return null;
  }
  ranges.sort((a, b) => a.start - b.start || a.end - b.end);
  const merged: HitRange[] = [];
  for (const r of ranges) {
    const last = merged[merged.length - 1];
    if (last && r.start <= last.end) last.end = Math.max(last.end, r.end); else merged.push({ ...r });
  }
  return merged;
}

/* ---------------------------------------------------------------- snippet */
/** ~80 characters around the first match; `…` where the text is cut. Highlight offsets are relative to the returned snippet. */
export function snippetOf(text: string, ranges: HitRange[], max: number = SEARCH.snippetChars): { snippet: string; highlights: HitRange[] } {
  const chars = Array.from(text.replace(/\s/g, ' '));            // one char per char: offsets stay valid
  if (chars.length <= max) return { snippet: chars.join(''), highlights: ranges.map((r) => ({ ...r })) };
  const first = ranges[0]?.start ?? 0, firstEnd = ranges[0]?.end ?? 0;
  let s = Math.max(0, first - Math.floor(max * 0.3));
  let e = Math.min(chars.length, s + max);
  s = Math.max(0, e - max);
  if (s > 0 && chars[s - 1] !== ' ') for (let k = s; k < Math.min(first, s + 12); k++) if (chars[k] === ' ') { s = k + 1; break; }   // start at a word, not inside one
  if (e < chars.length && chars[e] !== ' ') { const k = chars.lastIndexOf(' ', e); if (k > firstEnd && e - k < 14) e = k; }              // end at a word too
  const pre = s > 0 ? '…' : '', post = e < chars.length ? '…' : '';
  const body = chars.slice(s, e).join('');
  const hl = ranges.filter((r) => r.end > s && r.start < e).map((r) => ({ start: Math.max(r.start, s) - s + pre.length, end: Math.min(r.end, e) - s + pre.length }));
  return { snippet: pre + body + post, highlights: hl };
}

/** Code point offsets (the core's) to UTF-16 offsets (what JS strings use). */
export function utf16Ranges(text: string, ranges: HitRange[]): HitRange[] {
  const idx: number[] = [0];
  let u = 0;
  for (const ch of text) { u += ch.length; idx.push(u); }
  return ranges.map((r) => ({ start: idx[Math.min(r.start, idx.length - 1)], end: idx[Math.min(r.end, idx.length - 1)] }));
}

/** Split text into plain and highlighted parts, from code point ranges. */
export function parts(text: string, ranges: HitRange[]): { t: string; hit: boolean }[] {
  const out: { t: string; hit: boolean }[] = [];
  const rs = utf16Ranges(text, ranges);
  let p = 0;
  for (const r of rs) {
    if (r.start > p) out.push({ t: text.slice(p, r.start), hit: false });
    if (r.end > r.start) out.push({ t: text.slice(r.start, r.end), hit: true });
    p = Math.max(p, r.end);
  }
  if (p < text.length) out.push({ t: text.slice(p), hit: false });
  return out;
}

/* ---------------------------------------------------------------- what is searchable */
/** The indexed text of a message, or null when it has no row (system line, call, deleted, no words). */
export function indexedText(m: Message): string | null {
  if (m.system || m.call || m.deleted) return null;
  const t = [m.text, m.file?.name].filter(Boolean).join(' ').trim();
  return t && words(t).length ? t : null;
}

export const utcDay = (at: number) => new Date(at).toISOString().slice(0, 10);

/** Conversations whose messages may be returned: what the chat list and Groups list show (active, left and removed groups included). */
function searchable(c: Chat): boolean {
  if (isGroup(c)) return true;
  return !!person(c.person);
}

/* ---------------------------------------------------------------- query */
function nameHits(q: string): NameHit[] {
  const nq = normalize(q).replace(/\s+/g, ' ').trim();
  if (!nq) return [];
  const out: NameHit[] = [];
  const hit = (kind: NameHit['kind'], id: Id, shown: string, extra: string[] = [], status?: NameHit['status']) => {
    const highlights = mapNameRange(shown, nq);
    if (!highlights.length && !extra.some((n) => normalize(n).includes(nq))) return;
    out.push({ kind, id, name: shown, highlights, status });
  };
  if ('you'.includes(nq) || normalize(db.account.name).includes(nq)) hit('me', 'me', 'You', [db.account.name]);
  const chatted = new Set(db.chats.filter((c) => c.kind === 'direct').map((c) => (c as { person: Id }).person));
  for (const p of db.people) if (chatted.has(p.id) || (p.contact && !p.blocked)) hit('contact', p.id, displayName(p.id), [p.name]);
  for (const c of db.chats) if (isGroup(c)) hit('group', c.id, c.name, [], c.status === 'active' ? undefined : c.status);
  return out;
}

/** Highlight range of `nq` in `shown`, in code point offsets of `shown` (names can fold to a different length). */
function mapNameRange(shown: string, nq: string): HitRange[] {
  const us = fold(shown);
  const norm = us.map((u) => u.n).join('');
  const i = norm.indexOf(nq);
  if (i < 0) return [];
  let pos = 0, start = -1, end = -1;
  for (const u of us) {
    const next = pos + u.n.length;
    if (start < 0 && next > i) start = u.o0;
    if (next >= i + nq.length && start >= 0) { end = u.o1; break; }
    pos = next;
  }
  return start >= 0 && end >= 0 ? [{ start, end }] : [];
}

interface Cand { c: Chat; m: Message; text: string; ranges: HitRange[] }

export function search(query: string, limit: number = SEARCH.limit, cursor: string | null = null): SearchResults {
  limit = Math.max(1, Math.min(100, Math.floor(limit)));
  const q = query.trim();
  const names = !cursor && q ? nameHits(q) : [];
  const qs = terms(q);
  if (!qs.length || Array.from(q).length < SEARCH.minChars) return { names, messages: [], next: null };

  const cands: Cand[] = [];
  const hot = searchSim.rebuilding ? clock.now - SEARCH.hotDays * 86_400_000 : -Infinity;
  for (const c of db.chats) {
    if (!searchable(c)) continue;
    for (const m of c.messages) {
      if (m.at < hot) continue;
      const text = indexedText(m);
      if (text === null) continue;
      const ranges = matchText(text, qs);
      if (ranges) cands.push({ c, m, text, ranges });
    }
  }
  cands.sort((a, b) => b.m.at - a.m.at || (a.m.id < b.m.id ? 1 : -1));
  const key = (x: Cand) => `${x.m.at}/${x.m.id}`;
  let from = 0;
  if (cursor) {
    const k = cursor.indexOf('/'), cat = Number(cursor.slice(0, k)), cid = cursor.slice(k + 1);
    from = cands.findIndex((x) => x.m.at < cat || (x.m.at === cat && x.m.id < cid));   // the first hit after the cursor, even if the cursor's own message is gone
    if (from < 0) from = cands.length;
  }
  const page = cands.slice(from, from + limit);
  const messages: MessageHit[] = page.map((x) => {
    const { snippet, highlights } = snippetOf(x.text, x.ranges);
    return {
      kind: x.c.kind === 'group' ? 'group' : 'dm', conv: x.c.id, conv_name: chatTitle(x.c), day: utcDay(x.m.at), id: x.m.id, at: x.m.at,
      author: x.m.from, author_name: displayName(x.m.from), snippet, highlights,
      status: isGroup(x.c) && x.c.status !== 'active' ? x.c.status : undefined,
    };
  });
  const more = from + limit < cands.length;
  return { names, messages, next: more && page.length ? key(page[page.length - 1]) : null };
}

/** `pages` pages through the cursor, concatenated (what the apps do while scrolling). */
export function searchPages(query: string, pages: number): SearchResults & { more: boolean } {
  let r = search(query, SEARCH.limit, null);
  const all: SearchResults = { names: r.names, messages: [...r.messages], next: r.next };
  for (let i = 1; i < pages && all.next; i++) {
    r = search(query, SEARCH.limit, all.next);
    all.messages.push(...r.messages); all.next = r.next;
  }
  return { ...all, more: !!all.next };
}

/* ---------------------------------------------------------------- find in this chat (Proposed: not in the core doc) */
export interface FindMatch { id: Id; ranges: HitRange[] }
/** Messages of one conversation that match, oldest first, with the highlight ranges in UTF-16 offsets of the bubble text. */
export function findInChat(c: Chat | undefined, query: string): FindMatch[] {
  if (!c) return [];
  const qs = terms(query);
  if (!qs.length) return [];
  const out: FindMatch[] = [];
  for (const m of c.messages) {
    const text = indexedText(m);
    if (text === null) continue;
    const r = matchText(text, qs);
    if (r) out.push({ id: m.id, ranges: utf16Ranges(text, r) });
  }
  return out;
}

/* ---------------------------------------------------------------- jump to a hit */
let pendingJump: { chat: Id; id: Id } | null = null;
export const jump = {
  set(chat: Id, id: Id) { pendingJump = { chat, id }; },
  peek(chat: Id): Id | null { return pendingJump && pendingJump.chat === chat ? pendingJump.id : null; },
  take(chat: Id): Id | null { const id = jump.peek(chat); if (id) pendingJump = null; return id; },
};

if (typeof window !== 'undefined') (window as any).__pgSearch = { search, searchPages, normalize, terms, matchText, snippetOf, parts, SEARCH, searchSim };
