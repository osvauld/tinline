/* Social events the OS layer turns into notifications when Tinline is out of sight (calls have their own path).
   Model actions emit; lib/calls/system.svelte.ts listens. Kept apart so model code never imports the call/system layer. */
import type { Id } from './types';

export type Social =
  | { kind: 'request'; who: Id; name: string }          // someone used my code
  | { kind: 'accepted'; who: Id; name: string }         // they accepted my request: the line is open
  | { kind: 'invite'; by: Id; byName: string; groupName: string; inv: Id };   // someone invited me to a group

export const onSocial = new Set<(e: Social) => void>();
export const emitSocial = (e: Social) => onSocial.forEach((fn) => fn(e));
