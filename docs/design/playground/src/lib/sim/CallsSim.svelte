<script lang="ts">
  /* Calls simulation: who rings me, what the other side does, and the network. Drawer only. */
  import { db } from '../model/store.svelte';
  import { call, CALL_MS } from '../calls/call.svelte';

  let from = $state('arjun');
  let note = $state('');
  const callers = $derived(db.people.filter((p) => p.id !== 'me'));
  const ring = () => {
    if (call.receive(from)) { note = ''; return; }
    const r = call.refusal(from);
    note = r === 'blocked' ? 'Blocked: the call never reaches the screen.' : r === 'messages-only' ? 'Message-only contact: calls are not delivered.'
      : r === 'busy' ? 'Already in a call: this one is left as a missed call.' : 'Unknown caller.';
  };
  const out = $derived(call.phase === 'outgoing');
  const live = $derived(call.phase === 'active' || call.phase === 'reconnecting' || call.phase === 'connecting');
</script>

<div class="rows">
  <p class="state">Now: <b>{call.phase}</b>{#if call.peer} with {call.peer}{/if}{#if call.phase === 'outgoing'} · {call.ringing ? 'their device is ringing' : 'reaching their device'}{/if}</p>

  <div class="row">
    <label for="callsim-from">Incoming from</label>
    <select id="callsim-from" bind:value={from}>{#each callers as p (p.id)}<option value={p.id}>{p.name}</option>{/each}</select>
    <button type="button" onclick={ring}>Ring me</button>
  </div>
  {#if note}<p class="note" role="status">{note}</p>{/if}
  <div class="btns">
    <button type="button" disabled={call.phase !== 'incoming'} onclick={() => call.remote('cancel')}>Caller hangs up</button>
  </div>

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

  <h4>Timings</h4>
  <label class="row"><span>Ring timeout (s)</span><input type="number" min="1" max="120" value={CALL_MS.ringTimeout / 1000} onchange={(e) => (CALL_MS.ringTimeout = Math.max(1, +e.currentTarget.value) * 1000)} /></label>

  <p class="note"><i class="prop">Proposed</i> Ring timeout: 20 s, then "No answer" (they are online) or "Couldn't connect" (offline or unreachable). Not decided.</p>
  <p class="note"><i class="prop">Proposed</i> A missed incoming call adds one unread to its chat. Not decided; answered and declined calls never do.</p>
  <p class="note"><i class="prop">Proposed</i> "Calling…" until their device answers the signal, then "Ringing…". An offline person stays on "Calling…" until the timeout.</p>
  <p class="note"><i class="prop">Proposed</i> "Busy" has no copy of its own: the call ends with "Call ended" and is recorded as Declined.</p>
  <p class="note"><i class="prop">Proposed</i> "End-to-end encrypted" is shown only after connection. Confirm against the core before launch.</p>
  <p class="note">Incoming calls from blocked people or message-only contacts are dropped silently. Lock-screen full-screen incoming and the ongoing-call notification are the next platform pieces.</p>
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
