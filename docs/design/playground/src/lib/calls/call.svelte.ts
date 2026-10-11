/* Call state (minimal until PLAN step 3 builds the full machine). Phases drive both the fx bus
   (sound) and every mark that shows call status, so they cannot disagree. */
import { emitFx } from '../fx/bus';

export type CallPhase = 'idle' | 'incoming' | 'outgoing' | 'active';

class Call {
  phase = $state<CallPhase>('idle');
  peer = $state<string | null>(null);

  set(next: CallPhase, peer: string | null = this.peer) {
    const prev = this.phase;
    if (prev === next) return;
    if (prev === 'incoming') emitFx({ type: 'incoming', on: false });
    if (prev === 'outgoing') emitFx({ type: 'ring', on: false });
    this.phase = next; this.peer = next === 'idle' ? null : peer;
    if (next === 'incoming') emitFx({ type: 'incoming', on: true });
    if (next === 'outgoing') emitFx({ type: 'ring', on: true });
    if (next === 'active') emitFx({ type: 'connect' });
    if (next === 'idle' && prev !== 'idle') emitFx({ type: 'end' });
  }
}
export const call = new Call();
