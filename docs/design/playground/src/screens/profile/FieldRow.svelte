<script lang="ts">
  /* One piece of profile information. View: label over value, tap anywhere on it to edit. Edit: field, Save / Cancel
     (Remove for optional ones), the draft is kept when a save fails. Read-only rows (other people) just show the value.
     Values this version can't edit (nested data from another app version) are shown readably and can only be removed. */
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import Value from './Value.svelte';
  import { isScalar, safeUrl, shortUrl } from './info';
  import { utf8, type Result } from '../../lib/model/actions/profile';

  let { label, value, kind = 'text', hint = '', placeholder = '', optional = false, readonly = false, limit = 0, offline = false, empty = 'Not set', note = '', onsave, onremove, onopen, field }: {
    label: string; value: unknown; kind?: 'text' | 'multiline' | 'link'; hint?: string; placeholder?: string; optional?: boolean; readonly?: boolean;
    limit?: number; offline?: boolean; empty?: string; note?: string; field?: string;
    onsave?: (v: string) => Result; onremove?: () => Result; onopen?: (url: string) => void;
  } = $props();

  const uid = $props.id();
  const SAVED_MS = 1800; // how long "Saved" stays on the row; a UI constant, not a motion timing
  let mode = $state<'view' | 'edit' | 'remove'>('view');
  let draft = $state('');
  let error = $state('');
  let saved = $state(false);
  let timer = 0;

  const text = $derived(typeof value === 'string' ? value : value == null ? '' : isScalar(value) ? String(value) : '');
  const editable = $derived(!readonly && !!onsave && (value == null || typeof value === 'string' || isScalar(value)));
  const removable = $derived(!readonly && !!onremove && optional && !editable);
  const url = $derived(kind === 'link' ? safeUrl(text) : null);
  const bytes = $derived(utf8(draft));
  const near = $derived(limit > 0 && bytes > limit * 0.8);

  function edit() { draft = text; error = ''; mode = 'edit'; }
  function cancel() { mode = 'view'; error = ''; }
  function save(e?: Event) {
    e?.preventDefault();
    const r = onsave?.(draft);
    if (!r) return;
    if (r.ok) {
      mode = 'view'; error = ''; saved = true;
      clearTimeout(timer); timer = window.setTimeout(() => (saved = false), SAVED_MS);
    } else error = r.error;
  }
  function drop() {
    const r = onremove?.();
    if (r && !r.ok) { error = r.error; return; }
    mode = 'view'; error = '';
  }
  function key(e: KeyboardEvent) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); cancel(); }
    else if (e.key === 'Enter' && kind === 'multiline' && (e.ctrlKey || e.metaKey)) { e.preventDefault(); save(); }
  }
  const focusIn = (el: HTMLElement) => { el.focus({ preventScroll: false }); if (el instanceof HTMLInputElement) el.select(); };
</script>

<div class="row" data-field={field}>
  {#if mode === 'edit'}
    <form class="edit" onsubmit={save} novalidate>
      <label for="{uid}-in">{label}</label>
      {#if kind === 'multiline'}
        <textarea id="{uid}-in" rows="4" bind:value={draft} {placeholder} onkeydown={key} use:focusIn aria-describedby="{uid}-d"></textarea>
      {:else}
        <input id="{uid}-in" type="text" bind:value={draft} {placeholder} onkeydown={key} inputmode={kind === 'link' ? 'url' : undefined} autocapitalize="none" autocomplete="off" spellcheck="false" use:focusIn aria-describedby="{uid}-d" />
      {/if}
      <div id="{uid}-d" class="below">
        {#if error}<p class="err" role="alert">{error}</p>{:else if hint}<p class="hint">{hint}</p>{/if}
        {#if near}<p class="count" class:over={bytes > limit}>{bytes} of {limit} bytes</p>{/if}
      </div>
      <div class="acts">
        {#if optional && value != null}<span class="rm"><Btn variant="dangerquiet" onclick={drop}>Remove</Btn></span>{/if}
        <Btn variant="quiet" onclick={cancel}>Cancel</Btn>
        <Btn variant="primary" type="submit">Save</Btn>
      </div>
    </form>
  {:else if mode === 'remove'}
    <div class="edit">
      <p class="lead">Remove “{label}” from your profile?</p>
      <p class="hint">It disappears from your profile. Earlier versions can remain in synced history.</p>
      {#if error}<p class="err" role="alert">{error}</p>{/if}
      <div class="acts"><Btn variant="quiet" onclick={cancel}>Cancel</Btn><Btn variant="danger" onclick={drop}>Remove</Btn></div>
    </div>
  {:else}
    <div class="line" class:live={editable}>
      {#if editable}
        <button type="button" class="main press" onclick={edit} aria-label="{label}: {text || empty}. Edit">
          <span class="txt">
            <span class="lbl">{label}{#if saved}<span class="saved"><Icon name="check" size={13} />{offline ? 'Saved · syncs when connected' : 'Saved'}</span>{/if}</span>
            <span class="val" class:ph={!text} class:ml={kind === 'multiline'} dir="auto">{text || empty}</span>
            {#if note}<span class="sub">{note}</span>{/if}
          </span>
        </button>
      {:else}
        <div class="main static" class:bare={!label}>
          <span class="txt">
            {#if label || saved}<span class="lbl">{label}{#if saved}<span class="saved"><Icon name="check" size={13} />Saved</span>{/if}</span>{/if}
            <span class="val" class:ml={kind === 'multiline'} dir="auto">{#if value == null || value === ''}<span class="ph">{empty}</span>{:else if typeof value === 'string'}{value}{:else}<Value v={value} />{/if}</span>
            {#if note}<span class="sub">{note}</span>{/if}
          </span>
        </div>
      {/if}
      {#if url && onopen}
        <button type="button" class="tool press" aria-label="Open {label}: {shortUrl(url)}" onclick={() => onopen?.(url)}><Icon name="link" size={20} /></button>
      {/if}
      {#if editable}
        <!-- The pencil is a pointer convenience; the text button beside it is the accessible control. -->
        <button type="button" class="tool pen" tabindex="-1" aria-hidden="true" onclick={edit}><Icon name="edit" size={18} /></button>
      {/if}
      {#if removable}
        <button type="button" class="tool press" aria-label="Remove {label}" onclick={() => { error = ''; mode = 'remove'; }}><Icon name="trash" size={20} /></button>
      {/if}
    </div>
  {/if}
  <span class="vh" role="status">{saved ? 'Saved' : ''}</span>
</div>

<style>
  .row { display: block; }
  .line { display: flex; align-items: stretch; }
  .line.live:hover { background: var(--surface-2); }
  .main { flex: 1; min-inline-size: 0; display: flex; align-items: center; gap: 12px; min-block-size: 64px; padding: 10px 16px; border: 0; background: none; color: var(--ink); font: inherit; text-align: start; cursor: pointer; }
  .main.static { cursor: default; align-items: flex-start; padding-block: 12px; }
  .main.static.bare { min-block-size: 0; padding-block: 14px; }
  .txt { flex: 1; min-inline-size: 0; display: grid; gap: 1px; }
  .lbl { display: flex; align-items: center; gap: 8px; font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .saved { display: inline-flex; align-items: center; gap: 3px; white-space: nowrap; font-weight: 600; color: var(--accent-ink); animation: fadein var(--dur-enter) var(--ease-out); }
  .val { line-height: 21px; overflow-wrap: anywhere; }
  .val.ml { white-space: pre-wrap; }
  .ph { color: var(--muted); }
  .sub { font-size: var(--fs-sub); line-height: 18px; color: var(--muted); margin-block-start: 2px; }
  .tool { flex: none; inline-size: var(--btn); border: 0; background: none; color: var(--muted); display: grid; place-items: center; cursor: pointer; border-radius: var(--r-ctl); }
  .tool:hover { color: var(--ink); }
  .tool.pen { opacity: .8; margin-inline-end: 6px; }
  .tool:not(.pen) + .pen { margin-inline-start: -8px; }

  .edit { position: relative; padding: 14px 16px 12px; display: grid; gap: 6px; animation: fadein var(--dur-crossfade) var(--ease-out); }
  label { font-size: var(--fs-sub); font-weight: 600; color: var(--muted); line-height: 18px; }
  input, textarea { inline-size: 100%; resize: none; padding: 10px 14px; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); font-size: var(--fs-body); line-height: 21px; color: var(--ink); min-block-size: var(--btn); }
  input:focus, textarea:focus { outline: 0; border-color: var(--accent); box-shadow: var(--ring-focus); }
  /* The hint / error line is always there (the counter sits on the label line), so typing or a refused save never moves the buttons. */
  .below { display: grid; gap: 2px; align-content: start; min-block-size: 36px; }   /* room for a refusal that wraps to two lines */
  .below p { margin: 0; font-size: var(--fs-sub); line-height: 18px; }
  .hint { color: var(--muted); }
  .err { color: var(--danger); }
  .count { position: absolute; inset-block-start: 14px; inset-inline-end: 16px; color: var(--muted); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .count.over { color: var(--danger); }
  .lead { margin: 0; font-weight: 600; }
  .edit > p.hint { margin: 0; font-size: var(--fs-sub); line-height: 19px; }
  .acts { display: flex; gap: 8px; justify-content: flex-end; align-items: center; margin-block-start: 0; }
  .rm { margin-inline-end: auto; }
  @keyframes fadein { from { opacity: 0; } }
</style>
