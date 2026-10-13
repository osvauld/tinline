<script lang="ts">
  /* Group calls simulation (step 6): someone starts a call, people join and leave, who is speaking, what the call host's
     device does (battery, thermal, mobile data, crash, rotation), whether a host refuses, and a 1:1 call arriving
     during a group call. Drawer only. */
  import Switch from '../ui/Switch.svelte';
  import { db, displayName, firstName, isGroup, person } from '../model/store.svelte';
  import { gcall, GCALL_MS } from '../calls/gcall.svelte';
  import { call } from '../calls/call.svelte';
  import { audio } from '../fx/audio';
  import { group } from '../model/actions/groups';
  import type { Group } from '../model/types';

  let gid = $state('hike');
  const groups = $derived(db.chats.filter(isGroup).filter((g) => g.status === 'active'));
  const g = $derived<Group | undefined>(group(gid) ?? groups[0]);
  const id = $derived(g?.id ?? '');
  const others = $derived(g ? g.members.filter((m) => m.id !== 'me') : []);
  const lc = $derived(g ? gcall.liveFor(g.id) : undefined);
  const hosts = $derived(g ? g.members.filter((m) => m.callHost || m.id === 'arjun') : []);
  let who = $state('sam');
  const pick = $derived(others.find((m) => m.id === who)?.id ?? others[0]?.id ?? '');
  let hostPick = $state('arjun');
  const hostId = $derived(hosts.find((m) => m.id === hostPick)?.id ?? hosts[0]?.id ?? 'arjun');
  const hc = $derived(gcall.cond(hostId));
  let ringer = $state('sana');
  const callers = $derived(db.people.filter((p) => p.id !== 'me' && !p.blocked && p.callsAllowed));
  const notInCall = $derived(others.filter((m) => !lc?.parts.includes(m.id)));
  const inCallOthers = $derived(others.filter((m) => lc?.parts.includes(m.id)));
  const fill = (n: number) => {
    if (!g || !lc) return;
    for (const m of others) { if (lc.parts.length >= n) break; gcall.remoteJoin(g.id, m.id); }
  };
  const addContacts = () => {
    if (!g) return;
    for (const p of db.people) if (p.id !== 'me' && !p.blocked && !g.members.some((m) => m.id === p.id)) g.members.push({ id: p.id, role: 'member', host: false, callHost: false, since: Date.now() });
  };
  const preview = (k: 'started' | 'joined' | 'left') => { audio.arm(); audio.setQuiet(false); gcall.say(k === 'started' ? 'Cue: call started' : k === 'joined' ? 'Cue: joined' : 'Cue: left'); audio.play(k === 'started' ? 'gstart' : k === 'joined' ? 'gjoin' : 'gleft'); };
  const ringOne = () => { call.receive(ringer); };
</script>

<div class="rows">
  <label class="row"><span>Group</span>
    <select bind:value={gid} aria-label="Group for the call">{#each groups as x (x.id)}<option value={x.id}>{x.name}</option>{/each}</select></label>
  <p class="state">Now: <b>{gcall.phase}</b>{#if gcall.groupId} in {gcall.group?.name}{/if}
    {#if lc} · host <b>{gcall.hostLabel(lc.hostId)}</b> ({gcall.hostClass(lc.hostId)}) · {lc.parts.length} of {gcall.capFor(lc)} · K {gcall.kFor(lc)} · gen {lc.gen}{lc.limited ? ' · limited' : ''}{:else} · no call running there{/if}</p>
  {#if lc}<p class="state">Backups: <b>{gcall.backups(id).map((b) => gcall.hostLabel(b)).join(', ') || 'none'}</b></p>{/if}

  <h4>People</h4>
  <label class="row"><span>Person</span>
    <select bind:value={who} aria-label="Person in the call">{#each others as m (m.id)}<option value={m.id}>{displayName(m.id)}</option>{/each}</select></label>
  <div class="btns">
    <button type="button" disabled={!!lc || !pick} onclick={() => gcall.remoteStart(id, pick)}>{firstName(pick || 'sam')} starts a call</button>
    <button type="button" disabled={!lc || !pick || !!lc?.parts.includes(pick)} onclick={() => gcall.remoteJoin(id, pick)}>Joins</button>
    <button type="button" disabled={!lc || !lc?.parts.includes(pick)} onclick={() => gcall.remoteLeave(id, pick)}>Leaves</button>
  </div>
  <div class="btns">
    <button type="button" disabled={!lc} onclick={() => fill(3)}>Fill to 3</button>
    <button type="button" disabled={!lc} onclick={() => fill(6)}>Fill to 6</button>
    <button type="button" disabled={!lc} onclick={() => fill(12)}>Fill to 12</button>
    <button type="button" onclick={addContacts}>Add all contacts to the group</button>
  </div>
  <p class="sub">{notInCall.length ? `${notInCall.length} not in the call.` : ''} {inCallOthers.length ? `${inCallOthers.length} here.` : ''}</p>

  <h4>Speaking and muting</h4>
  <label class="row"><span>People talk on their own</span><Switch label="People talk on their own" bind:checked={gcall.chatter} /></label>
  <div class="btns">
    <button type="button" disabled={!lc} onclick={() => gcall.setSpeaking(pick, true)}>{firstName(pick || 'sam')} speaks</button>
    <button type="button" disabled={!lc} onclick={() => gcall.setSpeaking(pick, false)}>Goes quiet</button>
    <button type="button" disabled={!lc} onclick={() => gcall.setSpeaking(pick, undefined)}>Back to chatter</button>
    <button type="button" disabled={!lc} onclick={() => gcall.remoteMute(pick, !gcall.remoteMuted[pick])}>{gcall.remoteMuted[pick] ? 'Unmutes' : 'Mutes'}</button>
  </div>

  <h4>The call host's device</h4>
  <label class="row"><span>Host device</span>
    <select bind:value={hostPick} aria-label="Call host device">{#each hosts as m (m.id)}<option value={m.id}>{m.id === 'me' ? 'This device' : displayName(m.id)}</option>{/each}</select></label>
  <label class="row"><span>Is a computer (else a phone)</span><Switch label="Host is a computer" checked={gcall.hostClass(hostId) === 'desktop'} onchange={(v) => { if (hostId !== 'me') hc.class = v ? 'desktop' : 'phone'; }} /></label>
  <label class="row"><span>Refuses requests</span><Switch label="Host refuses requests" bind:checked={hc.refuses} /></label>
  <label class="row"><span>Already carrying a call</span><Switch label="Host is busy" bind:checked={hc.busy} /></label>
  <p class="sub">Starting a call asks the best-ranked call host; a refusal moves on to the next, none left shows "No call host available". A phone only agrees to calls its own owner starts.</p>
  <div class="btns">
    <button type="button" disabled={!lc} onclick={() => gcall.hostEvent(id, 'lowBattery')}>Battery under 30 %</button>
    <button type="button" disabled={!lc} onclick={() => gcall.hostEvent(id, 'limited')}>Battery under 15 %</button>
    <button type="button" disabled={!lc} onclick={() => gcall.hostEvent(id, 'critical')}>Battery under 8 %</button>
    <button type="button" disabled={!lc} onclick={() => gcall.hostEvent(id, 'metered')}>Mobile data</button>
    <button type="button" disabled={!lc} onclick={() => gcall.hostEvent(id, 'recover')}>Plugged in, Wi-Fi</button>
  </div>
  <div class="btns">
    <button type="button" disabled={!lc} onclick={() => gcall.hostEvent(id, 'lost')}>Host connection lost</button>
    <button type="button" disabled={!lc} onclick={() => gcall.hostEvent(id, 'rotate')}>Host rotation due</button>
    <button type="button" disabled={!lc} onclick={() => gcall.hostEnds(id)}>Host's user ends hosting</button>
  </div>
  <p class="sub">Host: {hc.power === 'mains' ? 'plugged in' : hc.pct + ' %'} · {hc.metered ? 'mobile data' : 'Wi-Fi'} · {hc.thermal}</p>

  <h4>A 1:1 call during a group call</h4>
  <div class="row"><label for="gcsim-ring">Caller</label>
    <select id="gcsim-ring" bind:value={ringer}>{#each callers as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select>
    <button type="button" onclick={ringOne}>Calls me</button></div>
  <p class="sub">Group and 1:1 calls exclude each other. A 1:1 call arriving now is turned away Busy and left as a missed call; placing one is refused with a note.</p>

  <h4>Sounds</h4>
  <div class="btns">
    <button type="button" onclick={() => preview('started')}>Call started</button>
    <button type="button" onclick={() => preview('joined')}>Someone joined</button>
    <button type="button" onclick={() => preview('left')}>Someone left</button>
  </div>

  <h4>Timings</h4>
  <label class="row"><span>Asking a host (s)</span><input type="number" min="0.1" step="0.1" value={GCALL_MS.ask / 1000} onchange={(e) => (GCALL_MS.ask = Math.max(100, +e.currentTarget.value * 1000))} /></label>
  <label class="row"><span>Host lost, redial after (s)</span><input type="number" min="0.1" step="0.1" value={GCALL_MS.redialCrash / 1000} onchange={(e) => (GCALL_MS.redialCrash = Math.max(100, +e.currentTarget.value * 1000))} /></label>
  <label class="row"><span>Banner stays (s)</span><input type="number" min="1" step="1" value={GCALL_MS.bannerHold / 1000} onchange={(e) => (GCALL_MS.bannerHold = Math.max(1000, +e.currentTarget.value * 1000))} /></label>

  <div class="notes">
    <p><b>Proposed, not decided.</b></p>
    <ul>
      <li>A group call does not ring. It is a quiet banner (in the app), a heads-up or toast (out of the app) and a soft cue, with a Join button; muted groups raise nothing. The banner stays 9 s, then lives on in the conversation, the Groups list and the notification shade.</li>
      <li>One record per call per device, written when my part ends: "Group call · 12 min". A call I never joined leaves "Call started by Sam" and "Call ended · 8 min" as notices.</li>
      <li>Starting with no call host does not open a call screen: a short note says to ask an admin. Hosts that all refuse or are unavailable end in "No call host available".</li>
      <li>Roster: on a phone the first 8 people, then a "+N" tile (it lights up if one of them speaks, and the speaking line names them); on a computer up to 12.</li>
      <li>"Call host: Arjun's laptop" always shows during a call, with an info card (the call host relays the audio and can hear it). Spec Q13.</li>
      <li>When the host's own user is the call host and conditions degrade, a note appears on their screen only; others see just fewer people highlighted.</li>
      <li>After "Call ended — host lost" the screen stays until Close or Rejoin (Rejoin starts the call again through the ranking).</li>
      <li>Core facts: K loudest speakers are forwarded (3 on a computer, 2 on a phone, 1 when limited); caps 12 / 8 / 4 on mobile data; a phone hosts only while its owner is in the call.</li>
      <li>Not modelled: "blocked person in this call" warning (no data in the core yet), the metered-data setting, call-host rotation after a removal.</li>
    </ul>
  </div>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 40px; font-size: var(--fs-sub); }
  .state { font-size: var(--fs-sub); margin: 0 0 4px; }
  .sub { font-size: var(--fs-sub); color: var(--muted); margin: 4px 0 0; }
  h4 { margin: 12px 0 2px; font-size: var(--fs-sub); color: var(--muted); font-weight: 650; }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  button:not([role]) { border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  button:disabled { opacity: .45; cursor: default; }
  select, input { border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); min-width: 0; max-width: 55%; }
  input { width: 64px; }
  .notes { margin-top: 12px; font-size: var(--fs-sub); color: var(--muted); }
  .notes p { margin: 0 0 4px; }
  .notes ul { margin: 0; padding-inline-start: 18px; display: grid; gap: 3px; }
</style>
