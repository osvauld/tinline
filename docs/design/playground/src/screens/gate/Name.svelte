<script lang="ts">
  import Btn from './Btn.svelte';
  import Field from './Field.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  const next = () => { if (gate.name.trim()) gate.go(gate.flow === 'restore' ? 'restorePass' : 'pass'); };
</script>

<StepShell title="What should people call you?" lead="Your name is shown to people you add. It never leaves your {gate.noun} otherwise." form onsubmit={next}>
  <Field label="Your name" bind:value={gate.name} autofocus autocomplete="name" hint="You can change it any time in Settings." />
  {#snippet foot()}
    <Btn type="submit" disabled={!gate.name.trim()}>Continue</Btn>
  {/snippet}
</StepShell>
