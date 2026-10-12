<script lang="ts">
  /* Android list entry: a search pill + the new-chat button in one row. In "Bottom" it sits above the tab bar (thumb reach), in
     "Top" under the title. It only opens the search screen; the text field lives there. */
  import Icon from '../../lib/ui/Icon.svelte';
  import { ui } from './state.svelte';
  import { nav, type Tab } from '../../lib/nav/router.svelte';
  import { open } from '../../lib/model/actions/people.svelte';

  let { tab, edge }: { tab: Tab; edge: 'bottom' | 'top' } = $props();
  const label = $derived(tab === 'groups' ? 'New group' : 'New chat');
  // Chats: the New chat picker (contacts, New group, Add contact). Groups: the same "New group" dialog as the row in the list.
  function add() { if (tab === 'groups') window.dispatchEvent(new CustomEvent('tinline-new-group')); else open('newChat'); }
</script>

<div class="dock {edge}" data-search-dock={edge}>
  <button type="button" class="pill press" onclick={() => ui.openScreen()} aria-label="Search">
    <Icon name="search" size={20} /><span>Search</span>
  </button>
  <button type="button" class="add press" onclick={add} aria-label={label} title={label}><Icon name="plus" size={24} /></button>
</div>

<style>
  .dock { display: flex; align-items: center; gap: 10px; padding: 8px 12px; background: var(--bg); flex: none; }
  .dock.bottom { padding-block-end: 10px; }
  .pill { flex: 1; min-inline-size: 0; block-size: 48px; display: flex; align-items: center; gap: 10px; padding: 0 16px; border: 0; border-radius: 24px; background: var(--surface-2); color: var(--muted);
    font-size: var(--fs-body); text-align: start; cursor: pointer; }
  .pill:hover { background: var(--hairline); }
  .add { flex: none; inline-size: 48px; block-size: 48px; border: 0; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent-ink); cursor: pointer; }
  .add:hover { filter: brightness(.97); }
</style>
