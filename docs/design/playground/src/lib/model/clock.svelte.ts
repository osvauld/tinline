/* Simulation clock. Starts at a fixed moment so screenshots are stable, then runs in real time
   (the drawer can jump it forward, e.g. to expire an invite or pass midnight). */

const START = new Date(2026, 9, 12, 13, 10, 0).getTime(); // Mon 12 Oct 2026, 13:10 local

const MIN = 60_000, HOUR = 60 * MIN, DAY = 24 * HOUR;
export const T = { MIN, HOUR, DAY };

class Clock {
  offset = $state(0);              // drawer jumps
  #base = performance.now();
  now = $state(START);

  constructor() {
    if (typeof window !== 'undefined') setInterval(() => this.tick(), 1000);
  }
  tick() { this.now = START + (performance.now() - this.#base) + this.offset; }
  jump(ms: number) { this.offset += ms; this.tick(); }
  /** A moment relative to the start, for seed data: ago(5 * T.MIN). */
  ago(ms: number) { return START - ms; }
}

export const clock = new Clock();

const sameDay = (a: Date, b: Date) =>
  a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate();

export function hhmm(at: number) {
  const d = new Date(at);
  return d.getHours().toString().padStart(2, '0') + ':' + d.getMinutes().toString().padStart(2, '0');
}

/** Chat-list stamp: time today, "Yesterday", weekday within a week, else "Sep 28". */
export function listStamp(at: number, now = clock.now) {
  const d = new Date(at), n = new Date(now);
  if (sameDay(d, n)) return hhmm(at);
  const y = new Date(now - DAY);
  if (sameDay(d, y)) return 'Yesterday';
  if (now - at < 6 * DAY) return d.toLocaleDateString(undefined, { weekday: 'short' });
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** Day separator in a conversation: "Today", "Yesterday", or a date. */
export function dayLabel(at: number, now = clock.now) {
  const d = new Date(at), n = new Date(now);
  if (sameDay(d, n)) return 'Today';
  if (sameDay(d, new Date(now - DAY))) return 'Yesterday';
  return d.toLocaleDateString(undefined, { weekday: 'long', month: 'short', day: 'numeric' });
}

export function dayKey(at: number) {
  const d = new Date(at);
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}
