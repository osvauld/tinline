<script lang="ts">
  /* Calls simulation: who rings me, what the other side does, and the network. Drawer only. */
  import { db } from '../model/store.svelte';
  import { call, CALL_MS } from '../calls/call.svelte';
  import { system } from '../calls/system.svelte';
  import { prefs } from '../model/prefs.svelte';
  import Switch from '../ui/Switch.svelte';
  import { field } from '../calls/field/texture.svelte';

  let from = $state('arjun');
  let note = $state('');
  const callers = $derived(db.people.filter((p) => p.id !== 'me'));
  const android = $derived(prefs.form === 'android');
  const ring = () => {
    if (call.receive(from)) { note = ''; return; }
    if (db.account.gate !== 'ready') { note = 'The account key is not on this device: nothing rings.'; return; }
    const r = call.refusal(from);
    note = r === 'blocked' ? 'Blocked: the call never reaches the screen.' : r === 'messages-only' ? 'Message-only contact: calls are not delivered.'
      : r === 'busy' ? (call.waiting ? 'Someone is already waiting: the core turns this one away busy, left as a missed call.' : from === call.peer ? 'That is the person on the call.' : 'Not an answered call yet: the core turns this one away busy, left as a missed call.') : 'Unknown caller.';
  };
  const inCall = $derived(call.phase === 'active' || call.phase === 'reconnecting');
  let during = $state('sana');
  const duringOpts = $derived(callers.filter((p) => p.id !== call.peer));
  const third = () => {
    const p = callers.find((c) => c.id !== call.peer && c.id !== call.waiting?.peer && !c.blocked && c.callsAllowed);
    if (p) call.receive(p.id);
  };
  const ringDuring = () => {
    if (call.receive(during)) { note = ''; return; }
    note = call.refusal(during) === 'busy' ? 'Not possible now: the person is on the call, or someone is already waiting.' : 'Blocked or message-only: the call never reaches the screen.';
  };
  const out = $derived(call.phase === 'outgoing');
  const live = $derived(call.phase === 'active' || call.phase === 'reconnecting' || call.phase === 'connecting');
</script>

<div class="rows">
  <p class="state">Now: <b>{call.phase}</b>{#if call.peer}{' '}with {call.peer}{/if}{#if call.phase === 'outgoing'} · {call.ringing ? 'their device is ringing' : 'reaching their device'}{/if}</p>

  <div class="row">
    <label for="callsim-from">Incoming from</label>
    <select id="callsim-from" bind:value={from}>{#each callers as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select>
    <button type="button" onclick={ring}>Ring me</button>
  </div>
  {#if note}<p class="note" role="status">{note}</p>{/if}
  <div class="btns">
    <button type="button" disabled={call.phase !== 'incoming'} onclick={() => call.remote('cancel')}>Caller hangs up</button>
  </div>

  <h4>Call waiting</h4>
  <p class="state">Waiting: <b>{call.waiting ? call.waiting.peer : 'nobody'}</b></p>
  <div class="row">
    <label for="callsim-during">Someone calls during the call</label>
  </div>
  <div class="row">
    <select id="callsim-during" bind:value={during}>{#each duringOpts as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select>
    <button type="button" disabled={!inCall || !!call.waiting} onclick={ringDuring}>Ring over the call</button>
  </div>
  <div class="btns">
    <button type="button" disabled={!call.waiting} onclick={() => call.remote('cancel')}>Waiting caller gives up</button>
    <button type="button" disabled={!call.waiting} onclick={third}>A third person calls</button>
  </div>

  <h4>Where {android ? 'the phone' : 'the window'} is</h4>
  {#if android}
    <label class="row"><span>Phone locked</span><Switch label="Phone locked" bind:checked={system.locked} onchange={(v) => system.setLocked(v)} /></label>
  {/if}
  <label class="row"><span>{android ? 'Tinline in background' : 'Window minimised'}</span><Switch label={android ? 'Tinline in background' : 'Window minimised'} bind:checked={system.background} onchange={(v) => system.setBackground(v)} /></label>
  {#if android}
    <div class="btns"><button type="button" onclick={() => (system.shadeOpen ? system.closeShade() : system.openShade())}>Pull down notifications</button></div>
  {/if}
  <p class="note">Calls ring only when the account key is remembered on this device. While the app is passphrase-locked there is no ring and no notification. The phone lock screen is a separate thing.</p>

  <h4>The other side (my outgoing call)</h4>
  <div class="btns">
    <button type="button" disabled={!out} onclick={() => call.remote('answer')}>Answers</button>
    <button type="button" disabled={!out} onclick={() => call.remote('decline')}>Declines</button>
    <button type="button" disabled={!out} onclick={() => call.remote('noanswer')}>No answer</button>
    <button type="button" disabled={!out} onclick={() => call.remote('busy')}>Busy</button>
    <button type="button" disabled={!live} onclick={() => call.remote('hangup')}>Hangs up</button>
  </div>

  <h4>Network</h4>
  <div class="btns">
    <button type="button" disabled={call.phase !== 'active'} onclick={() => call.drop()}>Drop connection</button>
    <button type="button" disabled={call.phase !== 'reconnecting'} onclick={() => call.recover()}>Recovered</button>
    <button type="button" disabled={call.phase !== 'reconnecting'} onclick={() => call.dropForGood()}>Dropped for good</button>
  </div>

  <h4>Call background</h4>
  <div class="row">
    <label for="callsim-field">Call background</label>
    <select id="callsim-field" value={field.texture} onchange={(e) => field.set(e.currentTarget.value === 'clouds' ? 'clouds' : 'fog')}><option value="fog">Fog</option><option value="clouds">Clouds</option></select>
  </div>
  <p class="note">Compare: Fog is the soft colour haze; Clouds is puffier, with lit edges and depth. Both move with the tune. Switches live, even during a call.</p>

  <h4>Timings</h4>
  <label class="row"><span>Waiting-call timeout (s)</span><input type="number" min="1" max="120" value={CALL_MS.waitingTimeout / 1000} onchange={(e) => (CALL_MS.waitingTimeout = Math.max(1, +e.currentTarget.value) * 1000)} /></label>
  <label class="row"><span>Ring timeout (s)</span><input type="number" min="1" max="120" value={CALL_MS.ringTimeout / 1000} onchange={(e) => (CALL_MS.ringTimeout = Math.max(1, +e.currentTarget.value) * 1000)} /></label>

  <p class="note"><i class="prop">Proposed</i> Ring timeout: 20 s, then "No answer" (they are online) or "Couldn't connect" (offline or unreachable). Not decided.</p>
  <p class="note"><i class="prop">Proposed</i> A missed incoming call adds one unread to its chat. Not decided; answered and declined calls never do.</p>
  <p class="note"><i class="prop">Proposed</i> "Calling…" until their device answers the signal, then "Ringing…". An offline person stays on "Calling…" until the timeout.</p>
  <p class="note"><i class="prop">Proposed</i> "Busy" has no copy of its own: the call ends with "Call ended" and is recorded as Declined.</p>
  <p class="note"><i class="prop">Proposed</i> "End-to-end encrypted" is shown only after connection. Confirm against the core before launch.</p>
  <p class="note">Decided 2026-10-12: the lock screen shows the caller's name.</p>
  <p class="note">Decided 2026-10-12: missed-call notifications fold into one per person ("2 missed calls · Sana Rao").</p>
  <p class="note"><i class="prop">Proposed</i> Tapping a call notification on the lock screen unlocks the phone first (there is no PIN in this simulation).</p>
  <p class="note">Core facts: while an answered call runs, ONE more incoming call rings over it (waiting, 30 s). A further call is turned away busy and left as a missed call. A call still dialling or connecting also turns a new caller away busy.</p>
  <p class="note">Core facts: the waiting call is declined (its caller is told busy, recorded as declined) or answered by ending the current call. Hold is not supported by the core: there is no switching between two calls.</p>
  <p class="note">Core facts: if the current call ends by itself, the waiting call takes the slot as an ordinary incoming call. If the waiting caller gives up (or 30 s pass) it leaves a missed-call record.</p>
  <p class="note"><i class="prop">Proposed</i> The waiting cue is two short soft tones every 3 s, quieter than the ring, in each sound palette; the in-call field leans a little toward the incoming tone. Not decided.</p>
  <p class="note"><i class="prop">Proposed</i> A minimised call shows the in-app banner (Decline / End &amp; answer) over the strip rather than squeezing actions into the strip. The call screen carries a card above its controls.</p>
  <p class="note"><i class="prop">Proposed</i> When the waiting call takes the slot after a hang-up, it rings full-screen if the call screen was open, else as the banner. The card disappears while "Call ended" is shown.</p>
  <p class="note">Incoming calls from blocked people or message-only contacts are dropped silently. Desktop has no missed-call toast yet.</p>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .state { font-size: var(--fs-sub); margin: 0 0 4px; }
  .row { display: flex; align-items: center; gap: 8px; min-height: 40px; font-size: var(--fs-sub); }
  .row label, .row span { flex: none; }
  select, input { border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); min-width: 0; }
  input { width: 64px; margin-inline-start: auto; }
  h4 { font-size: var(--fs-sub); margin: 12px 0 2px; }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 4px; }
  button { border: 1px solid var(--line); background: var(--surface); color: var(--ink); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  button:disabled { opacity: .45; cursor: default; }
  .note { font-size: var(--fs-sub); color: var(--muted); margin: 6px 0 0; }
  .prop { font-style: normal; font-weight: 700; color: var(--gold-ink); margin-inline-end: 4px; }
</style>
