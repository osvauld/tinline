<script lang="ts">
  /* Composer: attach (1:1 only) · autosizing field · send. Draft lives on the chat (`c.draft`), so the
     list shows it. Reply and edit modes show a context chip with a cancel button. */
  import Icon from '../../lib/ui/Icon.svelte';
  import { firstName } from '../../lib/model/store.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { canAttach, editMessage, sendText } from '../../lib/model/actions/chat';
  import type { Chat, Message } from '../../lib/model/types';
  import { shape, shapeRing, cssRadii } from '../../lib/geom/action';
  import { geo, R, uniform } from '../../lib/geom/geo.svelte';
  const fieldRadii = uniform(R().field);
  const ctxRadii = uniform(R().quote);

  export type Ctx = { type: 'reply' | 'edit'; id: string } | null;
  let { c, ctx, target, oncancel, onsent, onattach, onedited }: {
    c: Chat; ctx: Ctx; target: Message | undefined; oncancel: () => void; onsent: () => void; onattach: () => void; onedited: () => void;
  } = $props();

  let ta = $state<HTMLTextAreaElement>();
  const editing = $derived(ctx?.type === 'edit');
  const empty = $derived(!c.draft.trim());
  const desktop = $derived(prefs.form === 'desktop');
  const name = $derived(c.kind === 'direct' ? firstName(c.person) : c.name);

  export function focus(end = false) {
    ta?.focus({ preventScroll: true });
    if (end && ta) ta.setSelectionRange(ta.value.length, ta.value.length);
  }

  function autosize() {
    if (!ta) return;
    ta.style.height = 'auto';
    ta.style.height = Math.min(ta.scrollHeight + 2, 140) + 'px';
  }
  $effect(() => { c.draft; ctx; autosize(); });

  function submit() {
    if (empty) { ta?.focus(); return; }
    if (editing && target) {
      if (editMessage(c, target, c.draft)) onedited();
      return;
    }
    if (sendText(c, c.draft, ctx?.type === 'reply' ? ctx.id : undefined)) onsent();
    ta?.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key === 'Enter') {
      const wantSend = desktop ? !e.shiftKey : e.ctrlKey || e.metaKey;
      if (wantSend) { e.preventDefault(); submit(); }
    } else if (e.key === 'Escape' && ctx) { e.preventDefault(); oncancel(); }
  }
</script>

<div class="composer" class:solo={!canAttach(c)}>
  {#if ctx && target}
    <div class="ctx" use:shape={{ radii: ctxRadii, smoothing: geo.smooth }}>
      <div class="tx">
        <b>{editing ? 'Editing message' : 'Replying to ' + (target.from === 'me' ? 'yourself' : firstName(target.from))}</b>
        <span>{target.text || target.file?.name || ''}</span>
      </div>
      <button type="button" class="ib" aria-label={editing ? 'Cancel edit' : 'Cancel reply'} onclick={oncancel}><Icon name="x" size={18} /></button>
    </div>
  {/if}
  {#if canAttach(c)}
    <button type="button" class="ib att" aria-label="Attach a file" disabled={editing} onclick={onattach}><Icon name="clip" /></button>
  {/if}
  <div class="field" style:--rad={cssRadii(fieldRadii)}>
  <span class="skin" aria-hidden="true" use:shape={{ radii: fieldRadii, smoothing: geo.smooth, stroke: { width: 1, color: 'var(--hairline)' } }}></span>
  <svg class="ring" aria-hidden="true" use:shapeRing={{ radii: fieldRadii.map((r) => r + 3) as typeof fieldRadii, smoothing: geo.smooth }}><path class="glow" data-inset="1.5" /></svg>
  <textarea
    bind:this={ta} rows="1" maxlength="4000" placeholder="Message" aria-label="Message to {name}" enterkeyhint={desktop ? 'send' : 'enter'}
    value={c.draft} oninput={(e) => (c.draft = e.currentTarget.value)} {onkeydown}
  ></textarea>
  </div>
  <button type="button" class="send press" aria-label={editing ? 'Save edit' : 'Send'} aria-disabled={empty} onclick={submit}><Icon name={editing ? 'check' : 'send'} size={20} /></button>
</div>

<style>
  .composer { display: grid; grid-template-columns: var(--btn) minmax(0, 1fr) var(--btn); column-gap: 6px; align-items: end; padding: 8px 10px 10px; flex: none; }
  .composer.solo { grid-template-columns: minmax(0, 1fr) var(--btn); }
  .composer > .ib, .composer > .field, .composer > .send { grid-row: 2; }
  .ib { inline-size: var(--btn); block-size: var(--btn); display: grid; place-items: center; border: 0; border-radius: 50%; background: transparent; color: var(--ink); flex: none; padding: 0; cursor: pointer; }
  .ib:hover:not(:disabled) { background: var(--surface-2); }
  .ib:disabled { opacity: .38; cursor: default; }
  .att { color: var(--muted); }
  .att:hover { color: var(--ink); }
  .ctx { grid-row: 1; grid-column: 2; display: flex; align-items: center; gap: 8px; margin-bottom: 6px; padding: 4px 2px 4px 10px; border-radius: 12px; background: var(--surface-2); border-inline-start: 3px solid var(--accent); font-size: var(--fs-sub); line-height: 18px; }
  .solo .ctx { grid-column: 1; }
  .ctx .tx { flex: 1; min-width: 0; }
  .ctx .tx b, .ctx .tx span { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ctx .tx b { color: var(--accent-ink); font-weight: 600; }
  .ctx .tx span { color: var(--muted); }
  .ctx .ib { color: var(--muted); margin-block: -4px; }
  /* The field is a wrapper: .skin is the Bézier-clipped surface + hairline, the ring SVG draws the focus glow outside it (a clip-path on the
     textarea itself would cut that off), the textarea is transparent on top. Box size = the old textarea's, so nothing moves. */
  .field { position: relative; min-inline-size: 0; isolation: isolate; }
  .skin { position: absolute; inset: 0; z-index: -1; pointer-events: none; border-radius: var(--rad); background: var(--surface); box-shadow: inset 0 0 0 1px var(--hairline); transition: box-shadow var(--dur-switch) var(--ease-out); }
  .skin:global([data-shaped]) { box-shadow: none; }
  .field:focus-within .skin:not(:global([data-shaped])) { box-shadow: inset 0 0 0 1px var(--accent); }
  .skin :global(svg path) { transition: stroke var(--dur-switch) var(--ease-out); }
  .field:focus-within .skin :global(svg path) { stroke: var(--accent) !important; }
  .ring { position: absolute; inset-block-start: -3px; inset-inline-start: -3px; inline-size: calc(100% + 6px); block-size: calc(100% + 6px); overflow: visible; pointer-events: none; }
  .ring path { fill: none; opacity: 0; transition: opacity var(--dur-switch) var(--ease-out); }
  .ring .glow { stroke: color-mix(in srgb, var(--accent-ink) 14%, transparent); stroke-width: 3; }
  .field:focus-within .glow { opacity: 1; }
  textarea { display: block; inline-size: 100%; resize: none; min-height: var(--btn); max-height: 140px; padding: 10px 16px; border-radius: var(--r-field); border: 1px solid transparent; background: transparent; color: var(--ink); font-size: var(--fs-body); line-height: 22px; }
  textarea::placeholder { color: var(--muted); }
  textarea:focus, textarea:focus-visible { outline: 0; }
  .send { inline-size: var(--btn); block-size: var(--btn); border-radius: 50%; border: 0; padding: 0; background: var(--accent); color: var(--on-accent); display: grid; place-items: center; cursor: pointer; }
  .send :global(svg) { margin: 0 2px 0 0; }
  .send[aria-disabled='true'] { opacity: .38; cursor: default; transform: none !important; }
</style>
