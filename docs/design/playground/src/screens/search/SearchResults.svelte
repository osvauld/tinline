<script lang="ts">
  /* Results: "People & groups" (a fixed-height strip), then "Messages" (fixed-height rows, newest first, cursor paging).
     Layout rule (user, hard): nothing moves when results change. The two section headers and the strip sit at fixed positions;
     rows are keyed by position, so a new query changes the text of the rows that are there instead of pushing them around;
     hints ("No messages match", "Type 2 or more characters", "Search is getting ready") are overlays, not rows. */
  import { onMount, tick, untrack } from 'svelte';
  import Avatar from '../../lib/ui/Avatar.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import Marked from './Marked.svelte';
  import { SEARCH, searchPages, searchSim } from '../../lib/model/actions/search';
  import type { MessageHit, NameHit } from '../../lib/model/actions/search';
  import { person, db } from '../../lib/model/store.svelte';
  import { listStamp } from '../../lib/model/clock.svelte';
  import { ui, openHit, openName } from './state.svelte';

  let { onactive }: { onactive?: (id: string) => void } = $props();

  const q = $derived(ui.applied.trim());
  const typing = $derived(ui.q.trim() !== '');
  const msgsOk = $derived(Array.from(q).length >= SEARCH.minChars);
  let paged = $state({ q: '', n: 1 });
  const n = $derived(paged.q === q ? paged.n : 1);
  const res = $derived(searchPages(q, n));
  const names = $derived(res.names);
  const msgs = $derived(res.messages);

  /* selection (keyboard): a flat index over names then messages; -1 = none */
  let sel = $state(-1);
  const total = $derived(names.length + msgs.length);
  const optId = (i: number) => `sr-opt-${i}`;
  $effect(() => { q; sel = -1; });
  $effect(() => { onactive?.(sel >= 0 && sel < total ? optId(sel) : ''); });
  $effect(() => {
    if (sel < 0) return;
    const i = sel;
    untrack(() => tick().then(() => document.getElementById(optId(i))?.scrollIntoView({ block: 'nearest', inline: 'nearest' })));
  });

  let listEl = $state<HTMLElement>();
  let tail = $state<HTMLElement>();
  $effect(() => { q; if (listEl) listEl.scrollTop = 0; });

  /* next page when the end of the list is near */
  function more() {
    if (!listEl || !tail || !res.more) return;
    const l = listEl.getBoundingClientRect(), t = tail.getBoundingClientRect();
    if (t.top - l.bottom < 240) paged = { q, n: n + 1 };
  }
  $effect(() => { msgs.length; res.more; untrack(() => requestAnimationFrame(more)); });

  const openAt = (i: number) => { if (i < names.length) openName(names[i]); else if (msgs[i - names.length]) openHit(msgs[i - names.length]); };

  /** From the field. Down/Up walk names then messages; Left/Right walk the names; Enter opens the selection, or the top hit. */
  function key(e: KeyboardEvent): boolean {
    if (!typing || e.ctrlKey || e.metaKey || e.altKey) return false;
    if (e.key === 'ArrowDown') { if (total) sel = Math.min(total - 1, sel + 1); return true; }
    if (e.key === 'ArrowUp') { sel = sel <= 0 ? -1 : sel - 1; return true; }
    if ((e.key === 'ArrowRight' || e.key === 'ArrowLeft') && sel >= 0 && sel < names.length) {
      sel = Math.max(0, Math.min(names.length - 1, sel + (e.key === 'ArrowRight' ? 1 : -1))); return true;
    }
    if (e.key === 'Enter') { if (total) openAt(sel >= 0 ? sel : 0); else ui.remember(); return true; }
    return false;
  }
  onMount(() => { ui.keys = key; return () => { if (ui.keys === key) ui.keys = null; }; });

  const by = (h: MessageHit) => (h.kind === 'group' ? (h.author === 'me' ? 'You' : h.author_name.split(' ')[0]) + ': ' : h.author === 'me' ? 'You: ' : '');
  const hue = (id: string) => person(id)?.hue ?? 0;
  const statusWord = (s?: string) => (s === 'removed' ? 'Removed' : s === 'left' ? 'Left' : '');
  const hint = $derived(!typing || !q ? '' : !msgsOk ? 'Type 2 or more characters to search messages.' : q && !msgs.length && !searchSim.rebuilding ? `No messages match “${q}”` : '');
  const emptyStrip = $derived(typing && q !== '' && !names.length);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="res" data-search-results onmousedown={(e) => { if (!(e.target as HTMLElement).closest('input')) e.preventDefault(); }}>
  {#if !typing}
    <div class="hd"><h3>Recent</h3>
      {#if ui.recents.length}<button type="button" class="link" onclick={() => ui.forgetAll()}>Clear</button>{/if}</div>
    <ul class="recents" role="list">
      {#each ui.recents as r, i (i)}
        <li><button type="button" class="rrow press" onclick={() => ui.set(r)}><span class="ri"><Icon name="clock" size={18} /></span><span class="rt">{r}</span></button></li>
      {/each}
    </ul>
    <p class="blank" class:on={!ui.recents.length}><span>Search messages, people and groups.</span><span>Everything stays on this device.</span></p>
  {:else}
    <div class="hd"><h3 id="sr-h-people">People &amp; groups</h3></div>
    <div class="strip" role="listbox" aria-labelledby="sr-h-people" aria-orientation="horizontal" id="sr-list">
      {#each names as nm, i (i)}
        <button type="button" class="tile" class:sel={sel === i} class:dim={!!nm.status} role="option" id={optId(i)} aria-selected={sel === i} tabindex="-1" onclick={() => openName(nm)}
          aria-label="{nm.name}, {nm.kind === 'group' ? 'group' : nm.kind === 'me' ? 'your profile' : 'contact'}{nm.status ? ', ' + statusWord(nm.status).toLowerCase() : ''}">
          {#if nm.kind === 'group'}<Avatar name={nm.name} group size={48} />
          {:else if nm.kind === 'me'}<Avatar name={db.account.name} hue={db.account.hue} person="me" size={48} />
          {:else}<Avatar name={nm.name} hue={hue(nm.id)} person={nm.id} size={48} />{/if}
          <span class="tn"><Marked text={nm.name} ranges={nm.highlights} /></span>
        </button>
      {/each}
      <p class="none" class:on={emptyStrip}>No people or groups match</p>
    </div>

    <div class="hd"><h3 id="sr-h-msgs">Messages</h3></div>
    <div class="msgs">
      <ul class="list" role="listbox" aria-labelledby="sr-h-msgs" bind:this={listEl} onscroll={more}>
        {#each msgs as h, i (i)}
          {@const k = names.length + i}
          <li role="presentation">
            <button type="button" class="row press" class:sel={sel === k} role="option" id={optId(k)} aria-selected={sel === k} tabindex="-1" onclick={() => openHit(h)}
              data-hit={h.id}>
              <Avatar name={h.conv_name} hue={hue(h.conv)} group={h.kind === 'group'} person={h.kind === 'dm' ? h.conv : undefined} size={44} />
              <span class="mid">
                <span class="top"><span class="name">{h.conv_name}{#if h.status}<span class="st"> · {statusWord(h.status)}</span>{/if}</span><span class="when">{listStamp(h.at)}</span></span>
                <span class="snip"><span class="by">{by(h)}</span><Marked text={h.snippet} ranges={h.highlights} /></span>
              </span>
            </button>
          </li>
        {/each}
        <li class="tail" bind:this={tail} aria-hidden="true"></li>
      </ul>
      <p class="hint" class:on={!!hint} role="status">{hint}</p>
    </div>
  {/if}
  <p class="getting" class:on={searchSim.rebuilding} role="status"><Icon name="clock" size={14} />Search is getting ready. Older messages may be missing for now.</p>
</div>

<style>
  .res { position: relative; display: flex; flex-direction: column; block-size: 100%; min-block-size: 0; }
  .hd { display: flex; align-items: center; justify-content: space-between; block-size: 36px; padding: 0 16px; flex: none; }
  h3 { margin: 0; font-size: 12px; font-weight: 650; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); line-height: 16px; }
  .link { position: relative; border: 0; background: transparent; color: var(--accent-ink); font-size: var(--fs-sub); font-weight: 650; cursor: pointer; padding: 0; }
  .link::after { content: ''; position: absolute; inset: -14px -10px; }

  /* people & groups: one row of tiles at a fixed height; the empty state is an overlay in the same box */
  .strip { position: relative; display: flex; gap: 2px; padding: 0 8px 8px; block-size: 100px; flex: none; overflow-x: auto; overflow-y: hidden; scrollbar-width: none; overscroll-behavior-x: contain; }
  .strip::-webkit-scrollbar { display: none; }
  .tile { flex: none; inline-size: 78px; block-size: 92px; display: flex; flex-direction: column; align-items: center; gap: 6px; padding: 6px 4px 0; border: 0; border-radius: var(--r-ctl);
    background: transparent; cursor: pointer; color: var(--ink); }
  .tile:hover { background: var(--surface-2); }
  .tile.sel { background: var(--accent-soft); }
  .tile.dim :global(.t-avatar) { opacity: .72; }
  .tn { inline-size: 100%; font-size: 12px; line-height: 15px; text-align: center; overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; }
  .tn :global(.b) { font-weight: 700; }
  .none { position: absolute; inset: 0; margin: 0; display: grid; place-items: center; font-size: var(--fs-sub); color: var(--muted); opacity: 0; pointer-events: none; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .none.on { opacity: 1; }

  /* messages */
  .msgs { position: relative; flex: 1; min-block-size: 0; display: flex; flex-direction: column; }
  .list { list-style: none; margin: 0; padding: 0 8px 12px; flex: 1; min-block-size: 0; overflow-y: auto; overscroll-behavior: contain; scrollbar-gutter: stable; }
  .tail { block-size: 1px; }
  .row { display: flex; gap: 12px; align-items: flex-start; inline-size: 100%; block-size: 80px; padding: 10px 8px; border: 0; border-radius: var(--r-ctl); background: transparent; text-align: start; cursor: pointer; color: inherit; font: inherit; }
  .row:hover { background: var(--surface-2); }
  .row.sel { background: var(--accent-soft); }
  .mid { flex: 1; min-inline-size: 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 2px; }
  .top { display: flex; align-items: baseline; gap: 8px; justify-content: space-between; }
  .name { flex: 1; min-inline-size: 0; font-weight: 600; line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .st { font-weight: 500; color: var(--muted); }
  .when { font-size: var(--fs-meta); color: var(--muted); flex: none; min-inline-size: 56px; text-align: end; font-variant-numeric: tabular-nums; }
  .snip { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); overflow-wrap: anywhere; display: -webkit-box; -webkit-line-clamp: 2; line-clamp: 2; -webkit-box-orient: vertical; overflow: hidden; min-block-size: 18px; }
  .by { color: var(--muted); }
  .hint { position: absolute; inset: 0; margin: 0; padding: 0 32px; display: grid; place-items: center; text-align: center; font-size: var(--fs-sub); line-height: 19px; color: var(--muted);
    opacity: 0; pointer-events: none; transition: opacity var(--dur-crossfade) var(--ease-out); overflow-wrap: anywhere; }
  .hint.on { opacity: 1; }

  /* recent searches */
  .recents { list-style: none; margin: 0; padding: 0 8px; }
  .rrow { display: flex; align-items: center; gap: 12px; inline-size: 100%; min-block-size: 48px; padding: 0 8px; border: 0; border-radius: var(--r-ctl); background: transparent; cursor: pointer; text-align: start; color: var(--ink); }
  .rrow:hover { background: var(--surface-2); }
  .ri { inline-size: 32px; display: grid; place-items: center; color: var(--muted); flex: none; }
  .rt { min-inline-size: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .blank { position: absolute; inset: 0; margin: 0; padding: 0 32px; display: grid; align-content: center; justify-items: center; gap: 4px; text-align: center; font-size: var(--fs-sub); line-height: 19px; color: var(--muted);
    opacity: 0; pointer-events: none; transition: opacity var(--dur-crossfade) var(--ease-out); }
  .blank.on { opacity: 1; }

  /* proposed: the reconcile state, an overlay that fades (nothing moves) */
  .getting { position: absolute; inset-inline: 16px; inset-block-end: 12px; margin: 0; padding: 8px 14px; display: flex; align-items: center; gap: 8px; border-radius: 14px; background: var(--surface); color: var(--muted);
    box-shadow: var(--shadow-pop), inset 0 0 0 1px var(--hairline); font-size: var(--fs-sub); line-height: 18px; opacity: 0; pointer-events: none; transition: opacity var(--dur-enter) var(--ease-out); }
  .getting.on { opacity: 1; }
</style>
