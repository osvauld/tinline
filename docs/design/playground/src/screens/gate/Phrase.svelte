<script lang="ts">
  import Btn from './Btn.svelte';
  import Notice from './Notice.svelte';
  import StepShell from './StepShell.svelte';
  import { gate, DEMO_PHRASE } from '../../lib/model/actions/gate.svelte';
  let copied = $state(false);
  const shown = $derived(gate.phraseShown);
  const half = DEMO_PHRASE.length / 2;
  async function copy() { try { await navigator.clipboard.writeText(DEMO_PHRASE.join(' ')); } catch { /* clipboard may be unavailable */ } copied = true; }
</script>

<StepShell title="Write down these 24 words" lead="They are your Tinline account. If you lose this {gate.noun}, they bring it back. Anyone who has them can pretend to be you.">
  <div class="wrap">
  <div class="grid" class:hidden={!shown} aria-hidden={!shown} data-testid="phrase">
    {#each DEMO_PHRASE as w, i (i)}
      <span class="w" style:order={(i % half) * 2 + (i >= half ? 1 : 0)}><i>{i + 1}</i><code>{w}</code></span>
    {/each}
  </div>
  {#if !shown}
    <button type="button" class="reveal press" onclick={() => (gate.phraseShown = true)}>Show the words</button>
  {/if}
  </div>
  <Notice icon="warning" kind="warn">Write them on paper, in order, and keep it somewhere safe. Nobody, not even Osvauld, can bring your account back without them.</Notice>
  {#if shown}
    <div class="copy">
      <Btn kind="text" onclick={copy}>{copied ? 'Copied' : 'Copy anyway'}</Btn>
      <small>Not recommended. Paste only into a trusted password manager; the clipboard holds your account key.</small>
    </div>
  {/if}
  {#snippet foot()}
    <Btn disabled={!shown} onclick={() => gate.go('device')}>I’ve written them down</Btn>
    <Btn kind="text" disabled={!shown} onclick={() => gate.go('confirm')}>Check my copy</Btn>
  {/snippet}
</StepShell>

<style>
  .wrap { position: relative; }
  .grid { position: relative; display: grid; grid-template-columns: repeat(2, 1fr); gap: 6px 10px; padding: 4px; }
  .w { display: flex; align-items: center; gap: 10px; min-block-size: 36px; padding-inline: 12px; border: 1px solid var(--line); border-radius: 8px; background: var(--surface); }
  .w i { inline-size: 20px; text-align: end; font-style: normal; font-size: var(--fs-meta); color: var(--muted); font-variant-numeric: tabular-nums; }
  .w code { font: 15px/20px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; }
  .hidden .w { filter: blur(7px); user-select: none; pointer-events: none; }
  .reveal { position: absolute; inset: 0; margin: auto; inline-size: max-content; block-size: 48px; padding-inline: 22px; border-radius: var(--r-ctl); border: 1px solid var(--line);
    background: var(--surface-raised); color: var(--ink); font-weight: 650; cursor: pointer; box-shadow: var(--shadow-lift); }
  .copy { display: grid; justify-items: start; gap: 2px; }
  .copy small { color: var(--muted); font-size: var(--fs-sub); padding-inline: 4px; }
  :global([data-form='desktop']) .grid { grid-template-columns: repeat(3, 1fr); }
  :global([data-form='desktop']) .w { order: unset !important; }
</style>
