<script lang="ts">
  /* A generic launcher / desktop icon for the simulated "other app" screens. Neutral on purpose. */
  import Icon from '../ui/Icon.svelte';
  let { icon, label }: { icon: string; label: string } = $props();
</script>

<span class="tile"><span class="sq"><Icon name={icon} size={24} /></span><span class="lb">{label}</span></span>

<style>
  .tile { display: grid; justify-items: center; gap: 6px; inline-size: 64px; color: var(--os-ink); }
  .sq { inline-size: 52px; block-size: 52px; border-radius: 16px; display: grid; place-items: center; background: var(--os-tile); border: 1px solid var(--os-line); }
  .lb { font-size: 11px; line-height: 14px; font-weight: 500; color: var(--os-ink); }
</style>
