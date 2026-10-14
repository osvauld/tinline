<script lang="ts">
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate, DEMO_PHRASE, DEMO_KEY } from '../../lib/model/actions/gate.svelte';
  const half = DEMO_PHRASE.length / 2;
  /* One primary slot, two moments: "Copy my words" first; once copied it becomes "I've saved them" in place (same
     size, so nothing moves). `gate.phraseShown` remembers it, so coming back from Check my copy keeps the second state.
     Someone writing on paper instead skips the copy with the quiet "I wrote them down". */
  const copied = $derived(gate.phraseShown);
  /* Words or key: the same secret two ways (core recovery_key.rs). Words suit paper; the key suits a password
     manager. Switching never loses "copied": either copy counts as saved. */
  let asKey = $state(false);
  async function copy() { try { await navigator.clipboard.writeText(asKey ? DEMO_KEY : DEMO_PHRASE.join(' ')); } catch { /* clipboard may be unavailable */ } gate.phraseShown = true; }
  const lead = $derived(asKey
    ? ['Your recovery key is the same as your 24 words, as one line.', 'Paste it somewhere safe. A password manager works great.']
    : ['If you ever switch devices, these 24 words bring your account along.', 'Save a copy somewhere safe. Paper works great.']);
</script>

<!-- Casual and calm, never alarming (like the app's sounds): what the words do, then a light ask. The words show
     straight away (no blur, no reveal); copying is fine and paper is a suggestion, not a rule. -->
<StepShell title={asKey ? 'Here is your recovery key' : 'Here are your recovery words'} {lead}>
  <div class="seg" role="group" aria-label="Show as">
    <button type="button" aria-pressed={!asKey} onclick={() => (asKey = false)}>Words</button>
    <button type="button" aria-pressed={asKey} onclick={() => (asKey = true)}>Key</button>
  </div>
  <div class="wrap">
  {#if asKey}
    <code class="key" data-testid="recovery-key">{DEMO_KEY}</code>
  {:else}
  <div class="grid" data-testid="phrase">
    {#each DEMO_PHRASE as w, i (i)}
      <span class="w" style:order={(i % half) * 2 + (i >= half ? 1 : 0)}><i>{i + 1}</i><code>{w}</code></span>
    {/each}
  </div>
  {/if}
  </div>
  {#snippet foot()}
    {#if copied}
      <Btn enter onclick={() => gate.go('device')}>{asKey ? 'I’ve saved it' : 'I’ve saved them'}</Btn>
      <!-- The words get a quiz; the key needs none (its checksum catches a bad copy when it is pasted). -->
      {#if !asKey}<Btn kind="text" onclick={() => gate.go('confirm')}>Check my copy</Btn>{/if}
    {:else}
      <Btn enter onclick={copy}>{asKey ? 'Copy my key' : 'Copy my words'}</Btn>
      <Btn kind="text" onclick={() => gate.go('device')}>{asKey ? 'I saved it elsewhere' : 'I wrote them down'}</Btn>
    {/if}
  {/snippet}
</StepShell>

<style>
  .wrap { position: relative; }
  .seg { display: inline-flex; justify-self: start; align-self: flex-start; inline-size: fit-content; gap: 4px; padding: 3px; border-radius: 12px; background: var(--surface-2); margin-block: -6px 2px; }
  .seg button { border: 0; padding: 0 14px; min-block-size: 36px; border-radius: 9px; font: inherit; font-size: var(--fs-sub); background: transparent; color: var(--ink); cursor: pointer; }
  .seg button[aria-pressed='true'] { background: var(--surface-raised, var(--bg)); box-shadow: var(--shadow-lift); font-weight: 600; }
  /* The key: one line where it fits, broken anywhere (it has no spaces), monospace, same card look as the words. */
  .key { display: block; padding: 14px 16px; border: 1px solid var(--line); border-radius: 10px; background: var(--surface); font: 15px/24px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; word-break: break-all; user-select: all; }
  .grid { position: relative; display: grid; grid-template-columns: repeat(2, 1fr); gap: 4px 10px; padding: 4px; }
  .w { display: flex; align-items: center; gap: 10px; min-block-size: 32px; padding-inline: 12px; border: 1px solid var(--line); border-radius: 8px; background: var(--surface); }
  .w i { inline-size: 20px; text-align: end; font-style: normal; font-size: var(--fs-meta); color: var(--muted); font-variant-numeric: tabular-nums; }
  .w code { font: 15px/20px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; }
  :global([data-form='desktop']) .grid { grid-template-columns: repeat(3, 1fr); padding-block: 2px; }
  :global([data-form='desktop']) .w { min-block-size: 30px; }   /* 8 rows fit with the note in the fixed card: no scrolling */
  :global([data-form='desktop']) .w { order: unset !important; }
</style>
