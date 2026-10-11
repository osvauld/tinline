<script lang="ts">
  let { count, muted = false, label }: { count: number; muted?: boolean; label?: string } = $props();
</script>

{#if count > 0}
  <span class="t-badge" class:muted aria-label={label ?? `${count} unread`}>{count > 99 ? '99+' : count}</span>
{/if}
