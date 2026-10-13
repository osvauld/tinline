<script lang="ts">
  /* Desktop: the field at the top (or bottom) of the list pane. Esc clears, then leaves the field. Ctrl+K focuses it from anywhere. */
  import SearchField from './SearchField.svelte';
  import { ui } from './state.svelte';
  let { active = '' }: { active?: string } = $props();
  function esc() { if (ui.q) ui.clear(); else ui.el?.blur(); }
</script>

<div class="df" data-search-field><SearchField placeholder="Search (Ctrl+K)" size={40} onescape={esc} activeId={active} /></div>

<style>
  .df { padding: 12px 12px 4px; flex: none; }
</style>
