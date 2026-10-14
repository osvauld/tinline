<script lang="ts">
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import { db } from '../../lib/model/store.svelte';
  import Btn from './Btn.svelte';
  import Field from './Field.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  let pass = $state('');
  const first = $derived(db.account.name.split(' ')[0]);
  const lock = $derived(gate.lockLeft);
  const mmss = $derived(Math.floor(lock / 60) + ':' + String(lock % 60).padStart(2, '0'));
  const submit = () => { if (pass && !lock) { const p = pass; gate.unlock(p).then(() => (pass = '')); } };
</script>

<StepShell title={first ? 'Welcome back, ' + first : 'Welcome back'} center anchor form onsubmit={submit}>
  <div class="av"><Avatar name={db.account.name} hue={db.account.hue} size={72} /></div>
  <div class="fld">
    <Field label="Passphrase" secret bind:value={pass} autofocus autocomplete="current-password" disabled={gate.busy || lock > 0}
      error={lock > 0 ? null : gate.error} hint={gate.error ? 'Checking takes a second. It’s deliberately slow to stop guessing.' : undefined} reserve={4.25}>
      {#snippet extra()}{#if lock > 0}<p class="lock" role="status">Too many tries. You can try again in {mmss}.</p>{/if}{/snippet}
    </Field>
  </div>
  {#if gate.isAndroid}
    <label class="rem"><span>Remember on this phone<small>Skips the passphrase when Tinline restarts, using the phone’s secure key store.</small></span><Switch label="Remember on this phone" bind:checked={gate.remember} /></label>
  {/if}
  {#snippet foot()}
    <Btn type="submit" enter busy={gate.busy} disabled={!pass || lock > 0}>{gate.busy ? 'Unlocking…' : 'Unlock'}</Btn>
    <Btn kind="text" disabled={gate.busy} onclick={() => gate.forgot()}>I forgot it</Btn>
  {/snippet}
</StepShell>

<style>
  .av { order: -1; padding-block-start: 12px; }
  .fld { inline-size: 100%; text-align: start; display: grid; gap: 8px; }
  .lock { margin: 0; color: var(--warn-ink); background: var(--warn-bg); border-radius: var(--r-ctl); padding: 8px 14px; font-size: var(--fs-sub); font-weight: 600; font-variant-numeric: tabular-nums; }
  .rem { inline-size: 100%; display: flex; align-items: center; justify-content: space-between; gap: 16px; text-align: start; min-block-size: 56px; }
  .rem span { display: grid; gap: 2px; } .rem small { color: var(--muted); font-size: var(--fs-sub); line-height: 18px; }
</style>
