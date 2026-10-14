<script lang="ts">
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Switch from '../../lib/ui/Switch.svelte';
  import { db } from '../../lib/model/store.svelte';
  import Btn from './Btn.svelte';
  import Field from './Field.svelte';
  import StepShell from './StepShell.svelte';
  import { gate, switchAccount } from '../../lib/model/actions/gate.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  let pass = $state('');
  // Identities have one nickname: the whole of it. With other identities here it is the switcher (lab A, 2026-10-14).
  const nick = $derived(db.account.name.trim());
  let openAs = $state(false);
  const lock = $derived(gate.lockLeft);
  const mmss = $derived(Math.floor(lock / 60) + ':' + String(lock % 60).padStart(2, '0'));
  const submit = () => { if (pass && !lock) { const p = pass; gate.unlock(p).then(() => (pass = '')); } };
</script>

<StepShell title={nick ? 'Welcome back, ' + nick : 'Welcome back'} center anchor form onsubmit={submit}>
  {#snippet heading()}
    {#if nick && db.parked.length}Welcome back, <button type="button" class="who press" aria-label="{nick}, switch identity" aria-haspopup="dialog" aria-expanded={openAs} onclick={() => (openAs = true)}>{nick}<Icon name="chev" size={20} /></button>{:else}{nick ? 'Welcome back, ' + nick : 'Welcome back'}{/if}
  {/snippet}
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

{#if openAs}
  <!-- "Open as": every identity on this device; picking another switches (nothing is open yet at login). -->
  <button type="button" class="scrim" aria-label="Close" onclick={() => (openAs = false)}></button>
  <div class="sheet" role="dialog" aria-label="Open as">
    <p class="sh">Open as</p>
    <button type="button" class="row cur press" onclick={() => (openAs = false)}><Avatar name={db.account.name} hue={db.account.hue} size={44} /><span><b>{db.account.name}</b><small>Asks for your passphrase</small></span><Icon name="check" size={18} /></button>
    {#each db.parked as a, i (a.name + i)}
      <button type="button" class="row press" onclick={() => { switchAccount(i); openAs = false; pass = ''; }}><Avatar name={a.name} hue={a.hue} size={44} /><span><b>{a.name}</b><small>Asks for your passphrase</small></span></button>
    {/each}
    <button type="button" class="row press" onclick={() => { openAs = false; gate.beginAdd(); }}><Icon name="plus" size={20} /><span><b class="plain">Add another name</b></span></button>
  </div>
{/if}

<style>
  .who { display: inline-flex; align-items: center; gap: 2px; min-block-size: 44px; padding: 0 4px; margin: -6px -4px; border: 0; border-radius: 10px; background: transparent; color: inherit; font: inherit; cursor: pointer; vertical-align: middle; }
  .who :global(svg) { color: var(--muted); }
  .scrim { position: fixed; inset: 0; z-index: 20; border: 0; background: rgb(0 0 0 / .28); }
  .sheet { position: fixed; inset-inline: 0; inset-block-end: 0; z-index: 21; display: grid; gap: 2px; padding: 12px 10px 18px; background: var(--surface-raised, var(--bg)); border-radius: var(--r-sheet, 24px) var(--r-sheet, 24px) 0 0; box-shadow: var(--shadow-sheet); max-inline-size: 520px; margin-inline: auto; animation: rise var(--dur-enter) var(--ease-out); }
  .sh { margin: 4px 12px 6px; font-size: 13px; color: var(--muted); }
  .row { display: flex; align-items: center; gap: 12px; padding: 10px 12px; min-block-size: 56px; border: 0; border-radius: 12px; background: transparent; color: var(--ink); font: inherit; text-align: start; cursor: pointer; }
  .row.cur { background: var(--surface); }
  .row span { display: grid; flex: 1; }
  .row small { color: var(--muted); font-size: 13px; }
  .plain { font-weight: 400; }
  @keyframes rise { from { transform: translateY(12px); opacity: 0; } }
  :global(:root[data-motion='reduce']) .sheet { animation: none; }
  .av { order: -1; padding-block-start: 12px; }
  .fld { inline-size: 100%; text-align: start; display: grid; gap: 8px; }
  .lock { margin: 0; color: var(--warn-ink); background: var(--warn-bg); border-radius: var(--r-ctl); padding: 8px 14px; font-size: var(--fs-sub); font-weight: 600; font-variant-numeric: tabular-nums; }
  .rem { inline-size: 100%; display: flex; align-items: center; justify-content: space-between; gap: 16px; text-align: start; min-block-size: 56px; }
  .rem span { display: grid; gap: 2px; } .rem small { color: var(--muted); font-size: var(--fs-sub); line-height: 18px; }
</style>
