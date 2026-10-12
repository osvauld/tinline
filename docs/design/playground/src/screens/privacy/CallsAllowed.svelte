<script lang="ts">
  /* Who can call me: the default for people I add, then each contact's Allow calls. The same switch lives on their profile. */
  import Switch from '../../lib/ui/Switch.svelte';
  import PersonRow from '../people/PersonRow.svelte';
  import { db, displayName } from '../../lib/model/store.svelte';
  import { pp } from '../../lib/model/actions/people.svelte';

  const list = $derived(db.people.filter((p) => p.contact && !p.blocked).sort((a, b) => displayName(a.id).localeCompare(displayName(b.id))));
</script>

<div class="pf-scroll" data-calls-page>
<div class="pf-col">
  <section class="pf-sec first" aria-labelledby="cd-h">
    <h2 id="cd-h">New contacts</h2>
    <div class="pf-card">
      <label class="pf-act row-sw"><span class="t"><b>New contacts can call me</b><small>{pp.newContactsCall ? 'Calls ring as soon as you add someone' : 'People you add can message you; you allow calls one by one'}</small></span>
        <Switch label="New contacts can call me" bind:checked={pp.newContactsCall} /></label>
    </div>
  </section>
  <section class="pf-sec" aria-labelledby="ca-h">
    <h2 id="ca-h">Contacts</h2>
    <div class="pf-card">
      {#each list as p (p.id)}
        <PersonRow {p} sub={p.callsAllowed ? 'Can call you' : 'Messages only'} pad={12}>
          {#snippet end()}<Switch label="Allow calls from {displayName(p.id)}" bind:checked={p.callsAllowed} />{/snippet}
        </PersonRow>
      {/each}
    </div>
    <p class="pf-note">Someone you allowed can ring you; someone on messages only can still write. People you blocked can do neither.</p>
  </section>
</div>
</div>

<style>
  .first { margin-block-start: 16px; }
  .row-sw { cursor: default; }
</style>
