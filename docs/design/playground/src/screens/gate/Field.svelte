<script lang="ts">
  import Icon from '../../lib/ui/Icon.svelte';
  let { value = $bindable(''), label, hint, error, secret = false, mono = false, autofocus = false, autocomplete = 'off' as any, placeholder = '', disabled = false,
    onenter, id, describedBy }:
    { value?: string; label: string; hint?: string; error?: string | null; secret?: boolean; mono?: boolean; autofocus?: boolean; autocomplete?: HTMLInputElement['autocomplete'] | string; placeholder?: string; disabled?: boolean;
      onenter?: () => void; id?: string; describedBy?: string } = $props();
  const gen = $props.id();
  const uid = $derived(id ?? 'f' + gen);
  let shown = $state(false);
</script>

<div class="field" class:bad={!!error}>
  <label for={uid}>{label}</label>
  <div class="wrap">
    <input id={uid} class:mono bind:value type={secret && !shown ? 'password' : 'text'} autocomplete={autocomplete as any} {placeholder} {disabled}
      data-autofocus={autofocus || undefined} aria-invalid={error ? 'true' : undefined}
      aria-describedby={[error ? uid + '-e' : hint ? uid + '-h' : '', describedBy].filter(Boolean).join(' ') || undefined}
      autocapitalize="off" spellcheck="false"
      onkeydown={(e) => { if (e.key === 'Enter') onenter?.(); }} />
    {#if secret}
      <button type="button" class="eye press" aria-label={shown ? 'Hide ' + label.toLowerCase() : 'Show ' + label.toLowerCase()} aria-pressed={shown} onclick={() => (shown = !shown)}>
        <Icon name={shown ? 'eyeoff' : 'eye'} size={20} />
      </button>
    {/if}
  </div>
  {#if error}<p id={uid + '-e'} class="msg err" role="alert">{error}</p>{:else if hint}<p id={uid + '-h'} class="msg">{hint}</p>{/if}
</div>

<style>
  .field { display: grid; gap: 6px; }
  label { font-size: var(--fs-sub); font-weight: 650; color: var(--ink); }
  .wrap { position: relative; display: flex; }
  input { inline-size: 100%; min-block-size: 48px; padding-inline: 14px; border: 1px solid var(--line); border-radius: var(--r-ctl); background: var(--surface); color: var(--ink); }
  input.mono { font-family: ui-monospace, 'SF Mono', Menlo, Consolas, monospace; }
  input::placeholder { color: var(--muted); opacity: .8; }
  input:focus-visible { outline: 2px solid var(--accent-ink); outline-offset: 1px; border-color: transparent; }
  .field:has(.eye) input { padding-inline-end: 52px; }
  .bad input { border-color: var(--danger); }
  .eye { position: absolute; inset-inline-end: 2px; inset-block-start: 2px; inline-size: 44px; block-size: 44px; display: grid; place-items: center; border: 0; background: transparent; color: var(--muted); border-radius: 50%; cursor: pointer; }
  .eye:hover { color: var(--ink); }
  .msg { margin: 0; font-size: var(--fs-sub); line-height: 18px; color: var(--muted); }
  .msg.err { color: var(--danger); font-weight: 600; }
</style>
