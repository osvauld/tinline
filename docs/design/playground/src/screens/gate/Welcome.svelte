<script lang="ts">
  import Mark from '../../lib/brand/Mark.svelte';
  import Btn from './Btn.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
</script>

<StepShell title="Call people directly." center lead="Tinline connects your {gate.noun} straight to theirs. End-to-end encrypted. No phone number, no ads, no tracking.">
  {#snippet children()}
    <div class="hero"><Mark size={112} live label="Tinline" /></div>
    <p class="warn"><b>Not for emergency calls.</b> Use your {gate.isAndroid ? 'phone’s dialer' : 'phone'} to reach emergency services.</p>
  {/snippet}
  {#snippet foot()}
    <Btn onclick={() => gate.startCreate()}>Create account</Btn>
    <Btn kind="secondary" onclick={() => gate.go('choose')}>I already have an account</Btn>
  {/snippet}
</StepShell>

<style>
  .hero { order: -1; display: grid; place-items: center; padding-block: 28px 8px; color: var(--ink); }
  .warn { margin: 0; font-size: var(--fs-sub); color: var(--muted); max-inline-size: 36ch; }
  :global(.step.center h1) { font-size: 30px; line-height: 36px; }
</style>
