/* Cue anchors: WHEN each call cue's first note sounds, taken from the same fx-bus events audio.ts
   reacts to (including its 30 ms look-ahead and the connect cue's markHit delay). The cloud field
   reads the note schedule from here, so it follows whichever sound palette is selected, still
   animates in Quiet mode (no latency is added then), and stays aligned after minimize/restore. */
import { onFx } from '../fx/bus';
import { audio } from '../fx/audio';
import { dur } from '../fx/motion';
import { motion } from '../fx/reduced.svelte';

export type CuePattern = ReturnType<typeof audio.pattern>;
export interface Anchor { start: number; pat: CuePattern }
type Kind = 'ring' | 'back' | 'connected' | 'ended' | 'failed' | 'pulse' | 'waiting';
const anchors: Partial<Record<Kind, Anchor>> = {};

/** audio.play() schedules 30 ms ahead; nothing sounds in Quiet mode, so the visual does not wait. */
const latency = () => (audio.armed && !audio.quiet ? 30 : 0);
const set = (k: Kind, delayMs = 0) => { anchors[k] = { start: performance.now() + delayMs + latency(), pat: audio.pattern(k) }; };

onFx((e) => {
  switch (e.type) {
    case 'ring': if (e.on) set('back'); else delete anchors.back; break;
    case 'incoming': if (e.on) set('ring'); else delete anchors.ring; break;
    case 'waiting': if (e.on) set('waiting'); else delete anchors.waiting; break;
    case 'connect': set('connected', motion.reduced ? 0 : Math.max(0, dur('markHit') - audio.hitS * 1000)); break;
    case 'end': set('ended'); break;
    case 'failed': set('failed'); break;
    case 'reconnecting': set('pulse'); break;
  }
});

const STATE_CUE: Record<string, Kind> = { incoming: 'ring', waiting: 'waiting', outgoing: 'back', active: 'connected', reconnecting: 'pulse', ended: 'ended', failed: 'failed' };

/** Start time and pattern snapshot for a field state's cue (undefined start = anchor on the state change). */
export function cueFor(state: string): { cueStart?: number; pat: CuePattern | null } {
  const kind = STATE_CUE[state];
  if (!kind) return { pat: null };
  const a = anchors[kind];
  return a ? { cueStart: a.start, pat: a.pat } : { pat: audio.pattern(kind) };
}
