/* What the phone / desktop OS shows OUTSIDE the app for a call (PLAN step 3, system surfaces): the lock
   screen, heads-up and ongoing-call notifications, the shade, the missed-call notification, the desktop
   toast and mini window. Simulated OS state only; every action drives the same call machine.
   `locked` is the DEVICE lock screen. It is not the app passphrase gate (db.account.gate): while the
   account key is not on this device nothing rings and none of these surfaces appear. */
import { call, onRecord } from './call.svelte';
import { clock } from '../model/clock.svelte';
import { chat, db } from '../model/store.svelte';
import { prefs } from '../model/prefs.svelte';
import { nav } from '../nav/router.svelte';

export interface Notice { id: number; kind: 'missed'; peer: string; count: number; at: number }

let uid = 0;

class System {
  locked = $state(false);       // Android: device lock screen
  background = $state(false);   // Android: another app / home is in front. Desktop: the window is minimised.
  shadeOpen = $state(false);    // Android: notification shade pulled down
  notifications = $state<Notice[]>([]);

  /** Is the app out of sight, so the OS has to present the call? */
  get away() { return prefs.form === 'android' ? this.locked || this.background : this.background; }
  /** A call that is running (or being placed): the foreground-service notification exists for these. */
  get live() { return ['outgoing', 'connecting', 'active', 'reconnecting'].includes(call.phase); }
  /** The user is looking at the in-call screen (over the lock screen too), so no notification is needed for it. */
  get onCallScreen() { return !this.background && call.view === 'full'; }
  get ongoing() { return this.live && !this.onCallScreen; }
  /** Text under "Ongoing call": the chronometer once connected, else the machine's own status. */
  get ongoingActive() { return call.phase === 'active'; }

  /* The drawer switches: putting the app away also minimises an open call screen to its strip. */
  setLocked(v: boolean) { this.locked = v; if (v) { if (call.phase !== 'idle') call.minimize(); } else this.shadeOpen = false; }
  setBackground(v: boolean) { this.background = v; if (v && call.phase !== 'idle') call.minimize(); }

  openShade() { this.shadeOpen = true; }
  closeShade() { this.shadeOpen = false; }

  /** Bring the app (or the window) back in front. */
  foreground() { this.locked = false; this.background = false; this.shadeOpen = false; }
  /** Tapping an incoming / ongoing call surface: unlock, come forward, show the in-app call screen. */
  openCall() { this.foreground(); call.restore(); }

  dismiss(id: number) { this.notifications = this.notifications.filter((n) => n.id !== id); }
  clearAll() { this.notifications = []; }
  openMissed(n: Notice) {
    this.dismiss(n.id); this.foreground();
    if (chat(n.peer)) nav.open({ name: 'conversation', chat: n.peer });
  }
  callBack(n: Notice) { this.dismiss(n.id); this.foreground(); call.place(n.peer); }

  /** A call record was written. A missed incoming call while the app is out of sight becomes a notification;
      later misses from the same person fold into it (Proposed, not decided). */
  record(id: string, direction: 'out' | 'in', outcome: string) {
    if (direction !== 'in' || (outcome !== 'missed' && outcome !== 'cancelled') || !this.away || prefs.form !== 'android') return;
    if (db.account.gate !== 'ready') return;
    const at = clock.now, had = this.notifications.find((n) => n.peer === id);
    if (had) { had.count++; had.at = at; } else this.notifications.push({ id: ++uid, kind: 'missed', peer: id, count: 1, at });
  }
}

export const system = new System();
onRecord.add((id, dir, outcome) => system.record(id, dir, outcome));

if (typeof window !== 'undefined') (window as any).__sys = system;
