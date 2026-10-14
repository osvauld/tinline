<script lang="ts">
  import Btn from './Btn.svelte';
  import Field from './Field.svelte';
  import Notice from './Notice.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';

  const kind = $derived(gate.step === 'forgotPass' ? 'forgot' : gate.step === 'linkPass' ? 'link' : gate.step === 'savepass' ? 'save' : gate.step === 'restorePass' ? 'restore' : 'create');
  const title = $derived({ forgot: 'Choose a new passphrase', link: 'Protect this ' + gate.noun, save: 'Add a passphrase', restore: 'Add a passphrase', create: 'Add a passphrase' }[kind]);
  const cta = $derived({ forgot: 'Restore account', link: 'Add passphrase', save: 'Save account', restore: 'Restore account', create: 'Continue' }[kind]);
  const canSkip = $derived(kind !== 'save');
  const protectLine = $derived(gate.isAndroid
    ? 'Encrypts your account on this phone. Your phone remembers it, so calls still ring after a restart.'
    : 'Encrypts your account on this computer. You type it each time Tinline starts.');

  const strength = $derived.by(() => {
    const p = gate.pass; if (!p) return 0;
    const words = p.trim().split(/\s+/).length;
    return p.length >= 16 || words >= 4 ? 3 : p.length >= 8 ? 2 : 1;
  });
  const says = $derived(['Any length works. A few random words are easy to remember.', 'Short. A few more words make it stronger.', 'Fair', 'Strong']);
  const cur = $derived(gate.pass ? strength : 0);
  const submit = () => {
    if (!gate.pass) return;
    if (kind === 'save') gate.savePassAlt(); else gate.submitPass(false);
  };
</script>

<StepShell {title} lead="Optional. A lock, so only you can open Tinline on this {gate.noun}." form onsubmit={submit}>
  <div class="col">
    <Field label="Passphrase" secret bind:value={gate.pass} autofocus autocomplete="new-password" disabled={gate.busy} describedBy="strength" />
    <div class="meter" id="strength" aria-live="polite">
      <span class="bars" aria-hidden="true">{#each [1, 2, 3] as i (i)}<i class:on={strength >= i} data-s={strength}></i>{/each}</span>
      <!-- Every line shares one cell (the tallest sets the height), so the hint turning into a rating never moves the note below. -->
      <span class="says">{#each says as t, i (i)}<small class:cur={i === cur}>{t}</small>{/each}</span>
    </div>
  </div>
  <Notice icon="lock">{protectLine} If you forget it, your recovery words let you set a new one.</Notice>
  {#if gate.error}<Notice kind="error">{gate.error}</Notice>{/if}
  {#snippet foot()}
    <Btn type="submit" busy={gate.busy} disabled={!gate.pass} next={kind === 'create'} enter label={gate.busy ? 'Encrypting…' : undefined}>{gate.busy ? 'Encrypting…' : cta}</Btn>
    {#if canSkip}<Btn kind="text" disabled={gate.busy} onclick={() => gate.submitPass(true)}>Skip for now</Btn>{/if}
  {/snippet}
</StepShell>

<style>
  .col { display: grid; gap: 12px; }
  .meter { display: flex; align-items: center; gap: 10px; margin-block-start: -6px; }
  .bars { display: inline-flex; gap: 4px; }
  .bars i { inline-size: 28px; block-size: 6px; border-radius: 3px; background: var(--line); transition: background-color var(--dur-switch) var(--ease-out); }
  .bars i.on[data-s='1'] { background: var(--danger); }
  .bars i.on[data-s='2'] { background: var(--gold); }
  .bars i.on[data-s='3'] { background: var(--accent); }
  .meter { align-items: flex-start; }
  .bars { margin-block-start: 6px; }
  .says { display: grid; flex: 1; min-inline-size: 0; }
  .says > small { grid-area: 1 / 1; visibility: hidden; }
  .says > small.cur { visibility: visible; }
  small { color: var(--muted); font-size: var(--fs-sub); line-height: 18px; }
</style>
