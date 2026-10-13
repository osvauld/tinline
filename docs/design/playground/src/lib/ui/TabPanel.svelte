<script lang="ts">
  /* Content for one peer destination (a tab). Switching keys cross-fades through, the old and new
     panels stacked in the same grid cell so nothing reflows, and each key keeps its own scroll position. */
  import type { Snippet } from 'svelte';
  import { fadeThroughIn, fadeThroughOut } from '../fx/transitions';

  let { key, children }: { key: string; children: Snippet } = $props();
</script>

<script lang="ts" module>
  const scrollMemory = new Map<string, number>();
  /** Remember/restore scrollTop per key for the scroll container inside a panel. */
  export function rememberScroll(node: HTMLElement, key: string) {
    node.scrollTop = scrollMemory.get(key) ?? 0;
    const save = () => scrollMemory.set(key, node.scrollTop);
    node.addEventListener('scroll', save, { passive: true });
    return { destroy() { node.removeEventListener('scroll', save); } };
  }
</script>

<div class="stack">
  {#key key}
    <div class="panel" in:fadeThroughIn out:fadeThroughOut>{@render children()}</div>
  {/key}
</div>

<style>
  .stack { display: grid; min-height: 0; flex: 1; }
  .panel { grid-area: 1 / 1; min-height: 0; display: flex; flex-direction: column; transform-origin: 50% 0; }
</style>
