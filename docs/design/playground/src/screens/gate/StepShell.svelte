<script lang="ts">
  /* One step: heading (focus target), optional lead, body, footer actions. Layout differs by form factor in CSS. */
  import { onMount, type Snippet } from 'svelte';
  let { title, lead, center = false, anchor = false, children, foot, form, onsubmit }:
    { title: string; lead?: string | string[]; center?: boolean; anchor?: boolean; children?: Snippet; foot?: Snippet; form?: boolean; onsubmit?: () => void } = $props();
  let root: HTMLElement, h1: HTMLElement;
  onMount(() => {
    const f = root.querySelector<HTMLElement>('[data-autofocus]');
    (f ?? h1).focus({ preventScroll: true });
  });
</script>

<svelte:element this={form ? 'form' : 'div'} bind:this={root} class="step" class:center class:anchor novalidate
  onsubmit={form ? (e: Event) => { e.preventDefault(); onsubmit?.(); } : undefined}>
  <div class="body">
    <h1 bind:this={h1} tabindex="-1">{title}</h1>
    {#if Array.isArray(lead)}<p class="lead">{#each lead as l, i (i)}<span class="ln">{l}</span>{/each}</p>{:else if lead}<p class="lead">{lead}</p>{/if}
    {@render children?.()}
  </div>
  {#if foot}<div class="foot">{@render foot()}</div>{/if}
</svelte:element>

<style>
  .step { display: flex; flex-direction: column; block-size: 100%; min-block-size: 0; }
  .body { flex: 1; min-block-size: 0; overflow: auto; display: flex; flex-direction: column; gap: 16px; padding: 8px 24px 16px; scrollbar-width: thin; }
  h1 { margin: 0; font-size: 26px; line-height: 32px; font-weight: 700; letter-spacing: -.01em; outline: none; text-wrap: balance; }
  .lead { margin: 0; color: var(--muted); font-size: var(--fs-body); line-height: 22px; }
  /* A lead given as sentences: each sentence starts its own line and wraps balanced on its own. */
  .lead .ln { display: block; text-wrap: balance; }
  /* Trial: ?align=center. Text centres; inputs, lists and grids keep full width. */
  :global([data-align='center']) h1, :global([data-align='center']) .lead { text-align: center; text-wrap: balance; }
  :global([data-align='center']) .lead .ln { margin-inline: auto; }
  :global([data-align='center'] .warn), :global([data-align='center'] .agree) { text-align: center; }
  .center .body { align-items: center; text-align: center; justify-content: center; }
  /* Centred steps whose content can grow (an error, a lockout) hang from a fixed point instead, so adding a line never re-centres the rest. */
  .center.anchor .body { justify-content: flex-start; padding-block-start: min(14dvh, 96px); }
  .foot { display: flex; flex-direction: column; gap: 8px; padding: 12px 24px 24px; }
  .foot :global(.btn) { inline-size: 100%; }
  /* Android "Continue" is a round arrow at the end; a secondary action (Skip for now) shares its row at the start. */
  .foot:has(:global(.arrow)) { flex-direction: row-reverse; justify-content: space-between; align-items: center; }
  .foot:has(:global(.arrow)) :global(.btn:not(.arrow)) { inline-size: auto; }
  :global([data-form='desktop']) .body { padding: 8px 40px 16px; }
  :global([data-form='desktop']) h1 { font-size: 28px; line-height: 34px; }
  :global([data-form='desktop']) .foot { flex-direction: row-reverse; justify-content: flex-start; padding: 16px 40px 32px; gap: 12px; }
  :global([data-form='desktop']) .foot :global(.btn) { inline-size: auto; min-inline-size: 220px; }   /* wide enough for the longest, "Add my first contact" + its ↵ keycap and the busy label (spinner + "Encrypting…"), so it never changes width */
</style>
