/* The fx bus: a sound cue and its visual are triggered by the SAME event, so they can't drift.
   Producers (the call machine, the composer) emit; consumers (audio.ts, Mark, overlays) listen. */

export type FxEvent =
  | { type: 'ring'; on: boolean }           // my outgoing call is ringing (ringback)
  | { type: 'incoming'; on: boolean }       // someone is calling me (ringtone)
  | { type: 'waiting'; on: boolean }        // a second call rings over my active call (call-waiting cue)
  | { type: 'connect' }                     // call connected: string pulls taut, `connected` cue
  | { type: 'taut-hit' }                    // emitted by the mark when the string reaches taut
  | { type: 'end' }                         // call ended by me: string releases, `ended` cue
  | { type: 'failed' }
  | { type: 'reconnecting' }
  | { type: 'join'; soft?: boolean }        // group roster change
  | { type: 'sent' };                       // message sent (visual only unless a cue is decided)

type Fn = (e: FxEvent) => void;
const subs = new Set<Fn>();

export function onFx(fn: Fn) { subs.add(fn); return () => subs.delete(fn); }
export function emitFx(e: FxEvent) {
  for (const fn of subs) { try { fn(e); } catch (err) { console.error(err); } }
  fxLog.push({ t: performance.now(), e });
  if (fxLog.length > 200) fxLog.shift();
}
/** Recent events, for tests and the drawer's event log. */
export const fxLog: { t: number; e: FxEvent }[] = [];
