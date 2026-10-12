<script lang="ts">
  /* The full call screen: cloud field behind, avatar (plain, no ring), name, status, timer, controls.
     Text is ink-level (not muted) so the field can carry real colour in dark mode.
     Copy: no encryption claim before connection; no anonymity or IP-privacy claims. */
  import { onDestroy, onMount } from 'svelte';
  import { fade } from 'svelte/transition';
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import Mark from '../brand/Mark.svelte';
  import { statusLook } from '../brand/status.svelte';
  import CloudField from './field/CloudField.svelte';
  import WaitingCard from './WaitingCard.svelte';
  import { call, fmtDuration } from './call.svelte';
  import { overlayIn, overlayOut } from './motion';
  import { displayName, person } from '../model/store.svelte';
  import { prefs } from '../model/prefs.svelte';
  import { dur } from '../fx/motion';
  import { motion } from '../fx/reduced.svelte';
  /* video (new files under ./video): their picture, my self-view, camera notes */
  import VideoStage from './video/VideoStage.svelte';
  import SelfView from './video/SelfView.svelte';
  import VideoChip from './video/VideoChip.svelte';
  import VideoTitle from './video/VideoTitle.svelte';
  import VideoNotice from './video/VideoNotice.svelte';
  import VIcon from './video/VIcon.svelte';
  import { vid } from './video/video.svelte';

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
  /* The same mark as the app bar, so the call's state reads here too: sways while ringing, pulls taut on connect. */
  const look = $derived(statusLook());

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
  const label = $derived(`${vid.videoCall ? 'Video' : 'Voice'} call with ${name}`);
  /* video: over their picture the screen turns light-on-dark and the controls hide after a few idle seconds. */
  const onvideo = $derived(vid.stageOn);
  const idleHide = $derived(onvideo && !vid.chrome);
  const incomingVideo = $derived(phase === 'incoming' && vid.intent);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="callscreen" class:android class:lock class:onvideo class:idle={idleHide} role="dialog" aria-modal="true" aria-label={label} tabindex="-1" bind:this={root} onkeydown={(e) => { vid.poke(); key(e); }} onpointermove={() => { if (onvideo) vid.poke(); }}
  data-phase={phase} in:overlayIn out:overlayOut>
  <CloudField state={call.fieldState} anchorEl={avatarEl} scrimEls={[whoEl, ctlEl].filter((e): e is HTMLElement => !!e)} muted={call.muted} />

  <!-- video: their picture sits over the cloud field, under everything else -->
  <VideoStage />

  <div class="content">
    <header class="top" inert={idleHide}>
      {#if live}
        <button type="button" class="min press" aria-label="Minimize call" onclick={() => call.minimize()}><Icon name="chev" size={24} /></button>
      {/if}
      <VideoTitle />
      <span class="mark" title={look.label}><Mark size={28} live state={look.state} sway={look.sway} dim={look.dim} /></span>
    </header>

    <VideoChip />

    <div class="who" aria-hidden={onvideo}>
      <!-- A new person (End & answer, or a waiting call taking the slot) cross-fades; same size, stacked, no jump. -->
      <div class="av" bind:this={avatarEl}>
        {#key call.peer}<span class="swap" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}><Avatar name={name} {hue} person={call.peer ?? undefined} size={android ? 112 : 128} /></span>{/key}
      </div>
      <div class="txt" bind:this={whoEl}>
      <div class="nm">{#key call.peer}<h2 class="name" dir="auto" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>{name}</h2>{/key}</div>
      <!-- One line: the status text, or the running timer once connected (stacked, crossfaded, no gap). -->
      <div class="status">
        <span class="msg" role="status" aria-live="polite">
          {#key lock && phase === 'incoming' ? 'lock' : call.statusText}
            <span in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>{lock && phase === 'incoming' ? (vid.intent ? 'Tinline video call' : 'Tinline voice call') : call.statusText}</span>
          {/key}
        </span>
        <span class="timer" class:on={connected && !call.statusText} aria-hidden={!connected}>{fmtDuration(call.elapsedS)}</span>
      </div>
      <p class="e2e" class:on={!!sub}>{sub || ' '}</p>
      </div>
    </div>

    <WaitingCard />

    <VideoNotice />

    <div class="controls" role="group" aria-label="Call controls" bind:this={ctlEl} class:two={!android && phase !== 'incoming'} class:compact={android && phase !== 'incoming'} class:three={incomingVideo} inert={idleHide}
      onpointerenter={() => { if (onvideo) vid.hold(); }} onpointerleave={() => { if (onvideo) vid.poke(); }} onfocusin={() => { if (onvideo) vid.hold(); }}>
      {#if phase === 'incoming'}
        <div class="ctl"><button type="button" class="round end press" aria-label="Decline" onclick={() => call.decline()}><Icon name="phone" size={28} class="hang" /></button><span>Decline</span></div>
        <div class="ctl"><button type="button" class="round ans press" aria-label="Answer" onclick={() => call.answer()}><Icon name="phone" size={28} /></button><span>Answer</span></div>
        {#if incomingVideo}
          <div class="ctl"><button type="button" class="round ans press" aria-label="Answer with camera" onclick={() => call.answer(true)}><VIcon name="video" size={28} /></button><span>With camera</span></div>
        {/if}
      {:else}
        <div class="ctl">
          <button type="button" class="round tog press" class:on={call.muted} aria-pressed={call.muted} aria-label="Mute" onclick={() => call.toggleMute()}>
            {#key call.muted}<span class="ic" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}><Icon name={call.muted ? 'micoff' : 'mic'} size={26} /></span>{/key}
          </button><span>{call.muted ? 'Muted' : 'Mute'}</span>
        </div>
        <!-- video: camera on/off is the voice <-> video switch. Flip exists for phones and only while my camera is on (it glides in, the row never jumps). -->
        <div class="ctl">
          <button type="button" class="round tog press" class:on={vid.camOn} aria-pressed={vid.camOn} aria-label="Camera" disabled={phase === 'ended' || phase === 'failed' || phase === 'outgoing' || phase === 'connecting'} onclick={() => vid.toggleCamera()}>
            {#key vid.camOn}<span class="ic" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}><VIcon name={vid.camOn ? 'video' : 'videooff'} size={26} /></span>{/key}
          </button><span>Camera</span>
        </div>
        {#if android}
          <div class="ctl flip" class:open={vid.camOn} inert={!vid.camOn}>
            <button type="button" class="round tog press" aria-label="Flip camera" onclick={() => vid.flip()}><VIcon name="flip" size={26} /></button><span>Flip</span>
          </div>
          <div class="ctl">
            <button type="button" class="round tog press" class:on={call.speaker === 'speaker'} aria-pressed={call.speaker === 'speaker'} aria-label="Speaker" onclick={() => call.toggleSpeaker()}><Icon name="vol" size={26} /></button><span>Speaker</span>
          </div>
        {/if}
        <div class="ctl">
          <button type="button" class="round end press" aria-label="End call" disabled={phase === 'ended' || phase === 'failed'} onclick={() => call.hangup()}><Icon name="phone" size={28} class="hang" /></button><span>End</span>
        </div>
      {/if}
    </div>
  </div>
  {#if vid.camOn}<SelfView />{/if}
</div>

<style>
  /* On the dark field the surface-coloured buttons nearly vanish: lift them with ink. */
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme='light'])) .callscreen { --btn-bg: color-mix(in srgb, var(--ink) 14%, var(--surface)); } }
  :global(:root[data-theme='dark']) .callscreen { --btn-bg: color-mix(in srgb, var(--ink) 14%, var(--surface)); }
  .callscreen { position: absolute; inset: 0; z-index: 40; overflow: hidden; background: var(--bg); color: var(--ink); outline: none; container-type: size; }
  .content { position: relative; display: flex; flex-direction: column; block-size: 100%; padding: 8px 20px calc(36px + env(safe-area-inset-bottom)); }
  .lock .content { padding-block-start: 38px; }
  .top { position: relative; min-block-size: 48px; display: flex; align-items: center; }
  /* Centred on its own, so the minimize button coming and going never moves it. */
  .top .mark { position: absolute; inset-inline-start: 50%; inset-block-start: 50%; translate: -50% -50%; display: inline-flex; pointer-events: none; }
  :global([dir='rtl']) .top .mark { translate: 50% -50%; }
  .min { inline-size: var(--btn); block-size: var(--btn); border-radius: 50%; border: 0; background: transparent; color: var(--ink); display: grid; place-items: center; cursor: pointer; }
  .min:hover { background: color-mix(in srgb, var(--ink) 10%, transparent); }
  .who { flex: 1; min-block-size: 0; display: flex; flex-direction: column; align-items: center; justify-content: center; text-align: center; padding-block-end: 8cqb; }
  .txt { display: flex; flex-direction: column; align-items: center; max-inline-size: 100%; }
  .av { display: grid; margin-block-end: 22px; animation: avin var(--dur-overlayOpen) var(--ease-out) 40ms backwards; }
  .swap { grid-area: 1 / 1; display: grid; }
  .nm { display: grid; justify-items: center; max-inline-size: 100%; }
  .nm > .name { grid-area: 1 / 1; }
  @keyframes avin { from { scale: .96; } }
  .name { margin: 0; font-size: 28px; line-height: 34px; font-weight: 650; letter-spacing: -.01em; max-inline-size: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { display: grid; place-items: center; min-block-size: 30px; margin-block-start: 6px; font-size: 17px; line-height: 24px; font-weight: 500; }
  .status > .msg, .status > .timer { grid-area: 1 / 1; display: grid; }
  .msg > :global(span) { grid-area: 1 / 1; }
  .timer { font-size: 20px; line-height: 30px; font-weight: 500; font-variant-numeric: tabular-nums; letter-spacing: .02em; min-inline-size: 5ch; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .timer.on { opacity: 1; }
  .e2e { margin: 10px 0 0; min-block-size: 18px; font-size: var(--fs-sub); line-height: 18px; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .e2e.on { opacity: 1; }

  .controls { display: flex; justify-content: center; align-items: flex-start; gap: 36px; }
  .controls.two { gap: 56px; }
  .controls.three { gap: 24px; }
  .controls.three .ctl { inline-size: 84px; }
  /* Phone: up to five buttons (Mute, Camera, Flip, Speaker, End) fit the narrowest width; Flip's slot is zero-width until the camera is on. */
  .controls.compact { gap: 10px; }
  .compact .ctl { inline-size: 62px; }
  .compact .round { inline-size: 56px; block-size: 56px; }
  .ctl.flip { overflow: hidden; inline-size: 0; margin-inline: -10px; opacity: 0; transition: inline-size var(--dur-overlayOpen) var(--ease-out), margin var(--dur-overlayOpen) var(--ease-out), opacity var(--dur-enter) var(--ease-out); }
  .ctl.flip.open { inline-size: 62px; margin-inline: 0; opacity: 1; }
  /* Over their picture: light on dark glass whatever the theme; controls and header fade out when idle. */
  :global(:root:not([data-x])) .callscreen.onvideo { --ink: #fff; --bg: #15171c; --btn-bg: rgb(255 255 255 / .2); color: #fff; }
  .callscreen.onvideo .round { backdrop-filter: blur(12px); box-shadow: none; }
  .callscreen.onvideo .round.end { backdrop-filter: none; }
  .callscreen.onvideo .min:hover { background: rgb(255 255 255 / .14); }
  .top, .controls, .who { transition: opacity var(--dur-overlayOpen) var(--ease-out); }
  .top .mark { transition: opacity var(--dur-overlayOpen) var(--ease-out); }
  .onvideo .top .mark, .onvideo .who { opacity: 0; pointer-events: none; }
  /* The picture takes taps (show / hide the controls); only the real controls inside catch them. */
  .onvideo .content, .onvideo .controls, .onvideo .top { pointer-events: none; }
  .onvideo .content :global(button), .onvideo .content :global(.card) { pointer-events: auto; }
  .idle .top, .idle .controls { opacity: 0; pointer-events: none; }
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
