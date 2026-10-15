/* What the phone / desktop OS shows OUTSIDE the app for a call (PLAN step 3, system surfaces): the lock
   screen, heads-up and ongoing-call notifications, the shade, the missed-call notification, the desktop
   toast and mini window. Simulated OS state only; every action drives the same call machine.
   `locked` is the DEVICE lock screen. It is not the app passphrase gate (db.account.gate): while the
   account key is not on this device nothing rings and none of these surfaces appear. */
import { call, onRecord } from './call.svelte';
import { gcall } from './gcall.svelte';
import { clock } from '../model/clock.svelte';
import { chat, db, ownerOf } from '../model/store.svelte';
import { switchTo } from '../model/actions/names.svelte';
import { prefs } from '../model/prefs.svelte';
import { nav } from '../nav/router.svelte';
import { onSocial, type Social } from '../model/social';
import { emitFx } from '../fx/bus';
import { pp } from '../model/actions/people.svelte';

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
  /** Someone is calling during my call and the call screen (which carries the waiting card) is not what is in front. */
  get waitingOutside() { return !!call.waitingNow && !this.onCallScreen && this.away; }
  /* Group calls (step 6): the same foreground-service notification while a group call runs and its screen is not in front.
     Its tap opens the group call screen. A "started a call" notice for a call I am not in (never rings). */
  get gOngoing() { return gcall.holdsLine && !(!this.background && gcall.view === 'full'); }
  get gOngoingActive() { return gcall.phase === 'active'; }
  get gStarted() {
    const a = gcall.announce;
    return a && !a.dismissed && !gcall.holdsLine && !!gcall.liveFor(a.gid) && !this.groupMuted(a.gid) ? a : null;
  }
  groupMuted(gid: string) { const c = chat(gid); return !!c && c.kind === 'group' && c.muted; }
  /** Text under "Ongoing call"
: the chronometer once connected, else the machine's own status. */
  get ongoingActive() { return call.phase === 'active'; }

  /* The drawer switches: putting the app away also minimises an open call screen to its strip. */
  setLocked(v: boolean) { this.locked = v; if (v) { if (call.phase !== 'idle') call.minimize(); gcall.minimize(); } else this.shadeOpen = false; }
  setBackground(v: boolean) { this.background = v; if (v) { if (call.phase !== 'idle') call.minimize(); gcall.minimize(); } }

  openShade() { this.shadeOpen = true; }
  closeShade() { this.shadeOpen = false; }

  /** Bring the app (or the window) back in front. */
  foreground() { this.locked = false; this.background = false; this.shadeOpen = false; }
  /** Tapping an incoming / ongoing call surface: unlock, come forward, show the in-app call screen. */
  openCall() { this.foreground(); call.restore(); gcall.restore(); }
  /** Tapping the "started a call" notice: open that group's conversation. */
  openGroup(gid: string) { this.foreground(); if (chat(gid)) nav.open({ name: 'conversation', chat: gid }); }
  joinFromNotice(gid: string) { this.foreground(); gcall.join(gid); }

  /** Contact requests, accepted requests and group invitations that arrived while Tinline was out of sight. */
  social = $state<(Social & { id: number; at: number })[]>([]);
  /** Out of sight: an OS notification. In front: no notification (requests and acceptances open their sheet; an
      invitation knocks once and the Groups tab shows its dot). Groups follow Notifications › Groups. */
  onSocial(e: Social) {
    if (db.account.gate !== 'ready') return;
    if (e.kind === 'invite' && !pp.notif.groups) return;
    if (this.away) this.social.push({ ...e, id: ++uid, at: clock.now });
    else if (e.kind === 'invite') emitFx({ type: 'request' });
  }
  openSocial(n: Social & { id: number }) {
    this.social = this.social.filter((x) => x.id !== n.id);
    this.foreground();
    if (n.kind === 'invite') nav.setTab('groups');
    else if (n.kind === 'accepted' && chat(n.who)) nav.open({ name: 'conversation', chat: n.who });   // the line is open: our chat
    else nav.open({ name: 'contact', person: n.who });
  }
  dismissSocial(id: number) { this.social = this.social.filter((x) => x.id !== id); }
  dismiss(id: number) { this.notifications = this.notifications.filter((n) => n.id !== id); }
  clearAll() { this.notifications = []; this.social = []; }
  /** A notice for another of my names opens that name first. */
  #openOwner(peer: string) { const o = ownerOf(peer); if (o) switchTo(db.others.indexOf(o)); }
  openMissed(n: Notice) {
    this.dismiss(n.id); this.foreground(); this.#openOwner(n.peer);
    if (chat(n.peer)) nav.open({ name: 'conversation', chat: n.peer });
  }
  callBack(n: Notice) { this.dismiss(n.id); this.foreground(); this.#openOwner(n.peer); call.place(n.peer); }

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
onSocial.add((e) => system.onSocial(e));

if (typeof window !== 'undefined') (window as any).__sys = system;
/** Test hook (captures): make people events arrive. Not used by the app. */
import { receiveInvitation } from '../model/actions/groups';
import { incomingRequest } from '../model/actions/people.svelte';
if (typeof window !== 'undefined') (window as any).__soc = { invite: (by: string, g: string) => receiveInvitation(by, g), request: () => incomingRequest() };
