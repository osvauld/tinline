/* How the call's connection is doing: the path (direct or through a relay) and the signal (round trip + loss).
   Core: `CallStats { direct, rtt_ms, lost, received, reconnecting }` from `call_stats()`, polled about once a second
   (`node.rs` `selected_path`: the selected iroh path `is_ip()` = direct). Desktop's `view.rs` `quality()` thresholds
   are kept so the three apps agree:
     4 Strong  rtt < 100 ms and loss < 1 %
     3 Good    rtt < 200 ms and loss < 3 %
     2 Fair    rtt < 350 ms and loss < 8 %
     1 Weak    anything worse
   Loss is over the last few seconds, not the whole call, or one bad minute would colour the rest of it.
   A call usually starts relayed and turns direct a second or two later, when the two devices find each other
   (iroh hole punching); the sim plays that by default. */

export type Path = 'direct' | 'relayed';
export type Strength = 1 | 2 | 3 | 4;

export const SIGNAL_WORD: Record<Strength, string> = { 4: 'Strong', 3: 'Good', 2: 'Fair', 1: 'Weak' };

export function strength(rttMs: number, lossPct: number): Strength {
  if (rttMs < 100 && lossPct < 1) return 4;
  if (rttMs < 200 && lossPct < 3) return 3;
  if (rttMs < 350 && lossPct < 8) return 2;
  return 1;
}

/** Sim presets for the drawer: what each strength looks like in numbers. */
export const PRESET: Record<Strength, { rttMs: number; lossPct: number }> = {
  4: { rttMs: 42, lossPct: 0.2 }, 3: { rttMs: 140, lossPct: 1.4 }, 2: { rttMs: 280, lossPct: 5 }, 1: { rttMs: 520, lossPct: 14 },
};

class Link {
  path = $state<Path>('relayed');
  rttMs = $state(PRESET[4].rttMs);
  lossPct = $state(PRESET[4].lossPct);
  /** Sim: the two devices never find a direct path (strict NAT on both sides), so the call stays relayed. */
  stayRelayed = $state(false);
  #t = 0;

  get strength(): Strength { return strength(this.rttMs, this.lossPct); }
  get word() { return SIGNAL_WORD[this.strength]; }

  /** The call just connected: relayed first, then direct after the hole punch (unless the sim says it never works). */
  connected() {
    clearTimeout(this.#t);
    this.path = 'relayed';
    // A relay adds a little round trip; going direct takes it away again.
    this.rttMs = PRESET[4].rttMs + 60; this.lossPct = PRESET[4].lossPct;
    if (!this.stayRelayed) this.#t = window.setTimeout(() => { this.path = 'direct'; this.rttMs = PRESET[4].rttMs; }, 2400);
  }
  ended() { clearTimeout(this.#t); }
  set(s: Strength) { this.rttMs = PRESET[s].rttMs; this.lossPct = PRESET[s].lossPct; }
  setPath(p: Path) { clearTimeout(this.#t); this.path = p; }
}

export const link = new Link();
