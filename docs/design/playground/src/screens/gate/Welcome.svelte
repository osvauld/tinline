<script lang="ts">
  /* Step 0 of sign-up, laid out like every other step: title and lead top-start, the mark filling the free
     space, actions in the footer. "Create my account" sits exactly where Continue sits on the next step. */
  import Mark from '../../lib/brand/Mark.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
</script>

<StepShell title={gate.adding ? 'Add an account' : 'A direct line to your people'}
  lead={gate.adding ? ['Create a new account or bring one you already have.', 'Your current account stays on this ' + gate.noun + '.'] : ['Tinline connects your ' + gate.noun + ' straight to theirs.', 'You just need a nickname. Encrypted end to end.']}>
  {#snippet children()}
    <!-- The line is slack here; on the last step it pulls taut ("Your line is open"). -->
    <div class="hero" data-welcome><Mark size={120} live lineColor label="Tinline" /></div>
    <p class="warn"><b>Not for emergency calls.</b> <span>Use your {gate.isAndroid ? 'phone’s dialer' : 'phone'} to reach emergency services.</span></p>
  {/snippet}
  {#snippet foot()}
    <Btn enter onclick={() => gate.startCreate()}>Create my account</Btn>
    <Btn kind="secondary" onclick={() => gate.go('choose')}>I have an account</Btn>
    {#if gate.adding}<Btn kind="text" onclick={() => gate.cancelAdd()}>Cancel</Btn>{/if}
  {/snippet}
</StepShell>

<style>
  .hero { flex: 1; min-block-size: 120px; display: grid; place-items: center; color: var(--ink); }
  .warn { margin: 0; font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .warn b, .warn span { display: block; }
  /* Desktop footer is the shared one: right-aligned, "Create my account" exactly where Continue sits next. */
</style>
