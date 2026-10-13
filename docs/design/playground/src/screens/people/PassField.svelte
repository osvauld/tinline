<script lang="ts">
  /* A passphrase field with a show/hide toggle and a reserved error line (appearing text moves nothing). */
  import Icon from '../../lib/ui/Icon.svelte';
  let { value = $bindable(''), label = 'Passphrase', error = '', id, autofocus = false, oninput, hint = '' }: {
    value?: string; label?: string; error?: string; id: string; autofocus?: boolean; oninput?: () => void; hint?: string;
  } = $props();
  let show = $state(false);
</script>

<div class="pw">
  <label for={id}>{label}</label>
  <div class="in" class:bad={!!error}>
    <input {id} type={show ? 'text' : 'password'} bind:value autocomplete="off" spellcheck="false" data-autofocus={autofocus ? '' : undefined}
      aria-invalid={error ? 'true' : undefined} aria-describedby="{id}-m" oninput={() => oninput?.()} />
    <button type="button" class="eye press" aria-pressed={show} aria-label={show ? 'Hide passphrase' : 'Show passphrase'} onclick={() => (show = !show)}><Icon name={show ? 'eyeoff' : 'eye'} size={20} /></button>
  </div>
  <p class="m" id="{id}-m" class:err={!!error} role={error ? 'alert' : undefined}>{error || hint}</p>
</div>

<style>
  .pw { display: grid; gap: 6px; text-align: start; }
  label { font-weight: 600; font-size: var(--fs-sub); }
  .in { display: flex; align-items: center; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); }
  .in:focus-within { border-color: var(--accent); box-shadow: 0 0 0 1px var(--accent); }
  .in.bad { border-color: var(--danger); box-shadow: none; }
  input { flex: 1; min-inline-size: 0; min-block-size: var(--btn); padding: 0 0 0 14px; border: 0; background: none; outline: 0; font: inherit; font-size: var(--fs-body); color: var(--ink); }
  .eye { inline-size: 44px; block-size: 44px; flex: none; border: 0; background: none; color: var(--muted); cursor: pointer; display: grid; place-items: center; border-radius: 50%; }
  .m { margin: 0; min-block-size: 19px; font-size: var(--fs-sub); line-height: 19px; color: var(--muted); }
  .m.err { color: var(--danger); }
</style>
