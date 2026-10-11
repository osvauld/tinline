<script lang="ts">
  let { title, step }: { title: string; step: string } = $props();
</script>
<div class="ph"><h2>{title}</h2><p>Not ported yet — PLAN.md step {step}.</p></div>
<style>
  .ph { display: grid; place-content: center; text-align: center; height: 100%; color: var(--muted); padding: 24px; }
  h2 { color: var(--ink); font-size: var(--fs-title); margin: 0 0 4px; }
  p { margin: 0; font-size: var(--fs-sub); }
</style>
