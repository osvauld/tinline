/* What the app-bar / sidebar mark shows. One derivation, used by both shells. */
import { connection } from '../model/connection.svelte';
import { call } from '../calls/call.svelte';
import { gcall } from '../calls/gcall.svelte';

export type MarkLook = { state: 'slack' | 'ringing' | 'taut'; sway?: { period: number; amp: number }; dim?: boolean; label: string };

/** Sway periods are locked to the cue patterns in fx/audio.ts: ringtone repeats every 3.2 s
    (two swings per ring), ringback every 4 s (two swings per cycle). */
export function statusLook(): MarkLook {
  switch (call.phase) {
    case 'incoming': return { state: 'ringing', sway: { period: 1.6, amp: 12 }, label: 'Incoming call' };
    case 'outgoing': return { state: 'ringing', sway: { period: 2.0, amp: 9 }, label: 'Calling' };
    case 'connecting': return { state: 'ringing', sway: { period: 1.2, amp: 6 }, label: 'Connecting' };
    case 'active': return { state: 'taut', label: 'In a call' };
    case 'reconnecting': return { state: 'ringing', sway: { period: 2.0, amp: 4 }, label: 'Reconnecting' };
  }
  switch (gcall.phase) {   // a group call reads like a 1:1 one: slack while setting up, taut once connected
    case 'starting': return { state: 'ringing', sway: { period: 2.0, amp: 9 }, label: 'Starting a group call' };
    case 'connecting': return { state: 'ringing', sway: { period: 1.2, amp: 6 }, label: 'Connecting' };
    case 'active': return { state: 'taut', label: 'In a group call' };
    case 'reconnecting': return { state: 'ringing', sway: { period: 2.0, amp: 4 }, label: 'Reconnecting' };
  }
  switch (connection.state) {
    case 'offline': return { state: 'slack', dim: true, label: 'No connection' };
    case 'connecting': return { state: 'ringing', sway: { period: 3.2, amp: 5 }, label: 'Connecting' };
    case 'online': return { state: 'taut', label: 'Connected to the network' };
  }
}
