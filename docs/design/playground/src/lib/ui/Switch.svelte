<script lang="ts">
  let { checked = $bindable(false), label, onchange }: { checked?: boolean; label: string; onchange?: (v: boolean) => void } = $props();
</script>

<button
  type="button" role="switch" aria-checked={checked} aria-label={label} class="sw"
  onclick={() => { checked = !checked; onchange?.(checked); }}
><span class="knob"></span></button>

<style>
  .sw { position: relative; width: 44px; height: 26px; border-radius: 13px; border: 0; padding: 0; flex: none; cursor: pointer;
    background: var(--line); transition: background-color var(--dur-switch) var(--ease-out); }
  .sw[aria-checked='true'] { background: var(--accent); }
  .knob { position: absolute; inset-block-start: 3px; inset-inline-start: 3px; width: 20px; height: 20px; border-radius: 50%;
    background: var(--surface); box-shadow: var(--shadow-lift); transition: translate var(--dur-switch) var(--ease-out); }
  .sw[aria-checked='true'] .knob { translate: 18px 0; }
  :global([dir='rtl']) .sw[aria-checked='true'] .knob { translate: -18px 0; }
</style>
