<script lang="ts">
  /* One message row. The bubble CSS is ported verbatim from refined/chat.html (look unchanged since 3002236);
     keep text and meta contiguous in the markup: the bubble is white-space:pre-wrap. */
  import Icon from '../../lib/ui/Icon.svelte';
  import { db, firstName, person } from '../../lib/model/store.svelte';
  import { hhmm } from '../../lib/model/clock.svelte';
  import { waitingReason } from '../../lib/model/actions/chat';
  import { SIM } from '../../lib/model/actions/chat';
  import type { Chat, Message } from '../../lib/model/types';

  let { c, m, pos, sender, fresh, held, menuOpen, flash, onflashend, popMode, quoted, onmenu, onretry, oncancel, onjump, oncall }: {
    c: Chat; m: Message; pos: 'solo' | 'first' | 'mid' | 'last'; sender: boolean; fresh: boolean; held: boolean; menuOpen: boolean;
    flash: boolean; onflashend: () => void; popMode: boolean; quoted: Message | undefined;
    onmenu: (m: Message, opener: HTMLElement, mode: 'pop' | 'sheet') => void;
    onretry: (m: Message) => void; oncancel: (m: Message) => void; onjump: (id: string) => void; oncall: () => void;
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
</script>

{#if call}
  <li class="m callrec {mine ? 'me' : 'them'} g-solo" class:rise={fresh} data-mid={m.id}>
    <div class="line">
      <button type="button" class="pill" class:bad={callBad} onclick={oncall}
        aria-label="{callText}, {hhmm(m.at)}. Call {c.kind === 'direct' ? firstName(c.person) : 'the group'} back">
        <Icon name="phone" size={16} /><span class="t">{callText}</span><time>{hhmm(m.at)}</time>
      </button>
    </div>
  </li>
{:else}
  <li class="m {mine ? 'me' : 'them'} g-{pos}" class:rise={fresh} class:menu-open={menuOpen} class:popm={popMode} data-mid={m.id}>
    <div class="line">
      <div class="bub" class:held class:gone class:emoji class:flash onanimationend={() => flash && onflashend()}>{#if sender}<span class="who" style:color={hueOf(m.from)}>{firstName(m.from)}</span>{/if}{#if gone}{mine ? 'You deleted this message' : fn + ' deleted this message'}<span class="meta"><time>{hhmm(m.at)}</time></span>{:else}{#if m.replyTo}{#if quoted}<button type="button" class="quote" onclick={() => onjump(quoted.id)} aria-label="Replying to {quoteWho}: {quoteText}. Jump to message"><b style:color={c.kind === 'group' && quoted.from !== 'me' ? hueOf(quoted.from) : undefined}>{quoteWho}</b><span>{quoteText}</span></button>{:else}<span class="quote"><span>{quoteText}</span></span>{/if}{/if}{#if f}<div><div class="file"><span class="ficon"><Icon name={f.kind === 'image' ? 'image' : 'file'} size={22} /></span><span class="fi"><span class="fn" dir="auto" title={f.name}>{f.name}</span><span class="fm">{kindLabel} · {fmtSize(f.size)}{fstate}</span></span></div>{#if f.state === 'sending' || f.state === 'waiting'}<div class="prog" role="progressbar" aria-label="Sending {f.name}" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(f.progress * 100)}><i style:width="{f.progress * 100}%" style:--tick="{SIM.tickMs}ms"></i></div><div class="facts"><button type="button" class="qbtn" data-act="cancel" aria-label="Cancel sending {f.name}" onclick={() => oncancel(m)}><Icon name="x" size={14} />Cancel</button></div>{/if}</div>{:else}{m.text}{/if}<span class="meta">{#if m.edited}<span>edited</span>{/if}<time>{hhmm(m.at)}</time>{#if mine && m.delivery !== 'failed'}<span class="tk">{#if pending}<Icon name="clock" size={11} class="clk" />{:else}<Icon name="check" size={12} />{/if}</span><span class="vh">{stLabel}</span>{/if}</span>{/if}</div>
      {#if !gone}<button type="button" class="more" aria-haspopup="menu" aria-label="Actions for {mine ? 'your' : fn + '’s'} message at {hhmm(m.at)}: {summary.slice(0, 40)}"
        onclick={(e) => onmenu(m, e.currentTarget, popMode ? 'pop' : 'sheet')}><Icon name="chev" size={20} /></button>{/if}
    </div>
    {#if mine && m.delivery === 'failed' && !gone}
      <div class="fail"><span>{f?.state === 'cancelled' ? 'Cancelled' : 'Not delivered'}</span><span aria-hidden="true">·</span>
        <button type="button" data-act="retry" aria-label={f ? 'Retry sending ' + f.name : 'Retry sending'} onclick={() => onretry(m)}>Retry</button></div>
    {/if}
  </li>
{/if}

<style>
  .m { position: relative; margin-top: 10px; }
  .m.g-mid, .m.g-last { margin-top: 2px; }
  .line { display: flex; align-items: center; gap: 4px; }
  .me .line { flex-direction: row-reverse; }
  .bub { max-width: min(78%, 560px); min-width: 0; padding: 7px 12px; font-size: var(--fs-body); line-height: var(--lh-body); border-radius: var(--r-bub); overflow-wrap: anywhere; white-space: pre-wrap;
    background: var(--bub-in); color: var(--bub-in-ink); border: 1px solid var(--bub-in-line); transition: transform var(--dur-release) var(--ease-out), box-shadow var(--dur-switch) var(--ease-out); }
  .me .bub { background: var(--bub-out); color: var(--bub-out-ink); border-color: transparent; }
  .me.g-first .bub { border-bottom-right-radius: var(--r-bub-tight); }
  .me.g-mid .bub { border-top-right-radius: var(--r-bub-tight); border-bottom-right-radius: var(--r-bub-tight); }
  .me.g-last .bub { border-top-right-radius: var(--r-bub-tight); }
  .them.g-first .bub { border-bottom-left-radius: var(--r-bub-tight); }
  .them.g-mid .bub { border-top-left-radius: var(--r-bub-tight); border-bottom-left-radius: var(--r-bub-tight); }
  .them.g-last .bub { border-top-left-radius: var(--r-bub-tight); }
  .bub.held { transform: scale(.97); }
  .bub.flash { animation: flash calc(var(--dur-overlayOpen) * 5) var(--ease-out); }
  @keyframes flash { 0%, 55% { box-shadow: 0 0 0 2px var(--accent); } 100% { box-shadow: 0 0 0 2px transparent; } }
  .bub.gone { background: transparent; border-color: var(--hairline); color: var(--muted); font-style: italic; }
  .bub.emoji { background: transparent; border-color: transparent; padding: 0 2px; font-size: 34px; line-height: 42px; }
  .bub.emoji .meta { float: none; display: flex; justify-content: flex-end; margin: 0; font-size: var(--fs-meta); line-height: 1; color: var(--muted); }
  .who { display: block; font-size: 13px; font-weight: 700; line-height: 18px; margin-bottom: 1px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .meta { float: right; margin: 10px -2px -2px 10px; font-size: var(--fs-meta); line-height: 1; font-variant-numeric: tabular-nums; display: inline-flex; gap: 3px; align-items: center;
    user-select: none; -webkit-user-select: none; white-space: nowrap; font-style: normal; color: var(--bub-meta-in); }
  .me .meta { color: var(--bub-meta-out); }
  .tk { display: inline-grid; place-items: center; width: 13px; height: 11px; }
  .tk :global(svg) { width: 12px; height: 12px; stroke-width: 2.2; }
  .tk :global(svg.clk) { width: 11px; height: 11px; stroke-width: 2; }
  .quote { display: block; inline-size: 100%; margin: 2px 0 6px; padding: 5px 10px 6px; border-radius: 12px; background: var(--surface-2); border: 0; border-inline-start: 3px solid var(--accent);
    white-space: normal; font-size: var(--fs-sub); line-height: 18px; color: var(--ink); text-align: start; }
  button.quote { cursor: pointer; }
  .quote b { display: block; font-size: 12px; font-weight: 700; color: var(--accent-ink); }
  .quote span { display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; color: var(--muted); }
  .fail { display: flex; justify-content: flex-end; align-items: center; gap: 5px; margin: 3px 2px 0 0; font-size: 12px; line-height: 16px; color: var(--danger); }
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
  .m:not(.popm):has(.more:focus-visible) .bub { outline: 2px solid var(--accent-ink); outline-offset: 2px; }
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
