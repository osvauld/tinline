/* Group call: how good each person's line to the call host is. Everyone in a group call connects only to the host,
   so "someone's signal" means their line to the host.
   - Mine: measured here, like a 1:1 call (core CallStats-style rtt + recent loss on my connection to the host),
     with the path (direct / relayed) shown in the card only.
   - Everyone else's: only the host can measure them. PROPOSED core change (SIGNUP-HANDOFF § 6h): the host sends a
     small "lines" note every ~2 s, one strength (1–4) per roster slot, from its own connection to each client. No
     path and no numbers for other people: a strength is enough to explain "Sam keeps breaking up".
   - The host's own tile shows no bars: it is the hub, there is no line to measure.
   Default Strong for everyone; the lab (`?lab=group`) and the drawer change it. */
import type { Strength } from './link.svelte';

export const glink = $state<{ level: Record<string, Strength>; path: 'direct' | 'relayed' }>({ level: {}, path: 'direct' });
export const lineOf = (id: string): Strength => glink.level[id] ?? 4;
