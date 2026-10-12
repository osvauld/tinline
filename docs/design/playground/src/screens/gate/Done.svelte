<script lang="ts">
  import Mark from '../../lib/brand/Mark.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  const first = $derived(gate.name.trim().split(/\s+/)[0] ?? '');
  const copy = $derived(
    gate.adding && gate.flow === 'restore' ? ['Added ' + (gate.name.trim() || 'your account') + ' to this ' + gate.noun, 'Only the account you select is online at a time, so ' + (gate.name.trim() || 'this account') + ' is online now and your other account is paused. You can switch in Settings.']
    : gate.flow === 'restore' ? ['Welcome back' + (first ? ', ' + first : ''), 'Your account is back on this ' + gate.noun + '. Your contacts, chats and call history are kept on your other devices, not with us. This ' + gate.noun + ' starts with an empty contact list.']
    : gate.flow === 'link' ? ['Linked to ' + (gate.peer || 'your other device'), 'Your contacts and chats are arriving from ' + (gate.peer || 'your other device') + ' and will keep in step on both.']
    : ['You’re all set' + (first ? ', ' + first : ''), 'Add a contact from the Contacts tab. Your account lives on this ' + gate.noun + ', protected by ' + (gate.skippedPass ? 'the ' + gate.noun + '’s own key' : 'your passphrase') + '.'],
  );
</script>

<StepShell title={copy[0]} lead={copy[1]} center>
  <div class="hero"><Mark size={96} state="taut" live label="Tinline" /></div>
  {#snippet foot()}
    <Btn onclick={() => gate.finish()}>Open Tinline</Btn>
  {/snippet}
</StepShell>

<style>
  .hero { order: -1; display: grid; place-items: center; padding-block: 28px 4px; color: var(--ink); }
</style>
