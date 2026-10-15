<script lang="ts">
  /* A call ringing while the app is open: the header's brand area turns into the caller, in place. Nothing is added to the
     layout and nothing moves; this is an overlay with the exact box of the header content it covers, cross-faded.
     Android home: the avatar takes the mark's place, name over "Incoming call", Decline/Answer take the icons' places.
     Android screens with their own header: the back button stays, everything right of it swaps.
     Desktop: the sidebar brand row. Tapping the caller opens the full incoming screen.
     Lock screen, heads-up, shade and desktop toast (SystemLayer) are outside the app and unchanged. */
  import Avatar from '../ui/Avatar.svelte';
  import Icon from '../ui/Icon.svelte';
  import { call } from './call.svelte';
  import { system } from './system.svelte';
  import { displayName, person } from '../model/store.svelte';
  import { prefs } from '../model/prefs.svelte';
  import { vid } from './video/video.svelte';
  import { dur } from '../fx/motion';
  import { motion } from '../fx/reduced.svelte';

  const waiting = $derived(call.waitingNow && call.view === 'strip' && !system.away ? call.waitingNow : null);
  const show = $derived((call.phase === 'incoming' && call.view === 'strip' && !system.away) || !!waiting);
  const id = $derived(waiting ? waiting.peer : call.peer);
  const name = $derived(id ? displayName(id) : '');
  const hue = $derived((id && person(id)?.hue) || 0);
  const sub = $derived(waiting ? 'Calling during your call' : (vid.intent ? 'Incoming video call' : 'Incoming call') + (call.toName ? ` to ${call.toName}` : ''));
  const desktop = $derived(prefs.form === 'desktop');

  function fade(_n: Element) {
    if (motion.reduced) return { duration: 0 };
    return { duration: dur('crossfade'), css: (t: number) => `opacity:${t}` };
  }

  /* The box of the header being covered, measured every frame while ringing (screens slide in; nothing is assumed). */
  let host = $state<HTMLElement>();
  let box = $state<{ x: number; y: number; w: number; h: number; pl: number; pr: number; home: boolean } | null>(null);
  const FIND = '.screen .bar, .screen .pf-bar, .screen .appbar, .vp > .appbar';
  $effect(() => {
    if (!show || !host) { box = null; return; }
    const h = host;
    let raf = 0, inerted: HTMLElement | null = null;
    const tick = () => {
      const parent = h.parentElement!, pr = parent.getBoundingClientRect();
      const t = document.querySelector<HTMLElement>(desktop ? '.side .brand' : FIND);
      if (t) {
        const r = t.getBoundingClientRect(), cs = getComputedStyle(t);
        const pl = parseFloat(cs.paddingLeft), prr = parseFloat(cs.paddingRight);
        const home = !!t.closest('.vp') && !t.closest('.screen');
        const back = !desktop && !home ? pl + 44 : 0;   // the back button stays where it is
        const trim = desktop ? parseFloat(cs.paddingBottom) : 0;   // the sidebar brand row carries spacing below its content
        const n = { x: r.left - pr.left + back, y: r.top - pr.top, w: r.width - back, h: r.height - trim, pl: back ? 4 : pl, pr: prr, home };
        if (!box || box.x !== n.x || box.y !== n.y || box.w !== n.w || box.h !== n.h) box = n;
        if ((home || desktop) && inerted !== t) { inerted?.removeAttribute('inert'); t.setAttribute('inert', ''); inerted = t; }
      }
      raf = requestAnimationFrame(tick);
    };
    tick();
    return () => { cancelAnimationFrame(raf); inerted?.removeAttribute('inert'); };
  });
</script>

<div bind:this={host} class="host" aria-hidden="true"></div>
{#if show && box}
  <div class="rh {prefs.form}" class:own={!box.home && !desktop} class:waiting role="alert" aria-label={waiting ? `${name} is calling during your call` : `${sub} from ${name}`}
    style:left="{box.x}px" style:top="{box.y}px" style:width="{box.w}px" style:height="{box.h}px" style:padding-inline="{box.pl}px {box.pr}px"
    in:fade out:fade data-incoming-alert>
    <button type="button" class="open" aria-label="Open call from {name}" onclick={() => call.restore()}>
      <Avatar {name} {hue} person={id ?? undefined} size={desktop ? 28 : box.home ? 28 : 36} />
      <span class="tx"><b dir="auto">{name}</b><small>{sub}</small></span>
    </button>
    <span class="acts">
      {#if waiting}
        <button type="button" class="rb dec press" aria-label="Decline" title="Decline" onclick={() => call.declineWaiting()}><span><Icon name="phone" size={desktop ? 16 : 22} class="hang" /></span></button>
        <button type="button" class="rb ans press" aria-label="End and answer" title="End and answer" onclick={() => call.answerWaiting()}><span><Icon name="phone" size={desktop ? 16 : 22} /></span></button>
      {:else}
        <button type="button" class="rb dec press" aria-label="Decline" title="Decline" onclick={() => call.decline()}><span><Icon name="phone" size={desktop ? 16 : 22} class="hang" /></span></button>
        <button type="button" class="rb ans press" aria-label="Answer" title="Answer" onclick={() => call.answer()}><span><Icon name="phone" size={desktop ? 16 : 22} /></span></button>
      {/if}
    </span>
  </div>
{/if}

<style>
  .host { display: none; }
  .rh { position: absolute; z-index: 45; display: flex; align-items: center; box-sizing: border-box; background: var(--bg); color: var(--ink); }
  .rh.own { background: var(--surface); border-block-end: 1px solid var(--hairline); }
  .rh.android:not(.own) { padding-block-start: 4px; }   /* the app bar's own top padding: same centre line as the mark */
  .open { flex: 1; min-inline-size: 0; align-self: stretch; display: flex; align-items: center; gap: 8px; padding: 0; border: 0; background: transparent; color: inherit; cursor: pointer; text-align: start; font: inherit; border-radius: 14px; }
  .tx { display: grid; min-inline-size: 0; }
  .tx b { font-size: 15px; font-weight: 650; line-height: 19px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .tx small { font-size: 12px; line-height: 15px; color: var(--muted); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .desktop .tx b { font-size: 13px; line-height: 16px; }
  .desktop .tx small { font-size: 11px; line-height: 14px; }
  .acts { display: flex; flex: none; }
  /* Same footprint as the 44 px icon buttons they replace; the filled circle sits inside it. */
  .rb { inline-size: 44px; block-size: 44px; display: grid; place-items: center; padding: 0; border: 0; background: transparent; cursor: pointer; flex: none; }
  .rb span { inline-size: 38px; block-size: 38px; border-radius: 50%; display: grid; place-items: center; }
  .desktop .rb { inline-size: 32px; block-size: 32px; }
  .desktop .rb span { inline-size: 28px; block-size: 28px; }
  .dec span { background: var(--danger-btn); color: var(--on-danger-btn); }
  .ans span { background: var(--accent); color: var(--on-accent); }
  .rb :global(.hang) { transform: rotate(135deg); }
  .open:focus-visible, .rb:focus-visible { outline: 2px solid var(--ink); outline-offset: 2px; }
</style>
