/* Additional profile information: which keys get a proper control, how any other key is named, and what counts as a safe link.
   Profile fields are data. Nothing here fetches, embeds or runs anything a field contains. */
import { isReserved } from '../../lib/model/actions/profile';

export interface Known { label: string; kind: 'text' | 'link'; hint: string }
/** Keys with their own control. Anything else is "other information": shown readably, kept when anything else is edited. */
export const KNOWN: Record<string, Known> = {
  pronouns: { label: 'Pronouns', kind: 'text', hint: 'For example, she/her' },
  location: { label: 'Location', kind: 'text', hint: 'City or region' },
  website: { label: 'Website', kind: 'link', hint: 'https://' },
};

/** "work-title" / "work_title" -> "Work title". The stored key is never changed. */
export function humanize(key: string) {
  const s = key.replace(/[-_.]+/g, ' ').replace(/([a-z])([A-Z])/g, '$1 $2').trim();
  return s ? s[0].toUpperCase() + s.slice(1) : key;
}
export const labelOf = (key: string) => KNOWN[key]?.label ?? humanize(key);

/** Additional-information keys in display order: known ones first, then the rest in the order they were added. */
export function extraKeys(fields: Record<string, unknown>): string[] {
  const keys = Object.keys(fields).filter((k) => !isReserved(k));
  const known = Object.keys(KNOWN).filter((k) => keys.includes(k));
  return [...known, ...keys.filter((k) => !(k in KNOWN))];
}

/** http(s) only, no embedded credentials. Anything else is shown as plain text and never offered as a link. */
export function safeUrl(raw: unknown): string | null {
  if (typeof raw !== 'string') return null;
  try {
    const u = new URL(raw.trim());
    return (u.protocol === 'https:' || u.protocol === 'http:') && !u.username && !u.password ? u.href : null;
  } catch { return null; }
}
/** What the user typed -> a stored address. A bare "arjun.example" becomes https://arjun.example. */
export function normaliseLink(raw: string): string | null {
  const t = raw.trim();
  if (!t) return null;
  return safeUrl(/^[a-z][a-z0-9+.-]*:/i.test(t) ? t : 'https://' + t);
}
export const shortUrl = (u: string) => u.replace(/^https?:\/\//, '').replace(/\/$/, '');
export const isScalar = (v: unknown): v is string | number | boolean => ['string', 'number', 'boolean'].includes(typeof v);

import { clock, hhmm } from '../../lib/model/clock.svelte';
/** "18:00", "tomorrow 08:00", "Friday 08:00": a deadline as people say it. */
export function untilLabel(at: number, now = clock.now) {
  const a = new Date(at), n = new Date(now), t = new Date(now + 86_400_000);
  const same = (x: Date, y: Date) => x.getFullYear() === y.getFullYear() && x.getMonth() === y.getMonth() && x.getDate() === y.getDate();
  if (same(a, n)) return hhmm(at);
  if (same(a, t)) return `tomorrow ${hhmm(at)}`;
  return `${a.toLocaleDateString(undefined, { weekday: 'long' })} ${hhmm(at)}`;
}
