<script lang="ts">
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate, DEMO_PHRASE } from '../../lib/model/actions/gate.svelte';
  const half = DEMO_PHRASE.length / 2;
  /* One primary slot, two moments: "Copy my words" first; once copied it becomes "I've saved them" in place (same
     size, so nothing moves). `gate.phraseShown` remembers it, so coming back from Check my copy keeps the second state.
     Someone writing on paper instead skips the copy with the quiet "I wrote them down". */
  const copied = $derived(gate.phraseShown);
  async function copy() { try { await navigator.clipboard.writeText(DEMO_PHRASE.join(' ')); } catch { /* clipboard may be unavailable */ } gate.phraseShown = true; }
</script>

<!-- Casual and calm, never alarming (like the app's sounds): what the words do, then a light ask. The words show
     straight away (no blur, no reveal); copying is fine and paper is a suggestion, not a rule. -->
<StepShell title="Here are your recovery words" lead={['If you ever switch devices, these 24 words bring your account along.', 'Save a copy somewhere safe. Paper works great.']}>
  <div class="wrap">
  <div class="grid" data-testid="phrase">
    {#each DEMO_PHRASE as w, i (i)}
      <span class="w" style:order={(i % half) * 2 + (i >= half ? 1 : 0)}><i>{i + 1}</i><code>{w}</code></span>
    {/each}
  </div>
  </div>
  {#snippet foot()}
    {#if copied}
      <Btn enter onclick={() => gate.go('device')}>I’ve saved them</Btn>
      <Btn kind="text" onclick={() => gate.go('confirm')}>Check my copy</Btn>
    {:else}
      <Btn enter onclick={copy}>Copy my words</Btn>
      <Btn kind="text" onclick={() => gate.go('device')}>I wrote them down</Btn>
    {/if}
  {/snippet}
</StepShell>

<style>
  .wrap { position: relative; }
  .grid { position: relative; display: grid; grid-template-columns: repeat(2, 1fr); gap: 4px 10px; padding: 4px; }
  .w { display: flex; align-items: center; gap: 10px; min-block-size: 32px; padding-inline: 12px; border: 1px solid var(--line); border-radius: 8px; background: var(--surface); }
  .w i { inline-size: 20px; text-align: end; font-style: normal; font-size: var(--fs-meta); color: var(--muted); font-variant-numeric: tabular-nums; }
  .w code { font: 15px/20px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; }
  :global([data-form='desktop']) .grid { grid-template-columns: repeat(3, 1fr); padding-block: 2px; }
  :global([data-form='desktop']) .w { min-block-size: 30px; }   /* 8 rows fit with the note in the fixed card: no scrolling */
  :global([data-form='desktop']) .w { order: unset !important; }
</style>
