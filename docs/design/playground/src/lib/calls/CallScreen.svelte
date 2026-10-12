<script lang="ts">
  /* The full call screen: cloud field behind, avatar (plain, no ring), name, status, timer, controls.
     Text is ink-level (not muted) so the field can carry real colour in dark mode.
     Copy: no encryption claim before connection; no anonymity or IP-privacy claims. */
  import { onDestroy, onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import CloudField from './field/CloudField.svelte';
  import { call, fmtDuration } from './call.svelte';
  import { overlayIn, overlayOut } from './motion';
  import { displayName, person } from '../model/store.svelte';
  import { prefs } from '../model/prefs.svelte';
  import { dur } from '../fx/motion';
  import { motion } from '../fx/reduced.svelte';

  const name = $derived(call.peer ? displayName(call.peer) : '');
  const hue = $derived((call.peer && person(call.peer)?.hue) || 0);
  const phase = $derived(call.phase);
  const live = $derived(phase === 'outgoing' || phase === 'connecting' || phase === 'active' || phase === 'reconnecting');
  /* Second line: the encryption note once live; while reconnecting or after the end, the call's length. */
  const sub = $derived(phase === 'active' ? 'End-to-end encrypted' : (phase === 'reconnecting' || phase === 'ended') && call.elapsedS > 0 ? fmtDuration(call.elapsedS) : '');
  const connected = $derived(phase === 'active' || phase === 'reconnecting' || (phase === 'ended' && call.elapsedS > 0));
  const android = $derived(prefs.form === 'android');
  /* Over the device lock screen (full-screen incoming): no minimize, the OS status bar sits above. */
  let { lock = false }: { lock?: boolean } = $props();
  const fadeMs = $derived(motion.reduced ? 0 : dur('crossfade'));

  let root: HTMLElement;
  let avatarEl = $state<HTMLElement>();
  let whoEl = $state<HTMLElement>();
  let ctlEl = $state<HTMLElement>();
  let opener: HTMLElement | null = null;

  onMount(() => {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    root.focus({ preventScroll: true });
  });
  onDestroy(() => {
    // Focus returns to where the call was opened from (if it is still on screen).
    if (opener?.isConnected) opener.focus({ preventScroll: true });
  });

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape' && live) { e.preventDefault(); call.minimize(); return; }
    if (e.key !== 'Tab') return;
    const items = [...root.querySelectorAll<HTMLElement>('button:not([disabled])')];
    if (!items.length) return;
    const first = items[0], last = items[items.length - 1], a = document.activeElement;
    if (e.shiftKey && (a === first || a === root)) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && a === last) { e.preventDefault(); first.focus(); }
  }
  const label = $derived(`Voice call with ${name}`);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="callscreen" class:android class:lock role="dialog" aria-modal="true" aria-label={label} tabindex="-1" bind:this={root} onkeydown={key}
  data-phase={phase} in:overlayIn out:overlayOut>
  <CloudField state={call.fieldState} anchorEl={avatarEl} scrimEls={[whoEl, ctlEl].filter((e): e is HTMLElement => !!e)} muted={call.muted} />

  <div class="content">
    <header class="top">
      {#if live}
        <button type="button" class="min press" aria-label="Minimize call" onclick={() => call.minimize()}><Icon name="chev" size={24} /></button>
      {/if}
    </header>

    <div class="who">
      <div class="av" bind:this={avatarEl}><Avatar name={name} {hue} size={android ? 112 : 128} /></div>
      <div class="txt" bind:this={whoEl}>
      <h2 class="name" dir="auto">{name}</h2>
      <!-- One line: the status text, or the running timer once connected (stacked, crossfaded, no gap). -->
      <div class="status">
        <span class="msg" role="status" aria-live="polite">
          {#key lock && phase === 'incoming' ? 'lock' : call.statusText}
            <span in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>{lock && phase === 'incoming' ? 'Tinline voice call' : call.statusText}</span>
          {/key}
        </span>
        <span class="timer" class:on={connected && !call.statusText} aria-hidden={!connected}>{fmtDuration(call.elapsedS)}</span>
      </div>
      <p class="e2e" class:on={!!sub}>{sub || ' '}</p>
      </div>
    </div>

    <div class="controls" bind:this={ctlEl} class:two={!android && phase !== 'incoming'}>
      {#if phase === 'incoming'}
        <div class="ctl"><button type="button" class="round end press" aria-label="Decline" onclick={() => call.decline()}><Icon name="phone" size={28} class="hang" /></button><span>Decline</span></div>
        <div class="ctl"><button type="button" class="round ans press" aria-label="Answer" onclick={() => call.answer()}><Icon name="phone" size={28} /></button><span>Answer</span></div>
      {:else}
        <div class="ctl">
          <button type="button" class="round tog press" class:on={call.muted} aria-pressed={call.muted} aria-label="Mute" onclick={() => call.toggleMute()}>
            {#key call.muted}<span class="ic" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}><Icon name={call.muted ? 'micoff' : 'mic'} size={26} /></span>{/key}
          </button><span>{call.muted ? 'Muted' : 'Mute'}</span>
        </div>
        <div class="ctl">
          <button type="button" class="round end press" aria-label="End call" disabled={phase === 'ended' || phase === 'failed'} onclick={() => call.hangup()}><Icon name="phone" size={28} class="hang" /></button><span>End</span>
        </div>
        {#if android}
          <div class="ctl">
            <button type="button" class="round tog press" class:on={call.speaker === 'speaker'} aria-pressed={call.speaker === 'speaker'} aria-label="Speaker" onclick={() => call.toggleSpeaker()}><Icon name="vol" size={26} /></button><span>Speaker</span>
          </div>
        {/if}
      {/if}
    </div>
  </div>
</div>

<style>
  /* On the dark field the surface-coloured buttons nearly vanish: lift them with ink. */
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme='light'])) .callscreen { --btn-bg: color-mix(in srgb, var(--ink) 14%, var(--surface)); } }
  :global(:root[data-theme='dark']) .callscreen { --btn-bg: color-mix(in srgb, var(--ink) 14%, var(--surface)); }
  .callscreen { position: absolute; inset: 0; z-index: 40; overflow: hidden; background: var(--bg); color: var(--ink); outline: none; container-type: size; }
  .content { position: relative; display: flex; flex-direction: column; block-size: 100%; padding: 8px 20px calc(36px + env(safe-area-inset-bottom)); }
  .lock .content { padding-block-start: 38px; }
  .top { min-block-size: 48px; display: flex; align-items: center; }
  .min { inline-size: var(--btn); block-size: var(--btn); border-radius: 50%; border: 0; background: transparent; color: var(--ink); display: grid; place-items: center; cursor: pointer; }
  .min:hover { background: color-mix(in srgb, var(--ink) 10%, transparent); }
  .who { flex: 1; min-block-size: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; padding-block-end: 8cqb; }
  .txt { display: flex; flex-direction: column; align-items: center; max-inline-size: 100%; }
  .av { display: grid; margin-block-end: 22px; animation: avin var(--dur-overlayOpen) var(--ease-out) 40ms backwards; }
  @keyframes avin { from { scale: .96; } }
  .name { margin: 0; font-size: 28px; line-height: 34px; font-weight: 650; letter-spacing: -.01em; max-inline-size: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { display: grid; place-items: center; min-block-size: 30px; margin-block-start: 6px; font-size: 17px; line-height: 24px; font-weight: 500; }
  .status > .msg, .status > .timer { grid-area: 1 / 1; display: grid; }
  .msg > :global(span) { grid-area: 1 / 1; }
  .timer { font-size: 20px; line-height: 30px; font-weight: 500; font-variant-numeric: tabular-nums; letter-spacing: .02em; min-inline-size: 5ch; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .timer.on { opacity: 1; }
  .e2e { margin: 10px 0 0; font-size: var(--fs-sub); line-height: 18px; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .e2e.on { opacity: 1; }

  .controls { display: flex; justify-content: center; align-items: flex-start; gap: 36px; }
  .controls.two { gap: 56px; }
  .ctl { display: grid; justify-items: center; gap: 8px; inline-size: 76px; font-size: var(--fs-sub); line-height: 18px; font-weight: 500; }
  .round { inline-size: 64px; block-size: 64px; border-radius: 50%; border: 0; display: grid; place-items: center; cursor: pointer; padding: 0;
    background: var(--btn-bg, var(--surface)); color: var(--ink); box-shadow: 0 1px 2px rgb(0 0 0 / .12), 0 4px 14px rgb(0 0 0 / .10);
    transition: transform var(--dur-release) var(--ease-out), background-color var(--dur-switch) var(--ease-out), color var(--dur-switch) var(--ease-out); }
  .round:hover { background: color-mix(in srgb, var(--btn-bg, var(--surface)) 90%, var(--ink)); }
  .round.tog.on { background: var(--ink); color: var(--bg); }
  .round.end { background: var(--danger-btn); color: var(--on-danger-btn); }
  .round.end:hover { background: var(--danger-btn); filter: brightness(1.08); }
  .round.end:not(:disabled):active { transform: scale(.93); }
  .round.ans { background: var(--accent); color: var(--on-accent); }
  .round.ans:hover { background: var(--accent); filter: brightness(1.1); }
  .round:disabled { opacity: .6; cursor: default; }
  .round.tog { grid-template-areas: 'x'; }
  .ic { grid-area: x; display: grid; place-items: center; }
  .round :global(.hang) { transform: rotate(135deg); }
  :global(:root[data-motion='reduce']) .round.end:active { transform: none; }
  .round:focus-visible, .min:focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }
</style>
