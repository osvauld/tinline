<script lang="ts">
  import { simSections } from './sections';
  import IconButton from '../ui/IconButton.svelte';
  let { open = $bindable(false) }: { open?: boolean } = $props();
</script>

{#if open}
  <aside class="drawer" aria-label="Simulation">
    <header><h2>Simulation</h2><IconButton icon="x" label="Close simulation" onclick={() => (open = false)} /></header>
    <p class="note">Everything here is simulated: people, network, permissions. Controls live only in this drawer, never in the app.</p>
    {#each simSections as s (s.title)}
      <section><h3>{s.title}</h3><s.component /></section>
    {/each}
  </aside>
{/if}

<style>
  .drawer { position: fixed; inset-block: 0; inset-inline-end: 0; width: min(340px, 92vw); z-index: 50; overflow: auto;
    background: var(--surface-raised); border-inline-start: 1px solid var(--line); box-shadow: var(--shadow-sheet); padding: 8px 16px 24px;
    animation: slide var(--dur-enter) var(--ease-out); }
  @keyframes slide { from { translate: 24px 0; opacity: 0; } }
  header { display: flex; align-items: center; justify-content: space-between; }
  h2 { font-size: var(--fs-title); margin: 0; }
  h3 { font-size: var(--fs-meta); text-transform: uppercase; letter-spacing: .06em; color: var(--muted); margin: 18px 0 4px; }
  .note { font-size: var(--fs-sub); color: var(--muted); margin: 4px 0 0; }
</style>
