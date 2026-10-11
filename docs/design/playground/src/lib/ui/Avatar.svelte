<script lang="ts">
  import Icon from './Icon.svelte';
  import { initials } from '../model/store.svelte';
  let { name, hue = 0, size = 48, group = false }: { name: string; hue?: number; size?: number; group?: boolean } = $props();
</script>

<span
  class="t-avatar {group ? 'grp' : 'av' + (hue % 6)}"
  style:width="{size}px" style:height="{size}px" style:font-size="{Math.round(size * 0.34)}px"
  aria-hidden="true"
>
  {#if group}<Icon name="users" size={Math.round(size * 0.46)} />{:else}{initials(name)}{/if}
</span>

<style>
  .grp { background: var(--surface-2); color: var(--accent-ink); }
</style>
