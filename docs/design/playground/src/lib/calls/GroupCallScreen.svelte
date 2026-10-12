<script lang="ts">
  /* The group call screen: the same cloud field and live mark as the 1:1 screen, the group's name, a roster of
     everyone in the call, who is speaking, who carries the call (the call host), and Mute / Leave / Speaker.
     Speaking is a soft glow and a 4 % lift behind the avatar's own edge (never a ring, per the 1:1 rule); the K loudest
     light up, everyone else stays calm. Every row that can change text has its space reserved: nothing moves when
     a word changes (status, count, speaking names, host line). Tiles that come and go glide into place. */
  import { onDestroy, onMount, tick } from 'svelte';
  import { fade } from 'svelte/transition';
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import Mark from '../brand/Mark.svelte';
  import { statusLook } from '../brand/status.svelte';
  import CloudField from './field/CloudField.svelte';
  import { gcall, speakingNames } from './gcall.svelte';
  import { fmtDuration } from './call.svelte';
  import { overlayIn, easeOut, easeIn, easeInOut, OFFSET } from './motion';
  import { prefs } from '../model/prefs.svelte';
  import { db } from '../model/store.svelte';
  import { dur, timeScale } from '../fx/motion';
  import { motion } from '../fx/reduced.svelte';
  import { plural } from '../../screens/groups/util';

  const android = $derived(prefs.form === 'android');
  const g = $derived(gcall.group);
  const name = $derived(g?.name ?? '');
  const phase = $derived(gcall.phase);
  const live = $derived(phase === 'starting' || phase === 'connecting' || phase === 'active' || phase === 'reconnecting');
  const connected = $derived(phase === 'active' || phase === 'reconnecting');
  const lc = $derived(gcall.current);
  const parts = $derived(gcall.participants);
  const hostLost = $derived(phase === 'ended' && gcall.endReason === 'host-lost');
  const fadeMs = $derived(motion.reduced ? 0 : dur('crossfade'));
  const look = $derived(statusLook());

  /* ---- roster: who is drawn, and how big ---- */
  const MAX_SHOWN = $derived(android ? 8 : 12);
  const shown = $derived(parts.length > (android ? 9 : 12) ? parts.slice(0, MAX_SHOWN) : parts);
  const more = $derived(parts.slice(shown.length));
  const moreSpeaking = $derived(more.some((p) => p.speaking));
  const tiles = $derived(shown.length + (more.length ? 1 : 0));
  const grid = $derived.by(() => {
    const n = Math.max(1, tiles);
    if (android) return n <= 2 ? { cols: n, av: 104 } : n <= 4 ? { cols: 2, av: 92 } : n <= 6 ? { cols: 3, av: 76 } : { cols: 3, av: 68 };
    return n <= 4 ? { cols: n, av: 112 } : n <= 8 ? { cols: 4, av: 96 } : { cols: 4, av: 84 };
  });
  const first = (p: { me: boolean; id: string; name: string }) => (p.me ? 'You' : p.name.split(' ')[0]);

  /* ---- text rows (each one a fixed-height stacked swap) ---- */
  const count = $derived(connected && parts.length ? (parts.length === 1 ? 'Only you so far. Waiting for others.' : `${plural(parts.length, 'person', 'people')} in the call`) : gcall.asking && phase === 'starting' ? `Asking ${gcall.asking}…` : phase === 'failed' && gcall.endReason === 'no-host' ? 'Ask an admin to appoint a call host' : hostLost ? 'The call host stopped answering' : '');
  const speakingLine = $derived(connected && phase === 'active' ? speakingNames(parts) : '');
  /* Hold the last speaking names for a moment so the line fades out calmly instead of flickering between turns. */
  let heldLine = $state('');
  let heldTimer = 0;
  $effect(() => {
    const s = speakingLine;
    if (s) { window.clearTimeout(heldTimer); heldLine = s; }
    else { window.clearTimeout(heldTimer); heldTimer = window.setTimeout(() => (heldLine = ''), 900); }
  });
  onDestroy(() => window.clearTimeout(heldTimer));
  const hostText = $derived(lc ? (lc.hostId === 'me' ? 'You’re the call host' : `Call host: ${gcall.hostLabelCap(lc.hostId)}`) : '');
  const showHost = $derived(!!lc && (connected || phase === 'connecting') );

  /* ---- host info ---- */
  let infoOpen = $state(false);
  let infoBtn = $state<HTMLElement>();
  let infoCard = $state<HTMLElement>();
  async function openInfo() { infoOpen = true; await tick(); infoCard?.focus({ preventScroll: true }); }
  async function closeInfo() { infoOpen = false; await tick(); infoBtn?.focus({ preventScroll: true }); }

  let root: HTMLElement;
  let headEl = $state<HTMLElement>();
  let hostEl = $state<HTMLElement>();
  let ctlEl = $state<HTMLElement>();
  let opener: HTMLElement | null = null;
  onMount(() => { opener = document.activeElement instanceof HTMLElement ? document.activeElement : null; root.focus({ preventScroll: true }); });
  onDestroy(() => { if (opener?.isConnected) opener.focus({ preventScroll: true }); });

  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') {
      e.preventDefault();
      if (infoOpen) closeInfo(); else if (live) gcall.minimize();
      return;
    }
    if (e.key !== 'Tab') return;
    const scope = infoOpen && infoCard ? infoCard : root;
    const items = [...scope.querySelectorAll<HTMLElement>('button:not([disabled])')];
    if (!items.length) return;
    const f = items[0], l = items[items.length - 1], a = document.activeElement;
    if (e.shiftKey && (a === f || a === root)) { e.preventDefault(); l.focus(); }
    else if (!e.shiftKey && a === l) { e.preventDefault(); f.focus(); }
  }

  /* Leaving: by minimising (up 24 px) or by ending (a short fade after the string releases). */
  function out(_n: Element) {
    const ending = gcall.phase === 'ended' || gcall.phase === 'failed' || gcall.phase === 'idle';
    if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
    if (ending) return { delay: OFFSET.endFade * timeScale(), duration: dur('callEnd') * 0.6, easing: easeOut, css: (t: number) => `opacity:${t};transform:scale(${0.98 + 0.02 * t})` };
    return { duration: dur('overlayClose'), easing: easeInOut, css: (t: number, u: number) => `opacity:${t};transform:translateY(${-24 * u}px)` };
  }
  function tileIn(_n: Element) {
    if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
    return { duration: dur('enter'), easing: easeOut, css: (t: number, u: number) => `opacity:${t};transform:scale(${1 - 0.08 * u})` };
  }
  function tileOut(_n: Element) {
    if (motion.reduced) return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
    return { duration: dur('exit'), easing: easeIn, css: (t: number, u: number) => `opacity:${t};transform:scale(${1 - 0.06 * u})` };
  }
  /* Tiles move by their centres only (svelte's flip also scales when a tile's width changes with the column count,
     which squashes the avatar for a moment). */
  function glide(_n: Element, { from, to }: { from: DOMRect; to: DOMRect }) {
    const dx = from.left + from.width / 2 - (to.left + to.width / 2), dy = from.top + from.height / 2 - (to.top + to.height / 2);
    if (!dx && !dy) return { duration: 0 };
    return { duration: motion.reduced ? 0 : dur('enter') + 60, easing: easeOut, css: (_t: number, u: number) => `transform: translate(${dx * u}px, ${dy * u}px)` };
  }
  const label = $derived(`Group call in ${name}`);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="callscreen gc" class:android role="dialog" aria-modal="true" aria-label={label} tabindex="-1" bind:this={root} onkeydown={key}
  data-gcall data-phase={phase} in:overlayIn out:out>
  <CloudField state={gcall.fieldState} anchorEl={headEl} scrimEls={[headEl, hostEl, ctlEl].filter((e): e is HTMLElement => !!e)} muted={gcall.muted} />

  <div class="content" inert={infoOpen}>
    <header class="top">
      {#if live}
        <button type="button" class="min press" aria-label="Minimize call" onclick={() => gcall.minimize()}><Icon name="chev" size={24} /></button>
      {/if}
      <span class="mark" title={look.label}><Mark size={28} live state={look.state} sway={look.sway} dim={look.dim} /></span>
    </header>

    <div class="head" bind:this={headEl}>
      <h2 class="name" dir="auto">{name}</h2>
      <div class="status">
        <span class="msg" role="status" aria-live="polite">
          {#key gcall.statusText}<span in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>{gcall.statusText}</span>{/key}
        </span>
        <span class="timer" class:on={connected && !gcall.statusText} aria-hidden={!connected}>{fmtDuration(gcall.elapsedS)}</span>
      </div>
      <p class="row cnt"><span class="stk">{#key count}<span in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>{count || ' '}</span>{/key}</span></p>
      <p class="row spk" class:on={!!heldLine} aria-hidden={!heldLine}><span>{heldLine ? `${heldLine} ${heldLine === 'You' || heldLine.includes(' and ') || heldLine.includes(',') ? 'are' : 'is'} speaking` : ' '}</span></p>
    </div>

    <div class="stage">
      <ul class="roster" class:dim={phase === 'reconnecting' || phase === 'ended' || phase === 'failed'} role="list" aria-label="People in the call" style:--cols={grid.cols} style:--av="{grid.av}px">
        {#each shown as p (p.id)}
          <li class="tile" class:speaking={p.speaking} class:muted={p.muted} style:--lv={p.level} data-part={p.id}
            aria-label="{p.me ? 'You' : p.name}{p.muted ? ', muted' : ''}{p.speaking ? ', speaking' : ''}"
            animate:glide in:tileIn out:tileOut>
            <span class="av"><span class="glow"></span><span class="pic"><Avatar name={p.me ? db.account.name : p.name} hue={p.hue} person={p.id} size={grid.av} /></span>
              {#if p.muted}<span class="mb" aria-hidden="true"><Icon name="micoff" size={Math.round(grid.av * .2) + 2} /></span>{/if}</span>
            <span class="nm" dir="auto">{first(p)}</span>
          </li>
        {/each}
        {#if more.length}
          <li class="tile more" class:speaking={moreSpeaking} aria-label="{more.length} more people{moreSpeaking ? ', someone is speaking' : ''}" in:tileIn out:tileOut data-part="more">
            <span class="av"><span class="glow"></span><span class="pic"><span class="plus" style:inline-size="{grid.av}px" style:block-size="{grid.av}px" style:font-size="{Math.round(grid.av * .3)}px">+{more.length}</span></span></span>
            <span class="nm">more</span>
          </li>
        {/if}
      </ul>

      {#if gcall.hostNote}
        <p class="hnote" role="status" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}><Icon name="battery" size={18} /><span>{gcall.hostNote}</span>
          <button type="button" class="ok press" onclick={() => gcall.muteHostNote()}>OK</button></p>
      {/if}
    </div>

    <div class="host" bind:this={hostEl} class:on={showHost}>
      <button type="button" class="hbtn press" bind:this={infoBtn} disabled={!showHost} aria-expanded={infoOpen} aria-haspopup="dialog" aria-label="{hostText || 'Call host'}. About the call host" onclick={openInfo}>
        <span class="ht">{#key hostText}<span in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>{hostText || ' '}</span>{/key}</span><Icon name="info" size={16} />
      </button>
    </div>

    <div class="controls" bind:this={ctlEl} class:two={!android}>
      {#if hostLost}
        <div class="ctl"><button type="button" class="round tog press" aria-label="Close" onclick={() => gcall.dismiss()}><Icon name="x" size={26} /></button><span>Close</span></div>
        <div class="ctl"><button type="button" class="round ans press" aria-label="Rejoin" onclick={() => gcall.rejoin()}><Icon name="phone" size={28} /></button><span>Rejoin</span></div>
      {:else}
        <div class="ctl">
          <button type="button" class="round tog press" class:on={gcall.muted} aria-pressed={gcall.muted} aria-label="Mute" onclick={() => gcall.toggleMute()}>
            {#key gcall.muted}<span class="ic" in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}><Icon name={gcall.muted ? 'micoff' : 'mic'} size={26} /></span>{/key}
          </button><span>{gcall.muted ? 'Muted' : 'Mute'}</span>
        </div>
        <div class="ctl">
          <button type="button" class="round end press" aria-label="Leave call" disabled={phase === 'ended' || phase === 'failed'} onclick={() => gcall.leave()}><Icon name="phone" size={28} class="hang" /></button><span>Leave</span>
        </div>
        {#if android}
          <div class="ctl">
            <button type="button" class="round tog press" class:on={gcall.speaker === 'speaker'} aria-pressed={gcall.speaker === 'speaker'} aria-label="Speaker" onclick={() => gcall.toggleSpeaker()}><Icon name="vol" size={26} /></button><span>Speaker</span>
          </div>
        {/if}
      {/if}
    </div>
  </div>

  {#if infoOpen && lc}
    <div class="scrim" role="presentation" onclick={closeInfo} transition:fade={{ duration: fadeMs }}></div>
    <div class="info" role="dialog" aria-modal="true" aria-label="About the call host" tabindex="-1" bind:this={infoCard} in:fade={{ duration: fadeMs }} out:fade={{ duration: fadeMs }}>
      <h3>About the call host</h3>
      <p><b>{gcall.hostLabelCap(lc.hostId)}</b> {lc.hostId === 'me' ? 'is carrying' : 'carries'} this call. Everyone in the call connects to it, and it passes the audio on and chooses who is heard when several people speak.</p>
      <p>Your connection to the call host is encrypted, but the call host itself receives the audio, so it can hear this call. You trust it for the call the way you trust a chat host with posts.</p>
      <p>A call host is a device a group admin chose and that agreed to it. It is separate from chat hosting. Admins can change call hosts in Group info.</p>
      <button type="button" class="b press" data-autofocus onclick={closeInfo}>Got it</button>
    </div>
  {/if}
</div>

<style>
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme='light'])) .callscreen { --btn-bg: color-mix(in srgb, var(--ink) 14%, var(--surface)); } }
  :global(:root[data-theme='dark']) .callscreen { --btn-bg: color-mix(in srgb, var(--ink) 14%, var(--surface)); }
  .callscreen { position: absolute; inset: 0; z-index: 40; overflow: hidden; background: var(--bg); color: var(--ink); outline: none; container-type: size; }
  .content { position: relative; display: flex; flex-direction: column; block-size: 100%; padding: 8px 20px calc(36px + env(safe-area-inset-bottom)); }
  .top { position: relative; min-block-size: 48px; display: flex; align-items: center; flex: none; }
  .top .mark { position: absolute; inset-inline-start: 50%; inset-block-start: 50%; translate: -50% -50%; display: inline-flex; pointer-events: none; }
  :global([dir='rtl']) .top .mark { translate: 50% -50%; }
  .min { inline-size: var(--btn); block-size: var(--btn); border-radius: 50%; border: 0; background: transparent; color: var(--ink); display: grid; place-items: center; cursor: pointer; }
  .min:hover { background: color-mix(in srgb, var(--ink) 10%, transparent); }

  .head { flex: none; display: flex; flex-direction: column; align-items: center; text-align: center; padding-block-start: 6px; }
  .name { margin: 0; font-size: 26px; line-height: 32px; font-weight: 650; letter-spacing: -.01em; max-inline-size: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .status { display: grid; place-items: center; min-block-size: 30px; margin-block-start: 4px; font-size: 17px; line-height: 24px; font-weight: 500; }
  .status > .msg, .status > .timer { grid-area: 1 / 1; display: grid; }
  .msg > :global(span) { grid-area: 1 / 1; }
  .timer { font-size: 20px; line-height: 30px; font-weight: 500; font-variant-numeric: tabular-nums; letter-spacing: .02em; min-inline-size: 5ch; opacity: 0; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .timer.on { opacity: 1; }
  .row { margin: 0; min-block-size: 20px; font-size: var(--fs-sub); line-height: 20px; max-inline-size: 100%; }
  .cnt { margin-block-start: 4px; font-variant-numeric: tabular-nums; }
  .stk { display: grid; justify-items: center; }
  .stk > :global(span) { grid-area: 1 / 1; }
  .spk { opacity: 0; transition: opacity var(--dur-enter) var(--ease-out); font-weight: 600; }
  .spk.on { opacity: .92; }
  .spk > span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .stage { position: relative; flex: 1; min-block-size: 0; display: flex; flex-direction: column; }
  /* Padding holds the speaking glow (it reaches 22 % past the avatar): the scroll box must not clip it square. */
  .roster { list-style: none; margin: auto; padding: 34px 28px; inline-size: 100%; max-inline-size: 760px; display: flex; flex-wrap: wrap; justify-content: center; row-gap: 20px; overflow: auto; max-block-size: 100%; transition: opacity var(--dur-enter) var(--ease-out); }
  .roster.dim { opacity: .7; }
  .tile { inline-size: calc(100% / var(--cols)); min-inline-size: 0; display: grid; justify-items: center; gap: 8px; padding-inline: 4px; }
  .av { position: relative; display: grid; place-items: center; inline-size: var(--av); block-size: var(--av); }
  /* Clip to the circle here too: while scaling, a composited photo can otherwise show its square corners. */
  .pic { display: grid; border-radius: 50%; clip-path: circle(50%); transition: scale var(--dur-enter) var(--ease-out); }
  .glow { position: absolute; inset: -22%; border-radius: 50%; pointer-events: none; opacity: 0; transition: opacity 200ms var(--ease-out);
    background: radial-gradient(closest-side, color-mix(in srgb, var(--accent) 66%, transparent) 56%, color-mix(in srgb, var(--accent) 30%, transparent) 78%, transparent 100%); }
  /* On the dark field the accent is too deep to read as light: glow with its lighter ink instead. */
  :global(:root[data-theme='dark']) .glow { background: radial-gradient(closest-side, color-mix(in srgb, var(--accent-ink) 78%, transparent) 56%, color-mix(in srgb, var(--accent-ink) 40%, transparent) 78%, transparent 100%); }
  @media (prefers-color-scheme: dark) { :global(:root:not([data-theme='light'])) .glow { background: radial-gradient(closest-side, color-mix(in srgb, var(--accent-ink) 78%, transparent) 56%, color-mix(in srgb, var(--accent-ink) 40%, transparent) 78%, transparent 100%); } }
  .speaking .glow { opacity: calc(.5 + var(--lv, .5) * .5); }
  .speaking .pic { scale: 1.04; }
  .nm { max-inline-size: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-sub); line-height: 18px; font-weight: 500; transition: font-weight var(--dur-enter); }
  .speaking .nm { font-weight: 650; }
  .mb { position: absolute; inset-inline-end: -2px; inset-block-end: -2px; display: grid; place-items: center; inline-size: calc(var(--av) * .3); block-size: calc(var(--av) * .3); min-inline-size: 22px; min-block-size: 22px;
    border-radius: 50%; background: var(--surface); color: var(--ink); box-shadow: 0 1px 2px rgb(0 0 0 / .18), 0 0 0 2px var(--bg); }
  .plus { display: grid; place-items: center; border-radius: 50%; background: color-mix(in srgb, var(--ink) 11%, var(--surface)); color: var(--ink); font-weight: 650; font-variant-numeric: tabular-nums; }

  .hnote { position: absolute; inset-inline: 0; inset-block-end: 4px; margin: 0; display: flex; align-items: center; gap: 10px; padding: 8px 8px 8px 14px; border-radius: 18px;
    background: var(--warn-bg); color: var(--warn-ink); font-size: var(--fs-sub); line-height: 18px; box-shadow: var(--shadow-pop); }
  .hnote span { flex: 1; min-inline-size: 0; }
  .ok { flex: none; min-block-size: 36px; padding-inline: 14px; border: 0; border-radius: 18px; background: color-mix(in srgb, var(--warn-ink) 14%, transparent); color: inherit; font-weight: 700; font-size: var(--fs-sub); cursor: pointer; }

  .host { flex: none; display: grid; justify-items: center; min-block-size: 44px; margin-block: 4px 12px; opacity: 0; transition: opacity var(--dur-enter) var(--ease-out); }
  .host.on { opacity: 1; }
  .hbtn { display: inline-flex; align-items: center; gap: 8px; min-block-size: 44px; padding-inline: 14px; border: 0; border-radius: 22px; background: transparent; color: inherit; font: inherit; font-size: var(--fs-sub); font-weight: 500; cursor: pointer; max-inline-size: 100%; }
  .hbtn:hover:not(:disabled) { background: color-mix(in srgb, var(--ink) 9%, transparent); }
  .hbtn:disabled { cursor: default; }
  .hbtn :global(svg) { flex: none; opacity: .75; }
  .ht { display: grid; min-inline-size: 0; }
  .ht > :global(span) { grid-area: 1 / 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  .controls { flex: none; display: flex; justify-content: center; align-items: flex-start; gap: 36px; }
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
  :global(:root[data-motion='reduce']) .pic, :global(:root[data-motion='reduce']) .glow { transition: none; }
  :global(:root[data-motion='reduce']) .speaking .pic { scale: 1; }
  .round:focus-visible, .min:focus-visible, .hbtn:focus-visible, .ok:focus-visible, .b:focus-visible { outline: 2px solid var(--ink); outline-offset: 3px; }

  .scrim { position: absolute; inset: 0; background: rgb(0 0 0 / .24); }
  .info { position: absolute; inset-inline: 16px; inset-block-end: calc(24px + env(safe-area-inset-bottom)); margin-inline: auto; max-inline-size: 420px; padding: 20px 20px 16px; border-radius: var(--r-sheet);
    background: var(--surface); color: var(--ink); box-shadow: var(--shadow-sheet); outline: none; }
  .info h3 { margin: 0 0 10px; font-size: var(--fs-title); line-height: 24px; font-weight: 650; }
  .info p { margin: 8px 0 0; font-size: var(--fs-sub); line-height: 19px; color: var(--muted); }
  .info p b { color: var(--ink); }
  .b { margin-block-start: 16px; inline-size: 100%; min-block-size: var(--btn); border: 0; border-radius: 22px; background: var(--accent); color: var(--on-accent); font-weight: 650; font-size: var(--fs-body); cursor: pointer; }
</style>
