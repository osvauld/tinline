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
  import ChatGate from './ChatGate.svelte';

  const VIEWS: Record<Step, Component> = {
    welcome: Welcome, choose: Choose, name: Name, restoreName: Name, pass: Pass, restorePass: Pass, forgotPass: Pass, linkPass: Pass, savepass: Pass,
    phrase: Phrase, confirm: Confirm, device: Device, linkName: Device, terms: Terms, perms: Perms, done: Done, link: Link,
    restore: Restore, unlock: Unlock, keylost: Failed, savefailed: Failed,
  };
  const LABEL: Partial<Record<Step, string>> = {
    name: 'Your nickname', pass: 'Passphrase', phrase: 'Recovery phrase', confirm: 'Check your copy', device: 'Device name', terms: 'Terms', perms: 'Permissions',
    restore: 'Restore account', restoreName: 'Your nickname', restorePass: 'Passphrase', linkName: 'Device name', link: 'Link to your account', linkPass: 'Passphrase',
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
  /* Effective theme ('system' resolved), flipped to an explicit light/dark. App: the same Appearance setting as Settings. */
  const sysDark = matchMedia('(prefers-color-scheme: dark)');
  let sys = $state(sysDark.matches);
  sysDark.addEventListener('change', (e) => (sys = e.matches));
  const dark = $derived(prefs.theme === 'dark' || (prefs.theme === 'system' && sys));
  const flipTheme = () => (prefs.theme = dark ? 'light' : 'dark');
  const View = $derived(VIEWS[gate.step]);
  const showHead = $derived(!!prog || gate.canBack);

  /* Android: a text field in focus brings up the soft keyboard and the window resizes above it (adjustResize).
     Mocked at 272px like the search screen's, so every step is judged with the keyboard up too. */
  let kbd = $state(false);
  /* Enter continues on every step: inputs submit their form natively; anywhere else (heading, page) Enter clicks the
     step's [data-enter] button. A focused button or link keeps its own Enter. */
  function onEnter(e: KeyboardEvent) {
    if (e.key !== 'Enter' || e.defaultPrevented || e.shiftKey || e.ctrlKey || e.altKey || e.metaKey || e.isComposing) return;
    const t = e.target as HTMLElement;
    if (t.closest('input, textarea, select, button, a, [contenteditable]')) return;
    const panes = document.querySelectorAll<HTMLElement>('.gate .pane');
    const b = panes[panes.length - 1]?.querySelector<HTMLButtonElement>('[data-enter]');
    if (b && !b.disabled) { e.preventDefault(); b.click(); }
  }
  const isText = (el: EventTarget | null) => el instanceof HTMLInputElement && (el.type === 'text' || el.type === 'password');
  /* Prototype (Proposed): ?gate=chat shows the create flow as a conversation (ChatGate). Other flows keep the steps. */
  /* Trial (Proposed): ?align=center centres every step's title, lead and short notes, to compare with start-aligned. */
  const alignCenter = new URLSearchParams(location.search).get('align') === 'center';
  const chatMode = new URLSearchParams(location.search).get('gate') === 'chat';
  const useChat = $derived(chatMode && gate.flow === 'create' && !['unlock', 'keylost', 'savefailed', 'savepass'].includes(gate.step));
</script>

<div class="gate {prefs.form}" data-align={alignCenter ? 'center' : undefined} data-gate-step={gate.step} data-flow={gate.flow}
  onfocusin={(e) => (kbd = prefs.form === 'android' && isText(e.target))} onfocusout={() => (kbd = false)} onkeydown={onEnter}>
  <div class="card">
    <header class="head" class:empty={!showHead && prefs.form === 'android'}>
      <span class="slot">{#if gate.canBack}<IconButton icon="back" label="Back" onclick={() => gate.back()} />{/if}</span>
      {#if prefs.form === 'desktop'}<span class="brand"><Mark size={24} /><b>Tinline</b></span>{/if}
      <span class="end">
      {#if prog}
        <span class="prog" role="progressbar" aria-label="Setup progress" aria-valuemin="0" aria-valuemax={prog[1]} aria-valuenow={prog[0]}
          aria-valuetext="Step {prog[0]} of {prog[1]}: {LABEL[gate.step] ?? ''}">
          <span class="segs" aria-hidden="true">{#each Array(prog[1]) as _, i (i)}<i class:on={i < prog[0]}></i>{/each}</span>
          <small aria-hidden="true" class:off={!prog[0]}>{prog[0]} of {prog[1]}</small>
        </span>
      {/if}
        <!-- Light/dark right in sign-up: the first screens are where someone decides if this feels like theirs. -->
        <IconButton icon={dark ? 'sun' : 'moon'} label={dark ? 'Switch to light' : 'Switch to dark'} size={40} iconSize={20} class="theme" onclick={flipTheme} />
      </span>
    </header>
    {#if useChat}
      <ChatGate />
    {:else}
    <div class="stage">
      {#key gate.step}
        <div class="pane" in:stepSlide={{ dir: gate.dir }} out:stepSlide={{ dir: gate.dir, out: true }}><View /></div>
      {/key}
    </div>
    {/if}
  </div>
  {#if kbd}<div class="kbd" aria-hidden="true"><span>Soft keyboard (mock)</span></div>{/if}
</div>

<style>
  .gate { block-size: 100%; display: flex; background: var(--bg); }
  .card { display: flex; flex-direction: column; inline-size: 100%; block-size: 100%; min-block-size: 0; }
  .head { display: flex; align-items: center; gap: 12px; min-block-size: 56px; padding-inline: 8px 20px; }
  .head.empty { min-block-size: 24px; }
  .slot { inline-size: 44px; flex: none; }
  .end { margin-inline-start: auto; display: inline-flex; align-items: center; gap: 8px; }
  .end :global(.theme) { color: var(--muted); margin-inline-end: -10px; }   /* icon's optical edge lines up with the card padding */
  .prog { display: inline-flex; align-items: center; gap: 10px; }
  .segs { display: inline-flex; gap: 4px; }
  .segs i { inline-size: 14px; block-size: 6px; border-radius: 3px; background: var(--line); transition: background-color var(--dur-switch) var(--ease-out); }
  .segs i.on { background: var(--accent); }
  .prog small { color: var(--muted); font-size: var(--fs-sub); font-variant-numeric: tabular-nums; min-inline-size: 42px; }
  .prog small.off { visibility: hidden; }   /* step 0 (welcome): the count's space is kept, so "1 of 6" appears in place */
  .brand { display: inline-flex; align-items: center; gap: 8px; font-size: 17px; }
  .stage { flex: 1; min-block-size: 0; display: grid; overflow: hidden; }
  .pane { grid-area: 1 / 1; min-block-size: 0; min-inline-size: 0; }

  .android { flex-direction: column; }
  .android .card { flex: 1; block-size: auto; }
  /* Depth: the shadow token carries the dark rim, as everywhere (no underglow, decided 2026-10-13). */
  .desktop .card { box-shadow: var(--shadow-sheet); }
  .kbd { flex: none; block-size: 272px; display: grid; place-items: center; background: var(--surface-2); color: var(--muted); font-size: var(--fs-sub); border-block-start: 1px solid var(--hairline); }

  .desktop { align-items: center; justify-content: center; padding: 24px; background: var(--canvas); }
  /* Fixed height: every step is the same card, so moving between steps never resizes or re-centres it; a long step scrolls inside. */
  .desktop .card { inline-size: min(600px, 100%); block-size: min(640px, 100%); background: var(--surface-raised); border: 1px solid var(--line); border-radius: var(--r-sheet); overflow: hidden; }
  .desktop .head { padding-block: 12px 0; padding-inline: 12px 32px; }
  .desktop .head.empty { min-block-size: 0; padding: 0; }
  .desktop .stage { min-block-size: 0; }
  .desktop .brand { margin-inline-start: 0; }
</style>
