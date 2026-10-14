/* The group-call lab (`?lab=group`): three playground windows, all members of "Weekend hike", linked over a
   BroadcastChannel. Maya (phone), Arjun (the group's call host, a laptop: groups are hosted on a computer) and Sam
   (phone). Each window is the whole app as that person (`?gme=maya|arjun|sam`); this file maps between the
   windows' ids (global: maya / arjun / sam) and each window's local ones ('me' for itself), and turns one window's
   group-call actions into the others' remote events (gcall.remoteStart / remoteJoin / remoteLeave / remoteMute).
   Nobody talks on their own in the lab (chatter off): the lab's "talks" switches make a person speak in every
   window, so the speaking highlight and the K-loudest rule can be followed everywhere at once.
   Design simulation only, not to port. */
import { db, person } from '../model/store.svelte';
import { gcall } from '../calls/gcall.svelte';
import { prefs } from '../model/prefs.svelte';
import { glink } from '../calls/glink.svelte';
import type { Strength } from '../calls/link.svelte';

export type GWho = 'maya' | 'arjun' | 'sam';
export const GID = 'hike';
const PEOPLE: Record<GWho, { name: string; hue: number }> = {
  maya: { name: 'Maya Kurian', hue: 0 }, arjun: { name: 'Arjun Oommen', hue: 5 }, sam: { name: 'Sam Rao', hue: 3 },
};

type Msg =
  | { t: 'start'; from: GWho }
  | { t: 'join'; from: GWho }
  | { t: 'leave'; from: GWho }
  | { t: 'mute'; from: GWho; on: boolean }
  | { t: 'hello'; from: GWho }                                   // a window (re)loaded: tell it what's running
  | { t: 'ctl'; what: 'talk'; who: GWho; on: boolean }
  | { t: 'ctl'; what: 'line'; who: GWho; level: Strength; relayed: boolean }
  | { t: 'ctl'; what: 'host'; kind: 'lowBattery' | 'limited' | 'recover' | 'lost' };

export function startGroupLab(me: GWho) {
  const room = new URLSearchParams(location.search).get('room') ?? '';
  const ch = new BroadcastChannel('tinline-group-' + room);
  const post = (m: Msg) => ch.postMessage(m);
  const local = (g: GWho) => (g === me ? 'me' : g);

  // Become `me`: the seed's account is Maya; any other window swaps Maya in as a person and itself out.
  if (me !== 'maya') {
    const self = person(me);
    const maya = { ...(self ?? db.people[0]), id: 'maya', name: PEOPLE.maya.name, hue: PEOPLE.maya.hue, profile: db.account.profile, contact: true, blocked: false, online: true, callsAllowed: true };
    db.account.name = self?.name ?? PEOPLE[me].name; db.account.hue = self?.hue ?? PEOPLE[me].hue;
    if (self?.profile) db.account.profile = self.profile;
    db.people = [...db.people.filter((p) => p.id !== me), maya];
    db.chats = db.chats.filter((c) => !(c.kind === 'direct' && c.person === me));
    for (const c of db.chats) {
      if (c.kind !== 'group') continue;
      const swap = (id: string) => (id === 'me' ? 'maya' : id === me ? 'me' : id);
      for (const m of c.members) m.id = swap(m.id);
      for (const m of c.messages) m.from = swap(m.from);
    }
  }
  gcall.conds['maya'] ??= { ...gcall.cond('sam') };
  if (me === 'arjun') {
    prefs.form = 'desktop';
    gcall.conds['me'] = { ...gcall.cond('me'), class: 'desktop', device: 'this device', power: 'mains', pct: 100 };
  }
  gcall.chatter = false;   // speaking comes from the lab's switches, the same in every window

  // My part, as the others see it.
  let wasIn = false, wasMuted = false;
  $effect.root(() => {
    $effect(() => {
      const inNow = gcall.groupId === GID && (gcall.phase === 'active' || gcall.phase === 'reconnecting');
      if (inNow && !wasIn) { if (gcall.startedByMe && gcall.current?.by === 'me') post({ t: 'start', from: me }); post({ t: 'join', from: me }); }
      if (!inNow && wasIn) post({ t: 'leave', from: me });
      wasIn = inNow;
    });
    $effect(() => {
      const m = gcall.muted;
      if (gcall.groupId === GID && m !== wasMuted) post({ t: 'mute', from: me, on: m });
      wasMuted = m;
    });
  });

  ch.onmessage = (e: MessageEvent<Msg>) => {
    const m = e.data;
    if (m.t === 'ctl') {
      if (m.what === 'talk') gcall.setSpeaking(local(m.who), m.on);
      else if (m.what === 'line') { glink.level[local(m.who)] = m.level; if (m.who === me) glink.path = m.relayed ? 'relayed' : 'direct'; }
      else if (gcall.current?.hostId !== 'me') gcall.hostEvent(GID, m.kind);   // the host's own window is the host: it stays put
      return;
    }
    if (m.from === me) return;
    const id = local(m.from);
    switch (m.t) {
      case 'start': if (!gcall.live[GID]) gcall.remoteStart(GID, id); return;
      case 'join': if (!gcall.live[GID]) gcall.remoteStart(GID, id); else gcall.remoteJoin(GID, id); return;
      case 'leave': gcall.remoteLeave(GID, id); return;
      case 'mute': gcall.remoteMute(id, m.on); return;
      case 'hello': if (wasIn) { post({ t: 'join', from: me }); if (gcall.muted) post({ t: 'mute', from: me, on: true }); } return;
    }
  };
  post({ t: 'hello', from: me });
}
