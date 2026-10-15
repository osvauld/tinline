<script lang="ts">
  /* Names on this device: put sample names here, and make something arrive for a name that is not open. */
  import { db } from '../model/store.svelte';
  import { simAddSample, nameLimit } from '../model/actions/names.svelte';
  import { receiveText } from '../model/actions/chat';
  import { call } from '../calls/call.svelte';
  import { prefs } from '../model/prefs.svelte';

  let note = $state('');
  const first = (o: (typeof db.others)[number]) => o.data.people[0]?.id;
  const ring = (i: number) => {
    const o = db.others[i], id = first(o);
    note = !o.online ? `${o.name} is offline here: its calls never arrive.` : id && call.receive(id) ? '' : 'Not now: a call is already ringing or running.';
  };
  const msg = (i: number) => {
    const o = db.others[i], id = first(o);
    if (!o.online) { note = `${o.name} is offline here: nothing arrives.`; return; }
    if (id) { receiveText(id, id, 'Are you free later?'); note = ''; }
  };
</script>

<div class="rows">
  <p class="state">Open: <b>{db.account.name}</b> · {1 + db.others.filter((o) => o.online).length} of {nameLimit()} online ({prefs.form === 'android' ? 'phone' : 'desktop'} limit, Proposed)</p>
  <div class="btns">
    <button type="button" onclick={() => simAddSample('studio')}>Add “Studio”</button>
    <button type="button" onclick={() => simAddSample('home')}>Add “Home”</button>
  </div>
  {#each db.others as o, i (o.id)}
    <div class="btns">
      <span class="who">{o.name}{o.online ? '' : ' (offline)'}</span>
      <button type="button" onclick={() => ring(i)}>Call it</button>
      <button type="button" onclick={() => msg(i)}>Message it</button>
    </div>
  {/each}
  {#if note}<p class="note" role="status">{note}</p>{/if}
  <ul class="notes">
    <li><b>Decided 2026-10-14:</b> several names online at once; each is its own address with its own contacts and chats.</li>
    <li><b>Proposed:</b> phone keeps 3 names online, desktop 5 (iroh cost run: ~11–14 MB and ~10 wakeups/s per idle name). Needs a real phone battery test.</li>
    <li><b>Proposed:</b> at the limit “Add another name” waits until one is taken offline (no silent swap).</li>
    <li><b>Proposed:</b> with more than one name, every notification names its name (OS sub-text), and answering a call to another name opens that name.</li>
  </ul>
</div>

<style>
  .rows { display: grid; gap: 8px; font-size: var(--fs-sub); }
  .state, .note { margin: 0; color: var(--muted); }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .who { min-inline-size: 90px; font-weight: 600; }
  .notes { margin: 4px 0 0; padding-inline-start: 18px; color: var(--muted); display: grid; gap: 4px; }
</style>
