<script lang="ts">
  import Icon from './Icon.svelte';
  let { icon, label, onclick, size = 44, iconSize = 22, pressed, disabled = false, class: cls = '' }:
    { icon: string; label: string; onclick?: (e: MouseEvent) => void; size?: number; iconSize?: number; pressed?: boolean; disabled?: boolean; class?: string } = $props();
</script>

<button
  type="button" class="ib press {cls}" aria-label={label} title={label} {onclick} {disabled}
  aria-pressed={pressed} style:width="{size}px" style:height="{size}px"
><Icon name={icon} size={iconSize} /></button>

<style>
  .ib { display: inline-grid; place-items: center; border: 0; border-radius: 50%; background: transparent; color: inherit; cursor: pointer; flex: none; }
  .ib:hover:not(:disabled) { background: var(--surface-2); }
  .ib:disabled { opacity: .45; cursor: default; }
</style>
