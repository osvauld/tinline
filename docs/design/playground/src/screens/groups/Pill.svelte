<script lang="ts">
  import type { Snippet } from 'svelte';
  let { tone = 'muted', children }: { tone?: 'owner' | 'admin' | 'host' | 'call' | 'warn' | 'muted'; children: Snippet } = $props();
</script>

<span class="p {tone}">{@render children()}</span>

<style>
  .p { display: inline-flex; align-items: center; block-size: 20px; padding: 0 8px; border-radius: 10px; font-size: 11px; font-weight: 650; line-height: 1; letter-spacing: .01em; white-space: nowrap; flex: none; }
  .owner { background: var(--accent-soft); color: var(--accent-ink); }
  .admin { background: var(--lav); color: var(--lav-ink); }
  .host { background: var(--warn-bg); color: var(--warn-ink); }
  .call { background: var(--surface-2); color: var(--ink); box-shadow: inset 0 0 0 1px var(--line); }
  .warn { background: var(--danger-bg); color: var(--danger); }
  .muted { background: var(--surface-2); color: var(--muted); }
</style>
