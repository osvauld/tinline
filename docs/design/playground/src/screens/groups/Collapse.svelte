<script lang="ts">
  /* A folded section: height + opacity, grid 0fr→1fr so the content never needs a measured height.
     The closed content is inert, so it can't be tabbed into while hidden. */
  import type { Snippet } from 'svelte';
  let { open, children }: { open: boolean; children: Snippet } = $props();
</script>

<div class="c" class:open><div class="in" inert={!open}>{@render children()}</div></div>

<style>
  .c { display: grid; grid-template-rows: 0fr; opacity: 0; transition: grid-template-rows var(--dur-exit) var(--ease-in), opacity var(--dur-exit) var(--ease-in); }
  .c.open { grid-template-rows: 1fr; opacity: 1; transition-duration: var(--dur-enter); transition-timing-function: var(--ease-out); }
  .in { overflow: hidden; min-block-size: 0; }
</style>
