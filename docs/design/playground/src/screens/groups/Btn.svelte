<script lang="ts">
  import type { Snippet } from 'svelte';
  let { variant = 'outline', onclick, disabled = false, full = false, label, children, ...rest }: {
    variant?: 'primary' | 'outline' | 'quiet' | 'danger' | 'dangerquiet'; onclick?: (e: MouseEvent) => void; disabled?: boolean; full?: boolean;
    label?: string; children: Snippet; [k: string]: unknown;
  } = $props();
</script>

<button type="button" class="b press {variant}" class:full {disabled} aria-label={label} {onclick} {...rest}>{@render children()}</button>

<style>
  .b { min-block-size: var(--btn); display: inline-flex; align-items: center; justify-content: center; gap: 8px; padding: 0 18px; border-radius: 22px; border: 1px solid var(--line);
    background: var(--surface); font-weight: 600; font-size: var(--fs-body); line-height: 20px; color: var(--ink); cursor: pointer; white-space: nowrap; }
  .b.full { inline-size: 100%; }
  .b:hover:not(:disabled) { background: var(--surface-2); }
  .b:disabled { opacity: .45; cursor: default; }
  .primary { background: var(--accent); border-color: transparent; color: var(--on-accent); }
  .primary:hover:not(:disabled) { background: var(--accent); filter: brightness(1.08); }
  .danger { background: var(--danger-btn); border-color: transparent; color: var(--on-danger-btn); }
  .danger:hover:not(:disabled) { background: var(--danger-btn); filter: brightness(1.08); }
  .quiet { background: transparent; border-color: transparent; }
  .dangerquiet { background: transparent; border-color: transparent; color: var(--danger); }
</style>
