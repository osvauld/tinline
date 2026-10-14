<script lang="ts">
  /* New chat (the + button on Chats, Android dock and desktop): pick a contact to open or start the 1:1 chat. "New group" and
     "Add contact" are shortcuts at the top. The list area has a fixed height, so filtering never resizes the dialog. */
  import Icon from '../../lib/ui/Icon.svelte';
  import PersonRow from './PersonRow.svelte';
  import { db, displayName } from '../../lib/model/store.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { open, closeDialog } from '../../lib/model/actions/people.svelte';
  import { profileOf } from '../../lib/model/actions/profile.svelte';
  import { ensureChat } from '../../lib/model/actions/profile';
  import { tick } from 'svelte';

  let q = $state('');
  const list = $derived(db.people
    .filter((p) => p.contact && !p.blocked && (!q.trim() || displayName(p.id).toLocaleLowerCase().includes(q.trim().toLocaleLowerCase())))
    .sort((a, b) => displayName(a.id).localeCompare(displayName(b.id))));
  const hasChat = (id: string) => db.chats.some((c) => c.kind === 'direct' && c.person === id);
  const status = (id: string) => { const s = profileOf(id)?.fields.status; return typeof s === 'string' ? s : ''; };

  function chooseContact(id: string) {
    const c = ensureChat(id);
    closeDialog();
    if (!c) return;
    if (prefs.form === 'desktop') nav.open({ name: 'conversation', chat: c.id });
    else nav.push({ name: 'conversation', chat: c.id });
  }
  async function newGroup() {
    closeDialog();
    if (nav.tab !== 'groups' || nav.top.name !== 'home') nav.setTab('groups');
    await tick(); await new Promise((r) => setTimeout(r, 60));
    window.dispatchEvent(new CustomEvent('tinline-new-group'));
  }
</script>

<div class="nc" data-new-chat>
  <div class="head"><h2>New chat</h2>
    <label class="find"><Icon name="search" size={18} />
      <input data-autofocus type="search" bind:value={q} placeholder="Find a contact" aria-label="Find a contact" autocomplete="off" /></label>
  </div>
  <div class="scroll">
    {#if !q.trim()}
      <button type="button" class="act press" data-nc-group onclick={newGroup}><span class="plus"><Icon name="users" size={22} /></span><span class="t"><b>New group</b><small>You’ll be the owner</small></span></button>
      <button type="button" class="act press" data-nc-add onclick={() => open('addContact')}><span class="plus"><Icon name="userplus" size={22} /></span><span class="t"><b>Add contact</b><small>Share your code or paste theirs</small></span></button>
      <h3>Contacts</h3>
    {/if}
    <ul role="list">
      {#each list as p (p.id)}
        <li><PersonRow {p} sub={hasChat(p.id) ? status(p.id) : status(p.id) || 'No messages yet'} onclick={() => chooseContact(p.id)} pad={20} /></li>
      {/each}
    </ul>
    <p class="none" aria-live="polite">{#if !list.length}No contact called “{q.trim()}”.{/if}</p>
  </div>
</div>

<style>
  .nc { display: flex; flex-direction: column; block-size: 540px; max-block-size: calc(100vh - 64px); }
  .head { padding: 18px 20px 8px; flex: none; }
  h2 { margin: 0 0 12px; font-size: var(--fs-title); font-weight: 650; line-height: 24px; }
  .find { display: flex; align-items: center; gap: 8px; padding: 0 14px; block-size: 44px; border-radius: 22px; background: var(--surface-2); color: var(--muted); }
  .find input { flex: 1; min-inline-size: 0; border: 0; background: none; outline: 0; font: inherit; color: var(--ink); }
  .find:focus-within { box-shadow: 0 0 0 2px var(--accent); }
  .scroll { flex: 1; min-block-size: 0; overflow: auto; padding-block-end: 8px; }
  ul { list-style: none; margin: 0; padding: 0; }
  .act { display: flex; gap: 12px; align-items: center; inline-size: 100%; min-block-size: 64px; padding: 0 20px; border: 0; background: transparent; text-align: start; cursor: pointer; color: inherit; font: inherit; }
  .act:hover { background: var(--surface-2); }
  .plus { inline-size: 44px; block-size: 44px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent-ink); flex: none; }
  .t { display: grid; gap: 1px; } .t b { font-weight: 600; line-height: 21px; } .t small { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  h3 { margin: 0; padding: 14px 20px 4px; font-size: 12px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); line-height: 16px; }
  .none { margin: 12px 20px 0; min-block-size: 19px; text-align: center; color: var(--muted); font-size: var(--fs-sub); }
</style>
