<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  let { kind = 'info', icon, children }: { kind?: 'info' | 'warn' | 'error'; icon?: string; children: Snippet } = $props();
</script>

<div class="n {kind}" role={kind === 'error' ? 'alert' : undefined}>
  <Icon name={icon ?? (kind === 'info' ? 'info' : 'warning')} size={20} />
  <div>{@render children()}</div>
</div>

<style>
  .n { display: flex; gap: 12px; padding: 12px 14px; border-radius: var(--r-ctl); font-size: var(--fs-sub); line-height: 19px; align-items: flex-start; text-align: start; inline-size: 100%; }
  .n :global(svg) { margin-block-start: 1px; }
  .info { background: var(--surface-2); color: var(--ink); }
  .warn { background: var(--warn-bg); color: var(--warn-ink); }
  .error { background: var(--danger-bg); color: var(--danger); }
  .n div { min-inline-size: 0; }
</style>
