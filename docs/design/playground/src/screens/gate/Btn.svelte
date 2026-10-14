<script lang="ts">
  import type { Snippet } from 'svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  /* `next`: a plain "Continue". On Android it becomes a round arrow at the end of the footer (the label stays as
     its accessible name); the spinner takes the arrow's place, so busy never changes its size. */
  /* `enter`: the step's Enter key action (implied by `next`). Gate clicks it on Enter; desktop shows a ↵ keycap. */
  let { kind = 'primary', onclick, disabled = false, busy = false, children, type = 'button', label, next = false, enter = next }:
    { kind?: 'primary' | 'secondary' | 'text'; onclick?: (e: MouseEvent) => void; disabled?: boolean; busy?: boolean; children: Snippet; type?: 'button' | 'submit'; label?: string; next?: boolean; enter?: boolean } = $props();
  const arrow = $derived(next && prefs.form === 'android');
  const keycap = $derived(enter && prefs.form === 'desktop');
</script>

{#if arrow}
  <button {type} class="btn press {kind} arrow" data-enter aria-keyshortcuts="Enter" {onclick} disabled={disabled || busy} aria-busy={busy || undefined} aria-label={label ?? 'Continue'}>
    {#if busy}<span class="spin" aria-hidden="true"></span>{:else}<Icon name="forward" size={24} />{/if}
  </button>
{:else}
<button {type} class="btn press {kind}" {onclick} disabled={disabled || busy} aria-busy={busy || undefined} aria-label={label}
  data-enter={enter || undefined} aria-keyshortcuts={enter ? 'Enter' : undefined}>
  {#if busy}<span class="spin" aria-hidden="true"></span>{/if}{@render children()}{#if keycap}<kbd class="key" aria-hidden="true">↵</kbd>{/if}
</button>
{/if}

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
  .arrow { inline-size: 56px; block-size: 56px; min-block-size: 56px; padding: 0; border-radius: 50%; align-self: flex-end; flex: none; }
  .btn.arrow.arrow { inline-size: 56px; }   /* beats the footer’s full-width rule */
  :global([dir='rtl']) .arrow :global(svg) { transform: scaleX(-1); }
  /* The ↵ keycap: a soft rounded chip in the button's own colour, so it reads as a hint, not a second control. */
  .key { display: inline-grid; place-items: center; min-inline-size: 22px; block-size: 20px; padding-inline: 5px; margin-inline-end: -6px;
    border-radius: 6px; font: inherit; font-size: 13px; line-height: 1; background: color-mix(in srgb, currentColor 16%, transparent); }
  .btn:disabled .key { background: color-mix(in srgb, currentColor 10%, transparent); }
  @keyframes spin { to { rotate: 360deg; } }
</style>
