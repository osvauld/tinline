<script lang="ts">
  /* Privacy (route `privacy`): a landing of rows, and one page per area (`section`): blocked, calls, phrase, passphrase.
     Only what the core supports is offered. Read receipts: a core account setting (default on; off sends no read
     receipts, delivered still goes, others' reads still show; no backdating). Online visibility is NOT shown: the core
     has none. */
  import '../profile/profile.css';
  import Icon from '../../lib/ui/Icon.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Blocked from './Blocked.svelte';
  import CallsAllowed from './CallsAllowed.svelte';
  import Phrase from './Phrase.svelte';
  import ChangePass from './ChangePass.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import { db } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { pp } from '../../lib/model/actions/people.svelte';

  let { section }: { section?: string } = $props();
  const TITLES: Record<string, string> = { blocked: 'Blocked people', calls: 'Who can call me', phrase: 'Recovery phrase', passphrase: 'Passphrase' };
  const title = $derived(section && TITLES[section] ? TITLES[section] : 'Privacy');
  const blockedN = $derived(db.people.filter((p) => p.blocked).length);
  const msgOnly = $derived(db.people.filter((p) => p.contact && !p.blocked && !p.callsAllowed).length);
  const ROWS = $derived([
    { icon: 'off', key: 'blocked', label: 'Blocked people', sub: blockedN ? `${blockedN} ${blockedN === 1 ? 'person' : 'people'}` : 'No one' },
    { icon: 'phone', key: 'calls', label: 'Who can call me', sub: msgOnly ? `${msgOnly} ${msgOnly === 1 ? 'contact is' : 'contacts are'} messages only` : 'All contacts can call' },
    { icon: 'key', key: 'phrase', label: 'Recovery phrase', sub: 'Needs your passphrase' },
    { icon: 'lock', key: 'passphrase', label: 'Passphrase', sub: 'Protects Tinline on this device' },
  ]);
</script>

<div class="pf" data-privacy={section ?? 'home'}>
  <header class="pf-bar"><IconButton icon="back" label="Back" onclick={() => nav.back()} /><b>{title}</b></header>
  {#if section === 'blocked'}<Blocked />
  {:else if section === 'calls'}<CallsAllowed />
  {:else if section === 'phrase'}<Phrase />
  {:else if section === 'passphrase'}<ChangePass />
  {:else}
  <div class="pf-scroll"><div class="pf-col">
    <section class="pf-sec first" aria-label="Privacy areas">
      <div class="pf-card">
        {#each ROWS as r (r.key)}
          <button type="button" class="pf-act press" data-privacy-row={r.key} onclick={() => nav.push({ name: 'privacy', section: r.key })}>
            <Icon name={r.icon} size={20} /><span class="t"><b>{r.label}</b><small>{r.sub}</small></span><Icon name="chevr" size={18} class="chev" />
          </button>
        {/each}
      </div>
      <p class="pf-note" data-privacy-note>Your contacts see your name, picture and profile. Tinline doesn’t hide who you talk to, and every device linked to your account can read your chats.</p>
    </section>
    <section class="pf-sec" aria-label="Read receipts" data-privacy-receipts>
      <div class="pf-card">
        <label class="pf-act row-sw"><Icon name="check" size={20} /><span class="t"><b>Read receipts</b><small>{pp.readReceipts ? 'People see when you’ve read their messages' : 'Your reads stay with you'}</small></span>
          <Switch label="Read receipts" bind:checked={pp.readReceipts} /></label>
      </div>
      <p class="pf-note">Delivered ticks still go out either way, and you still see when others read yours. Messages you read while this is off stay unconfirmed.</p>
    </section>
  </div></div>
  {/if}
</div>

<style>
  .first { margin-block-start: 16px; }
  .row-sw { cursor: default; }
</style>
