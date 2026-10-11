<script lang="ts">
  import type { Snippet } from 'svelte';
  let { kind = 'primary', onclick, disabled = false, busy = false, children, type = 'button', label }:
    { kind?: 'primary' | 'secondary' | 'text'; onclick?: (e: MouseEvent) => void; disabled?: boolean; busy?: boolean; children: Snippet; type?: 'button' | 'submit'; label?: string } = $props();
</script>

<button {type} class="btn press {kind}" {onclick} disabled={disabled || busy} aria-busy={busy || undefined} aria-label={label}>
  {#if busy}<span class="spin" aria-hidden="true"></span>{/if}{@render children()}
</button>

<style>
  .btn { display: inline-flex; align-items: center; justify-content: center; gap: 8px; min-block-size: 48px; min-inline-size: 44px; padding-inline: 20px;
    border-radius: var(--r-ctl); border: 1px solid transparent; font-weight: 650; font-size: var(--fs-body); cursor: pointer; text-align: center; }
  .primary { background: var(--accent); color: var(--on-accent); }
  .secondary { background: transparent; color: var(--accent-ink); border-color: var(--line); }
  .secondary:hover:not(:disabled) { background: var(--surface-2); }
  .text { background: transparent; color: var(--accent-ink); }
  .text:hover:not(:disabled) { background: var(--surface-2); }
  .btn:disabled { cursor: default; }
  .primary:disabled { background: var(--surface-2); color: var(--muted); }
  .secondary:disabled, .text:disabled { opacity: .5; }
  .primary[aria-busy] { background: var(--accent); color: var(--on-accent); opacity: .85; }
  .spin { inline-size: 16px; block-size: 16px; border-radius: 50%; border: 2px solid currentColor; border-inline-end-color: transparent; animation: spin 0.8s linear infinite; }
  @keyframes spin { to { rotate: 360deg; } }
</style>
