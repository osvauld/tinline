<script lang="ts">
  /* Key lost (locked, no passphrase, phone key gone) and Save failed (storage refused the key). */
  import Icon from '../../lib/ui/Icon.svelte';
  import { db } from '../../lib/model/store.svelte';
  import Btn from './Btn.svelte';
  import Notice from './Notice.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  const lost = $derived(gate.step === 'keylost');
  const name = $derived(db.account.name);
</script>

{#if lost}
  <StepShell title={name ? 'Can’t open ' + name + '’s account' : 'Can’t open your account'}
    lead="This {gate.noun} no longer has the key that opens your account. That happens after a screen-lock reset or a restored backup. Once you restore, your calls come through again." center anchor>
    <div class="ic"><Icon name="key" size={32} /></div>
    <Notice kind="warn" icon="info">You didn’t set a passphrase, so the 24-word recovery phrase is the only way back. Your contacts stay.</Notice>
    {#if gate.error}<Notice kind="error">{gate.error}</Notice>{/if}
    {#snippet foot()}
      <Btn disabled={gate.busy} onclick={() => gate.forgot()}>Restore with recovery phrase</Btn>
      <Btn kind="text" busy={gate.busy} onclick={() => gate.retryKey()}>Try again</Btn>
    {/snippet}
  </StepShell>
{:else}
  <StepShell title="Couldn’t save your account"
    lead={gate.isAndroid ? 'Tinline couldn’t store the key that opens your account on this phone, so your account has not been saved yet.' : 'Tinline couldn’t store the key that opens your account on this computer, so your account has not been saved yet. The system keyring did not accept the key.'} center anchor>
    <div class="ic bad"><Icon name="warning" size={32} /></div>
    <Notice kind="info">Nothing was kept on this {gate.noun} yet, so there is nothing to lose. Try again, or protect the account with a passphrase instead of the {gate.noun}’s own key.</Notice>
    {#if gate.error}<Notice kind="error">{gate.error}</Notice>{/if}
    {#snippet foot()}
      <Btn busy={gate.busy} onclick={() => gate.retrySave()}>{gate.busy ? 'Trying…' : 'Try again'}</Btn>
      <Btn kind="text" disabled={gate.busy} onclick={() => { gate.pass = ''; gate.go('savepass'); }}>Add a passphrase instead</Btn>
    {/snippet}
  </StepShell>
{/if}

<style>
  .ic { order: -1; display: grid; place-items: center; inline-size: 72px; block-size: 72px; border-radius: 50%; background: var(--accent-soft); color: var(--accent-ink); margin-block-start: 12px; }
  .ic.bad { background: var(--danger-bg); color: var(--danger); }
</style>
