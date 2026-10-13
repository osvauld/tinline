<script lang="ts">
  import Btn from './Btn.svelte';
  import Field from './Field.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  const sugg = $derived(gate.isAndroid ? ['Personal phone', 'Work phone', 'Tablet'] : ['Home computer', 'Work computer', 'Laptop']);
  if (!gate.deviceLabel) gate.deviceLabel = gate.defaultLabel;
  let busy = $state(false);
  const next = () => { if (!gate.deviceLabel.trim()) return; busy = true; gate.deviceLabel = gate.deviceLabel.trim(); busy = false; gate.nextAfterDevice(); };
</script>

<StepShell title="Name this {gate.noun}" lead="So you can tell your devices apart in Linked devices. Only your own devices see this name; people you call never do." form onsubmit={next}>
  <Field label="Device name" bind:value={gate.deviceLabel} autofocus hint="You can rename it later." />
  <div class="chips" role="group" aria-label="Suggestions">
    {#each sugg as s (s)}
      <button type="button" class="t-chip press" aria-pressed={gate.deviceLabel === s} onclick={() => (gate.deviceLabel = s)}>{s}</button>
    {/each}
  </div>
  {#snippet foot()}
    <Btn type="submit" {busy} disabled={!gate.deviceLabel.trim()}>Continue</Btn>
  {/snippet}
</StepShell>

<style>
  .chips { display: flex; flex-wrap: wrap; gap: 8px; }
  .chips :global(.t-chip) { min-block-size: 44px; padding-inline: 16px; cursor: pointer; }
</style>
