<script lang="ts">
  /* One message row. The bubble CSS is ported verbatim from refined/chat.html (look unchanged since 3002236);
     keep text and meta contiguous in the markup: the bubble is white-space:pre-wrap. */
  import Icon from '../../lib/ui/Icon.svelte';
  import VIcon from '../../lib/calls/video/VIcon.svelte';
  import { fade, slide } from 'svelte/transition';
  import { motion } from '../../lib/fx/reduced.svelte';
  import { dur } from '../../lib/fx/motion';
  import { db, firstName, person } from '../../lib/model/store.svelte';
  import { hhmm } from '../../lib/model/clock.svelte';
  import { waitingReason } from '../../lib/model/actions/chat';
  import { SIM } from '../../lib/model/actions/chat';
  import type { Chat, Message } from '../../lib/model/types';
  import Marked from '../search/Marked.svelte';
  import type { HitRange } from '../../lib/model/actions/search';
  import { bubbleRadii, type Radii } from '../../lib/geom/shape';
  import { shape, shapeRing, cssRadii } from '../../lib/geom/action';
  import { geo, R, uniform } from '../../lib/geom/geo.svelte';

  let { c, m, pos, last, sender, fresh, held, menuOpen, flash, onflashend, popMode, quoted, onmenu, onretry, oncancel, onjump, oncall, found = false, find, findCur = false }: {
    c: Chat; m: Message; pos: 'solo' | 'first' | 'mid' | 'last'; last: boolean; sender: boolean; fresh: boolean; held: boolean; menuOpen: boolean;
    flash: boolean; onflashend: () => void; popMode: boolean; quoted: Message | undefined;
    onmenu: (m: Message, opener: HTMLElement, mode: 'pop' | 'sheet') => void;
    onretry: (m: Message) => void; oncancel: (m: Message) => void; onjump: (id: string) => void; oncall: () => void;
    /* search: opened on this message from a result (brief highlight); find-in-chat ranges (UTF-16) and whether this is the current match */
    found?: boolean; find?: HitRange[]; findCur?: boolean;
  } = $props();

  const mine = $derived(m.from === 'me');
  const fn = $derived(c.kind === 'direct' ? firstName(c.person) : firstName(m.from));
  const hueOf = (id: string) => `var(--av${(person(id)?.hue ?? 0) % 6}-ink)`;
  const isEmoji = (t: string) => !!t && /^(?:\p{Extended_Pictographic}(?:️|‍\p{Extended_Pictographic}|\p{EMod})*\s*){1,3}$/u.test(t.trim());
  const emoji = $derived(!m.deleted && !m.file && !m.call && !m.replyTo && isEmoji(m.text));
  const fmtSize = (b: number) => (b >= 1e6 ? (b / 1e6).toFixed(1) + ' MB' : Math.max(1, Math.round(b / 1e3)) + ' KB');
  const summary = $derived(m.call ? 'Call' : m.text || m.file?.name || '');

  const pending = $derived(mine && m.delivery !== 'delivered');
  const stLabel = $derived(m.delivery === 'waiting' ? waitingReason(c) : m.delivery === 'sending' ? 'Sending' : 'Delivered');

  /* call record pill */
  const call = $derived(m.call);
  const kindWord = $derived(call?.kind === 'video' ? 'video' : 'voice');
  const callText = $derived.by(() => {
    if (!call) return '';
    const dur = (s: number) => `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
    if (call.group) return `Group call · ${call.durationS < 60 ? call.durationS + ' s' : Math.round(call.durationS / 60) + ' min'}`;
    if (call.outcome === 'answered') return `${call.kind === 'video' ? 'Video' : 'Voice'} call · ${dur(call.durationS)}`;
    if (call.direction === 'in') return call.outcome === 'declined' ? `Declined ${kindWord} call` : `Missed ${kindWord} call`;
    return ({ missed: 'No answer', declined: 'Declined', cancelled: 'Cancelled call', failed: 'Call failed' } as Record<string, string>)[call.outcome] ?? 'Call';
  });
  const callBad = $derived(!!call && call.outcome !== 'answered' && (call.direction === 'in' ? call.outcome !== 'declined' : false));

  const f = $derived(m.file);
  const fstate = $derived(
    f?.state === 'sending' ? ` · ${Math.round(f.progress * 100)}%` : f?.state === 'waiting' ? ` · waiting for ${c.kind === 'direct' ? firstName(c.person) : 'a host'}` : ''
  );
  const kindLabel = $derived(f?.kind === 'image' ? 'Photo' : f?.kind === 'video' ? 'Video' : 'File');
  const quoteWho = $derived(quoted ? (quoted.from === 'me' ? 'You' : firstName(quoted.from)) : '');
  const quoteText = $derived(quoted ? (quoted.deleted ? 'Message deleted' : quoted.text || quoted.file?.name || 'Call') : 'Original message unavailable');
  const gone = $derived(!!m.deleted);
  /* Bézier outline (lib/geom): the same radii as the CSS fallback, stack-facing corners tight by position, mirrored for me/them. */
  const rtl = typeof document !== 'undefined' && document.documentElement.dir === 'rtl';
  const radii = $derived<Radii>(bubbleRadii(call ? 'solo' : pos, mine !== rtl ? 'right' : 'left', R().bub, R().tight));
  const ringRadii = (d: number): Radii => radii.map((r) => r + d) as Radii;
  const quoteRadii = uniform(R().quote);
  const note = (n: Element, o: { duration: number }) => (last ? fade(n, o) : slide(n, o));
</script>

{#if call}
  <li class="m callrec {mine ? 'me' : 'them'} g-solo" class:rise={fresh} data-mid={m.id}>
    <div class="line">
      <button type="button" class="pill" class:bad={callBad} onclick={oncall}
        aria-label="{callText}, {hhmm(m.at)}. {call.group ? 'Start or join a call in this group' : `Call ${c.kind === 'direct' ? firstName(c.person) : 'the group'} back`}">
        {#if call.kind === 'video'}<VIcon name="video" size={16} />{:else}<Icon name="phone" size={16} />{/if}<span class="t">{callText}</span><time>{hhmm(m.at)}</time>
      </button>
    </div>
  </li>
{:else}
  <li class="m {mine ? 'me' : 'them'} g-{pos}" class:rise={fresh} class:menu-open={menuOpen} class:popm={popMode} data-mid={m.id}>
    <div class="line">
      <div class="bub" class:held class:gone class:emoji class:flash class:found onanimationend={() => flash && onflashend()} style:--rad={cssRadii(radii)} style:--rad-ring={cssRadii(ringRadii(5))}>{#if !emoji}<span class="skin" aria-hidden="true" use:shape={{ radii, smoothing: geo.smooth, stroke: gone ? { width: 1, color: 'var(--hairline)' } : !mine ? { width: 1, color: 'var(--bub-in-line)' } : undefined }}></span>{/if}<svg class="ring" aria-hidden="true" use:shapeRing={{ radii: ringRadii(5), smoothing: geo.smooth }}><path class="wash" data-inset="3" /><path class="line" data-inset="2" /><path class="fl" data-inset="4" /><path class="foc" data-inset="2" /></svg>{#if sender}<span class="who" style:color={hueOf(m.from)}>{firstName(m.from)}</span>{/if}{#if gone}{mine ? 'You deleted this message' : fn + ' deleted this message'}<span class="meta"><time>{hhmm(m.at)}</time></span>{:else}{#if m.replyTo}{#if quoted}<button type="button" class="quote" use:shape={{ radii: quoteRadii, smoothing: geo.smooth }} onclick={() => onjump(quoted.id)} aria-label="Replying to {quoteWho}: {quoteText}. Jump to message"><b style:color={c.kind === 'group' && quoted.from !== 'me' ? hueOf(quoted.from) : undefined}>{quoteWho}</b><span>{quoteText}</span></button>{:else}<span class="quote" use:shape={{ radii: quoteRadii, smoothing: geo.smooth }}><span>{quoteText}</span></span>{/if}{/if}{#if f}<div><div class="file"><span class="ficon"><Icon name={f.kind === 'image' ? 'image' : 'file'} size={22} /></span><span class="fi"><span class="fn" dir="auto" title={f.name}>{#if find && !m.text}<Marked text={f.name} ranges={find} utf16 variant="mark" cur={findCur} />{:else}{f.name}{/if}</span><span class="fm">{kindLabel} · {fmtSize(f.size)}{fstate}</span></span></div>{#if f.state === 'sending' || f.state === 'waiting'}<div class="prog" role="progressbar" aria-label="Sending {f.name}" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(f.progress * 100)}><i style:width="{f.progress * 100}%" style:--tick="{SIM.tickMs}ms"></i></div><div class="facts"><button type="button" class="qbtn" data-act="cancel" aria-label="Cancel sending {f.name}" onclick={() => oncancel(m)}><Icon name="x" size={14} />Cancel</button></div>{/if}</div>{:else}{#if find && m.text}<Marked text={m.text} ranges={find} utf16 variant="mark" cur={findCur} />{:else}{m.text}{/if}{/if}<span class="meta">{#if m.edited}<span>edited</span>{/if}<time>{hhmm(m.at)}</time>{#if mine}<span class="tk" class:bad={m.delivery === 'failed'}>{#if m.delivery === 'failed'}<Icon name="warning" size={12} />{:else if pending}<Icon name="clock" size={11} class="clk" />{:else}<Icon name="check" size={12} />{/if}</span>{#if m.delivery !== 'failed'}<span class="vh">{stLabel}</span>{/if}{/if}</span>{/if}</div>
      {#if !gone}<button type="button" class="more" aria-haspopup="menu" aria-label="Actions for {mine ? 'your' : fn + '’s'} message at {hhmm(m.at)}: {summary.slice(0, 40)}"
        onclick={(e) => onmenu(m, e.currentTarget, popMode ? 'pop' : 'sheet')}><Icon name="chev" size={20} /></button>{/if}
    </div>
    <!-- Notes under my newest message are overlaid on the list's reserved bottom padding (they fade, nothing moves);
         under an older message there is a message below, so they take a row of their own. -->
    {#if mine && c.kind === 'group' && m.delivery === 'waiting' && !gone}
      <div class="wait" class:tail={last} role="status" transition:note={{ duration: motion.reduced ? 0 : dur('enter') }}><Icon name="clock" size={12} />Waiting for a host</div>
    {/if}
    {#if mine && m.delivery === 'failed' && !gone}
      <div class="fail" class:tail={last} transition:note={{ duration: motion.reduced ? 0 : dur('enter') }}><span>{f?.state === 'cancelled' ? 'Cancelled' : 'Not delivered'}</span><span aria-hidden="true">·</span>
        <button type="button" data-act="retry" aria-label={f ? 'Retry sending ' + f.name : 'Retry sending'} onclick={() => onretry(m)}>Retry</button></div>
    {/if}
  </li>
{/if}

<style>
  .m { position: relative; margin-top: 10px; }
  .m.g-mid, .m.g-last { margin-top: 2px; }
  .line { display: flex; align-items: center; gap: 4px; }
  .me .line { flex-direction: row-reverse; }
  /* The surface is a child (.skin) clipped to the Bézier outline, so the bubble itself is never clipped and the rings can sit outside it.
     --rad is the same outline as circular CSS corners: what shows until the shape is measured (first frame) or with the shape off. */
  .bub { position: relative; isolation: isolate; max-width: min(78%, 560px); min-width: 0; padding: 7px 12px; font-size: var(--fs-body); line-height: var(--lh-body); overflow-wrap: anywhere; white-space: pre-wrap;
    color: var(--bub-in-ink); border: 1px solid transparent; transition: transform var(--dur-release) var(--ease-out); }
  .me .bub { color: var(--bub-out-ink); }
  .skin { position: absolute; inset: -1px; z-index: -1; pointer-events: none; background: var(--bub-in); border-radius: var(--rad); box-shadow: inset 0 0 0 1px var(--bub-in-line); }
  .me .skin { background: var(--bub-out); box-shadow: none; }
  .skin:global([data-shaped]) { box-shadow: none; }
  .bub.held { transform: scale(.97); }
  /* Rings are an SVG around the bubble (radius + 5 px at its edge; each path is inset to its own offset so all stay concentric with the bubble):
     found = soft wash + 2 px ring 2..4 px out (faded out by Conversation after a moment), flash = 2 px ring hugging the edge, foc = keyboard focus. */
  .ring { position: absolute; inset-block-start: -6px; inset-inline-start: -6px; inline-size: calc(100% + 12px); block-size: calc(100% + 12px); overflow: visible; pointer-events: none; }
  .ring path { fill: none; opacity: 0; transition: opacity var(--dur-overlayOpen) var(--ease-out); }
  .ring .wash { fill: color-mix(in srgb, var(--accent) 16%, transparent); }
  .ring .line { stroke: color-mix(in srgb, var(--accent) 70%, transparent); stroke-width: 2; }
  .ring .fl { stroke: var(--accent); stroke-width: 2; }
  .ring .foc { stroke: var(--accent-ink); stroke-width: 2; }
  .bub.found .wash, .bub.found .line { opacity: 1; transition-duration: var(--dur-enter); }
  .bub.flash .fl { animation: flash calc(var(--dur-overlayOpen) * 5) var(--ease-out); }
  @keyframes flash { 0%, 55% { opacity: 1; } 100% { opacity: 0; } }
  .bub.gone { color: var(--muted); font-style: italic; }
  .gone .skin { background: transparent; box-shadow: inset 0 0 0 1px var(--hairline); }
  .gone .skin:global([data-shaped]) { box-shadow: none; }
  .bub.emoji { padding: 0 2px; font-size: 34px; line-height: 42px; }
  .bub.emoji .meta { float: none; display: flex; justify-content: flex-end; margin: 0; font-size: var(--fs-meta); line-height: 1; color: var(--muted); }
  .who { display: block; font-size: 13px; font-weight: 700; line-height: 18px; margin-bottom: 1px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .meta { float: right; margin: 10px -2px -2px 10px; font-size: var(--fs-meta); line-height: 1; font-variant-numeric: tabular-nums; display: inline-flex; gap: 3px; align-items: center;
    user-select: none; -webkit-user-select: none; white-space: nowrap; font-style: normal; color: var(--bub-meta-in); }
  .me .meta { color: var(--bub-meta-out); }
  .tk { display: inline-grid; place-items: center; width: 13px; height: 11px; }
  .tk.bad { color: var(--danger); }   /* a failed post keeps its tick slot (a warning mark), so the bubble does not narrow */
  .tk :global(svg) { width: 12px; height: 12px; stroke-width: 2.2; }
  .tk :global(svg.clk) { width: 11px; height: 11px; stroke-width: 2; }
  .quote { display: block; inline-size: 100%; margin: 2px 0 6px; padding: 5px 10px 6px; border-radius: var(--r-quote); background: var(--surface-2); border: 0; border-inline-start: 3px solid var(--accent);
    white-space: normal; font-size: var(--fs-sub); line-height: 18px; color: var(--ink); text-align: start; }
  button.quote { cursor: pointer; }
  button.quote:focus-visible { clip-path: none !important; border-radius: var(--r-quote) !important; }   /* the outline would be clipped by the shape */
  .quote b { display: block; font-size: 12px; font-weight: 700; color: var(--accent-ink); }
  .quote span { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; color: var(--muted); }
  .fail { display: flex; justify-content: flex-end; align-items: center; gap: 5px; margin: 3px 2px 0 0; font-size: 12px; line-height: 16px; color: var(--danger); }
  .wait { display: flex; justify-content: flex-end; align-items: center; gap: 4px; margin: 3px 2px 0 0; font-size: 12px; line-height: 16px; color: var(--gold-ink); font-weight: 600; }
  .wait.tail, .fail.tail { position: absolute; inset-block-start: 100%; inset-inline: 0; }
  .fail button { position: relative; border: 0; background: none; padding: 0; color: var(--danger); font-weight: 700; font-size: 12px; line-height: 16px; cursor: pointer; }
  .fail button::before { content: ''; position: absolute; inset: -14px -10px; }
  .fail button:hover { text-decoration: underline; text-underline-offset: 2px; }

  /* on-demand menu: a visually hidden but focusable button everywhere; a hover/focus chevron on desktop */
  .more { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0; padding: 0; margin: -1px; background: none; color: inherit; }
  .popm .more { position: relative; width: 28px; height: 28px; margin: 0; clip-path: none; overflow: visible; display: grid; place-items: center; border-radius: 50%; color: var(--muted); opacity: 0; cursor: pointer;
    transition: opacity var(--dur-switch) var(--ease-out), background-color var(--dur-switch) var(--ease-out); flex: none; }
  .popm .more::after { content: ''; position: absolute; inset: -8px; } /* 44px hit area, 28px look */
  .popm:hover .more, .popm:focus-within .more, .popm.menu-open .more { opacity: 1; }
  .popm .more:hover, .popm.menu-open .more { background: var(--surface); color: var(--ink); box-shadow: 0 0 0 1px var(--hairline); }
  .m:not(.popm):has(.more:focus-visible) .foc { opacity: 1; }
  :global([data-form='android']) .bub { -webkit-user-select: none; user-select: none; -webkit-touch-callout: none; }

  .rise { animation: rise var(--dur-bubbleIn) var(--ease-out); }
  @keyframes rise { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }

  .file { display: flex; gap: 10px; align-items: center; min-width: min(240px, 58vw); white-space: normal; margin: 2px 0; }
  .ficon { width: 40px; height: 40px; border-radius: 10px; background: color-mix(in srgb, currentColor 9%, transparent); display: grid; place-items: center; flex: none; }
  .fi { min-width: 0; flex: 1; display: grid; }
  .fn { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; line-height: 20px; }
  .fm { font-size: 12px; line-height: 16px; color: var(--bub-meta-in); font-variant-numeric: tabular-nums; }
  .me .fm { color: var(--bub-meta-out); }
  .prog { height: 4px; border-radius: 2px; background: color-mix(in srgb, currentColor 14%, transparent); margin-top: 8px; overflow: hidden; }
  .prog i { display: block; height: 100%; background: var(--accent); border-radius: 2px; transition: width var(--tick) linear; }
  .facts { display: flex; justify-content: flex-end; margin-top: 6px; white-space: normal; }
  .qbtn { position: relative; min-height: 32px; display: inline-flex; align-items: center; gap: 4px; padding: 0 12px 0 8px; border-radius: 16px; border: 0; background: color-mix(in srgb, currentColor 9%, transparent);
    color: inherit; font-size: var(--fs-sub); font-weight: 600; cursor: pointer; }
  .qbtn::after { content: ''; position: absolute; inset: -6px -2px; } /* 44px hit area */
  .qbtn:hover { background: color-mix(in srgb, currentColor 15%, transparent); }

  /* call record: a compact one-line pill on the side of whoever placed the call */
  .callrec { margin-top: 10px; }
  .callrec .line { gap: 0; }
  .pill { position: relative; display: inline-flex; align-items: center; gap: 6px; min-height: 32px; padding: 0 12px; border-radius: 16px; background: var(--surface); border: 1px solid var(--hairline);
    color: var(--muted); font-size: var(--fs-sub); line-height: 18px; cursor: pointer; max-inline-size: 78%; }
  .pill::after { content: ''; position: absolute; inset: -6px; }
  .pill:hover { background: var(--surface-2); }
  .pill .t { color: var(--ink); font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .pill.bad .t, .pill.bad :global(svg) { color: var(--danger); }
  .pill time { font-size: var(--fs-meta); font-variant-numeric: tabular-nums; }
</style>
