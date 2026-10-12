<script lang="ts">
  import Mark from '../../lib/brand/Mark.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
</script>

<StepShell title={gate.adding ? 'Add an account' : 'Call people directly.'} center
  lead={gate.adding ? 'Create a new account or bring one you already have. Your current account stays on this ' + gate.noun + '.' : 'Tinline connects your ' + gate.noun + ' straight to theirs. End-to-end encrypted. No phone number, no ads, no tracking.'}>
  {#snippet children()}
    <div class="hero"><Mark size={112} live label="Tinline" /></div>
    <p class="warn"><b>Not for emergency calls.</b> Use your {gate.isAndroid ? 'phone’s dialer' : 'phone'} to reach emergency services.</p>
  {/snippet}
  {#snippet foot()}
    <div class="acts" data-welcome-actions>
      <Btn onclick={() => gate.startCreate()}>Create account</Btn>
      <Btn kind="secondary" onclick={() => gate.startLink()}>Link to a device</Btn>
      <Btn kind="secondary" onclick={() => gate.startRestore()}>Restore from recovery phrase</Btn>
      {#if gate.adding}<Btn kind="text" onclick={() => gate.cancelAdd()}>Cancel</Btn>{/if}
    </div>
  {/snippet}
</StepShell>

<style>
  .hero { order: -1; display: grid; place-items: center; padding-block: 28px 8px; color: var(--ink); }
  .warn { margin: 0; font-size: var(--fs-sub); color: var(--muted); max-inline-size: 36ch; }
  .acts { display: contents; }
  :global([data-form='desktop']) .acts { display: grid; gap: 8px; inline-size: min(100%, 340px); margin-inline: auto; }
  :global([data-form='desktop']) .acts :global(.btn) { inline-size: 100%; }
  :global(.step.center h1) { font-size: 30px; line-height: 36px; }
</style>
