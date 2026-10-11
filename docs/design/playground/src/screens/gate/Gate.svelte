<script lang="ts">
  /* The gate shown instead of the app while db.account.gate !== 'ready'. Android: full-screen stepped flow.
     Desktop: a centred card. Same steps, same state (gate.svelte.ts); only the frame differs. */
  import { untrack, type Component } from 'svelte';
  import { db } from '../../lib/model/store.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { gate, progressOf, type Step } from '../../lib/model/actions/gate.svelte';
  import IconButton from '../../lib/ui/IconButton.svelte';
  import Mark from '../../lib/brand/Mark.svelte';
  import { stepSlide } from './slide';
  import Welcome from './Welcome.svelte';
  import Choose from './Choose.svelte';
  import Name from './Name.svelte';
  import Pass from './Pass.svelte';
  import Phrase from './Phrase.svelte';
  import Confirm from './Confirm.svelte';
  import Device from './Device.svelte';
  import Terms from './Terms.svelte';
  import Perms from './Perms.svelte';
  import Done from './Done.svelte';
  import Link from './Link.svelte';
  import Restore from './Restore.svelte';
  import Unlock from './Unlock.svelte';
  import Failed from './Failed.svelte';

  const VIEWS: Record<Step, Component> = {
    welcome: Welcome, choose: Choose, name: Name, restoreName: Name, pass: Pass, restorePass: Pass, forgotPass: Pass, linkPass: Pass, savepass: Pass,
    phrase: Phrase, confirm: Confirm, device: Device, linkName: Device, terms: Terms, perms: Perms, done: Done, link: Link,
    restore: Restore, unlock: Unlock, keylost: Failed, savefailed: Failed,
  };
  const LABEL: Partial<Record<Step, string>> = {
    name: 'Your name', pass: 'Passphrase', phrase: 'Recovery phrase', confirm: 'Check your copy', device: 'Device name', terms: 'Terms', perms: 'Permissions',
    restore: 'Restore account', restoreName: 'Your name', restorePass: 'Passphrase', linkName: 'Device name', link: 'Link to your account', linkPass: 'Passphrase',
  };
  const locked = (s: Step) => s === 'unlock' || s === 'keylost';

  $effect(() => {
    const g = db.account.gate;
    untrack(() => {
      if (g === 'locked' && !locked(gate.step) && gate.flow !== 'forgot') gate.enter('locked');
      else if (g === 'onboarding' && locked(gate.step)) gate.enter('onboarding');
    });
  });

  const prog = $derived(progressOf(gate.step, gate.flow));
  const View = $derived(VIEWS[gate.step]);
  const showHead = $derived(!!prog || gate.canBack);
</script>

<div class="gate {prefs.form}" data-gate-step={gate.step} data-flow={gate.flow}>
  <div class="card">
    <header class="head" class:empty={!showHead && prefs.form === 'android'}>
      <span class="slot">{#if gate.canBack}<IconButton icon="back" label="Back" onclick={() => gate.back()} />{/if}</span>
      {#if prefs.form === 'desktop' && gate.step !== 'welcome'}<span class="brand"><Mark size={24} /><b>Tinline</b></span>{/if}
      {#if prog}
        <span class="prog" role="progressbar" aria-label="Setup progress" aria-valuemin="1" aria-valuemax={prog[1]} aria-valuenow={prog[0]}
          aria-valuetext="Step {prog[0]} of {prog[1]}: {LABEL[gate.step] ?? ''}">
          <span class="segs" aria-hidden="true">{#each Array(prog[1]) as _, i (i)}<i class:on={i < prog[0]}></i>{/each}</span>
          <small aria-hidden="true">{prog[0]} of {prog[1]}</small>
        </span>
      {/if}
    </header>
    <div class="stage">
      {#key gate.step}
        <div class="pane" in:stepSlide={{ dir: gate.dir }} out:stepSlide={{ dir: gate.dir, out: true }}><View /></div>
      {/key}
    </div>
  </div>
</div>

<style>
  .gate { block-size: 100%; display: flex; background: var(--bg); }
  .card { display: flex; flex-direction: column; inline-size: 100%; block-size: 100%; min-block-size: 0; }
  .head { display: flex; align-items: center; gap: 12px; min-block-size: 56px; padding-inline: 8px 20px; }
  .head.empty { min-block-size: 24px; }
  .slot { inline-size: 44px; flex: none; }
  .prog { margin-inline-start: auto; display: inline-flex; align-items: center; gap: 10px; }
  .segs { display: inline-flex; gap: 4px; }
  .segs i { inline-size: 14px; block-size: 6px; border-radius: 3px; background: var(--line); transition: background-color var(--dur-switch) var(--ease-out); }
  .segs i.on { background: var(--accent); }
  .prog small { color: var(--muted); font-size: var(--fs-sub); font-variant-numeric: tabular-nums; min-inline-size: 42px; }
  .brand { display: inline-flex; align-items: center; gap: 8px; font-size: 17px; }
  .stage { flex: 1; min-block-size: 0; display: grid; overflow: hidden; }
  .pane { grid-area: 1 / 1; min-block-size: 0; min-inline-size: 0; }

  .android .prog { margin-inline-start: auto; }

  .desktop { align-items: center; justify-content: center; padding: 24px; background: var(--canvas); }
  .desktop .card { inline-size: min(600px, 100%); block-size: auto; max-block-size: 100%; background: var(--surface); border: 1px solid var(--line); border-radius: var(--r-sheet); box-shadow: var(--shadow-sheet); overflow: hidden; }
  .desktop .head { padding-block: 12px 0; padding-inline: 12px 32px; }
  .desktop .stage { min-block-size: min(560px, 70dvh); }
  .desktop .brand { margin-inline-start: 0; }
</style>
