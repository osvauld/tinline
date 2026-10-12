<script lang="ts">
  /* A permission problem, said calmly: what is affected, what still works, one button. It opens and closes by animating
     its height (never a snap), and is absent entirely when there is nothing to fix. */
  import { fade } from 'svelte/transition';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import { dur } from '../../lib/fx/motion';
  import { motion } from '../../lib/fx/reduced.svelte';
  import { fix, type Problem } from './perms';
  let { p, onopen }: { p: Problem; onopen?: () => void } = $props();
  const ms = $derived(motion.reduced ? 0 : dur('crossfade'));
  /* Height animates from 0 to its content (a measured transition, so it works for any copy length). */
  function grow(node: HTMLElement, { duration }: { duration: number }) {
    const h = node.offsetHeight;
    return { duration, css: (t: number) => `overflow:hidden;height:${t * h}px;opacity:${t};margin-block-end:${t * 12}px` };
  }
</script>

<div class="pb" role="status" data-perm-banner={p.key} transition:grow={{ duration: ms }}>
  <div class="in">
    <span class="ic"><Icon name={p.icon} size={20} /></span>
    <div class="tx"><b>{p.title}</b><p>{p.body}</p>
      <div class="acts"><Btn variant="outline" data-fix={p.key} onclick={() => (onopen ? onopen() : fix(p.key))}>{onopen ? 'See details' : p.fix}</Btn></div></div>
  </div>
</div>

<style>
  .pb { margin-block-end: 12px; }
  .in { display: flex; gap: 12px; padding: 12px 14px; border-radius: var(--r-ctl); background: var(--warn-bg); color: var(--warn-ink); }
  .ic { flex: none; margin-block-start: 1px; }
  .tx { display: grid; gap: 4px; min-inline-size: 0; }
  .tx b { font-weight: 650; line-height: 20px; }
  .tx p { margin: 0; font-size: var(--fs-sub); line-height: 19px; }
  .acts { margin-block-start: 6px; }
  .acts :global(.b) { background: transparent; border-color: currentColor; color: inherit; min-block-size: 36px; }
</style>
