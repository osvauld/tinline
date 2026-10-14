/* The two-phone lab (`?lab=pair`): two playground windows, Maya and Tara, talk to each other over a BroadcastChannel,
   so one contact addition can be followed on BOTH sides, through the first message and the first call.
   Design simulation only (not to port). What it models, from the core:
   - Adding by code (protocol.md "Add contact"): the joiner dials the code's owner. The owner's device accepts on its
     own (no request, no question) if it is online and the code is the one it hands out now; then the code is spent
     and a fresh one is minted (node.rs `accept_hello`, "Tickets in the core"). Offline: the joiner's dial times out.
   - Both sides may call from the start (caps.rs `policy_for`, ticket origin).
   - Messages arrive when the sender's transport marks them delivered; presence follows each window's network switch.
   - Calls: place/answer/decline/hangup on one side drive `call.remote(...)` / `call.receive(...)` on the other. */
import { db, person } from '../model/store.svelte';
import { pp, pairing, CARD_OWNER, sampleCard, codeHooks, CODE_DAYS, incomingRequest, accepted, type Reach } from '../model/actions/people.svelte';
import { clock, T } from '../model/clock.svelte';
import { ensureChat } from '../model/actions/profile';
import { receiveText } from '../model/actions/chat';
import { call } from '../calls/call.svelte';
import { link, type Path } from '../calls/link.svelte';

type Who = 'maya' | 'tara';
const SELF: Record<Who, { name: string; hue: number }> = { maya: { name: 'Maya Kurian', hue: 0 }, tara: { name: 'Tara Iyer', hue: 3 } };
type Msg =
  | { t: 'code'; from: Who; name: string; hue: number; code: string; expired?: boolean }
  | { t: 'hello'; from: Who; to: Who; name: string; hue: number; code: string; nonce: string }
  | { t: 'welcome'; from: Who; to: Who; nonce: string; ok: boolean; reason?: 'refused' | 'blocked' }
  | { t: 'accepted'; from: Who; to: Who; name: string; hue: number }
  | { t: 'presence'; from: Who; online: boolean }
  | { t: 'msg'; from: Who; to: Who; text: string }
  | { t: 'call'; from: Who; to: Who; video: boolean }
  | { t: 'callsig'; from: Who; to: Who; what: 'answer' | 'decline' | 'cancel' | 'hangup' }
  | { t: 'link'; from: Who; to: Who; path: Path; rttMs: number; lossPct: number }
  | { t: 'ctl'; to: Who; what: 'offline' | 'online' | 'renew' | 'announce' | 'expire' | 'block' | 'unblock' };

export const DIAL_TIMEOUT_MS = 4000;
/** Hello → "delivered" on the owner's side (verify, store the request). Real: well under a second. */
export const JOIN_MS = 600;

export function startPair(me: Who) {
  const peer: Who = me === 'maya' ? 'tara' : 'maya';
  const ch = new BroadcastChannel('tinline-pair-' + (new URLSearchParams(location.search).get('room') ?? ''));
  const post = (m: Msg) => ch.postMessage(m);
  let uses = 0, blocksPeer = false;

  // Who I am in this window. Tara is an existing user (sample data); Maya is whoever signs up in the left window.
  if (me === 'tara') { db.account.name = SELF.tara.name; db.account.hue = SELF.tara.hue; }
  db.people = db.people.filter((p) => p.id !== peer);
  db.chats = db.chats.filter((c) => !(c.kind === 'direct' && c.person === peer));
  const myName = () => db.account.name || SELF[me].name;
  const announce = () => post({ t: 'code', from: me, name: myName(), hue: db.account.hue, code: pp.myCode });
  // The code starts with its owner (OSVC2:MAYA… / OSVC2:TARA…) and a fresh seed each time, so codes from an earlier
  // load never collide with today's and any window can tell whose a code is (core: the signed name inside the ticket).
  const mint = () => { pp.myCode = 'OSVC2:' + me.toUpperCase() + sampleCard(me + '#' + uses++ + '#' + Date.now()).slice(6 + 4); pp.myCodeExp = clock.now + CODE_DAYS * T.DAY; CARD_OWNER.set(pp.myCode, { id: 'me', name: myName(), hue: db.account.hue }); announce(); };
  mint();
  codeHooks.renew = mint;
  pairing.owner = (c) => {
    const who = (['maya', 'tara'] as Who[]).find((w) => c.startsWith('OSVC2:' + w.toUpperCase()));
    if (!who) return undefined;
    return who === me ? { id: 'me', name: myName(), hue: db.account.hue } : { id: who, ...SELF[who] };
  };
  pairing.answered = (r, yes) => { if (yes && r.who === peer) post({ t: 'accepted', from: me, to: peer, name: myName(), hue: db.account.hue }); };   // "New code" in the sheet: the other window learns it, the old one stops working

  // Joiner side: "Add Tara" dials her and waits for a Welcome (or nothing, if her device is off).
  const pending = new Map<string, (r: Reach) => void>();
  pairing.reach = (f) => new Promise<Reach>((resolve) => {
    if (f.id !== peer) { setTimeout(() => resolve('ok'), 700); return; }
    const nonce = Math.random().toString(36).slice(2);
    const code = f.code ?? '';
    pending.set(nonce, resolve);
    post({ t: 'hello', from: me, to: peer, name: myName(), hue: db.account.hue, code, nonce });
    setTimeout(() => { if (pending.delete(nonce)) resolve('timeout'); }, DIAL_TIMEOUT_MS);
  });

  let linkSeen = '';
  ch.onmessage = (e: MessageEvent<Msg>) => {
    const m = e.data;
    if (m.t === 'ctl') {
      if (m.to !== me) return;
      if (m.what === 'offline' || m.what === 'online') db.env.networkOnline = m.what === 'online';
      if (m.what === 'renew') mint();          // I made a new code: the old one stops working
      if (m.what === 'announce') announce();
      if (m.what === 'expire') post({ t: 'code', from: me, name: myName(), hue: db.account.hue, code: pp.myCode, expired: true });
      if (m.what === 'block') blocksPeer = true;
      if (m.what === 'unblock') blocksPeer = false;
      return;
    }
    if (m.from === me) return;
    if (m.t === 'code') { CARD_OWNER.set(m.code, { id: m.from, name: m.name, hue: m.hue, expired: m.expired }); return; }
    if ('to' in m && m.to !== me) return;
    if (!db.env.networkOnline && m.t !== 'presence') return;   // my device is off: nothing reaches me
    switch (m.t) {
      case 'hello': {
        if (blocksPeer) { post({ t: 'welcome', from: me, to: m.from, nonce: m.nonce, ok: false, reason: 'blocked' }); return; }
        if (m.code !== pp.myCode) { post({ t: 'welcome', from: me, to: m.from, nonce: m.nonce, ok: false, reason: 'refused' }); return; }
        // A request (decided 2026-10-13): Tara's device takes it and tells Maya it was delivered; Tara answers.
        const who = { id: m.from, name: m.name, hue: m.hue };
        setTimeout(() => {
          incomingRequest(who, call.phase === 'idle');
          post({ t: 'welcome', from: me, to: m.from, nonce: m.nonce, ok: true });
        }, JOIN_MS);
        return;
      }
      case 'welcome': { const r = pending.get(m.nonce); if (r) { pending.delete(m.nonce); r(m.ok ? 'pending' : m.reason ?? 'refused'); } return; }
      case 'accepted': accepted({ id: m.from, name: m.name, hue: m.hue }, call.phase !== 'idle'); return;
      case 'presence': { const p = person(m.from); if (p) p.online = m.online; return; }
      case 'msg': { const c = ensureChat(m.from); if (c) receiveText(c.id, m.from, m.text); return; }
      case 'call': call.receive(m.from, m.video); return;
      case 'link': if (call.peer === m.from) { linkSeen = `${m.path}|${m.rttMs}|${m.lossPct}`; link.setPath(m.path); link.rttMs = m.rttMs; link.lossPct = m.lossPct; } return;
      case 'callsig': if (call.peer === m.from) call.remote(m.what); return;
    }
  };

  // Presence: my network switch is what the other side sees.
  $effect.root(() => {
    $effect(() => { post({ t: 'presence', from: me, online: db.env.networkOnline }); });
    // The call's path and signal belong to the one connection both ends share: whichever side changes it (the
    // hole punch, or the drawer) tells the other, so both say Direct / Relayed and show the same bars.
    $effect(() => {
      const k = `${link.path}|${link.rttMs}|${link.lossPct}`;
      if (call.peer !== peer || (call.phase !== 'active' && call.phase !== 'reconnecting') || k === linkSeen) return;
      linkSeen = k; post({ t: 'link', from: me, to: peer, path: link.path, rttMs: link.rttMs, lossPct: link.lossPct });
    });
    // Messages: forward each of mine to the peer once my transport marks it delivered.
    const sent = new Set<string>();
    $effect(() => {
      for (const c of db.chats) {
        if (c.kind !== 'direct' || c.person !== peer) continue;
        for (const msg of c.messages) {
          if (msg.from === 'me' && msg.delivery === 'delivered' && msg.text && !sent.has(msg.id)) { sent.add(msg.id); post({ t: 'msg', from: me, to: peer, text: msg.text }); }
        }
      }
    });
  });

  // Calls: my actions become the other side's remote events.
  const sig = (what: 'answer' | 'decline' | 'cancel' | 'hangup') => { if (call.peer === peer) post({ t: 'callsig', from: me, to: peer, what }); };
  const place = call.place.bind(call), answer = call.answer.bind(call), decline = call.decline.bind(call), hangup = call.hangup.bind(call);
  call.place = (id: string, video = false) => { const ok = place(id, video); if (ok && id === peer) post({ t: 'call', from: me, to: peer, video }); return ok; };
  call.answer = (cam = false) => { const was = call.phase; answer(cam); if (was === 'incoming') sig('answer'); };
  call.decline = () => { const was = call.phase; if (was === 'incoming') sig('decline'); decline(); };
  call.hangup = () => {
    const was = call.phase;
    if (was === 'outgoing') sig('cancel');
    else if (was === 'connecting' || was === 'active' || was === 'reconnecting') sig('hangup');
    hangup();   // 'incoming' goes through decline() above
  };
}
