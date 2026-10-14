<script lang="ts">
  /* A stand-in for the phone's keyboard (in-call lab, `?ic=kbd`): the size and place of Gboard, so the strip and the
     composer can be seen where they really end up while typing. Not interactive; the composer keeps focus. */
  const rows = ['qwertyuiop', 'asdfghjkl', 'zxcvbnm'];
</script>

<div class="kbd" aria-hidden="true" onpointerdown={(e) => e.preventDefault()}>
  {#each rows as r, i (r)}
    <div class="r" style:padding-inline="{i === 1 ? 18 : i === 2 ? 44 : 4}px">{#each r as k (k)}<span>{k}</span>{/each}</div>
  {/each}
  <div class="r"><span class="w">?123</span><span class="sp">space</span><span class="w">↵</span></div>
</div>

<style>
  .kbd { flex: none; block-size: 268px; display: grid; align-content: start; gap: 10px; padding: 12px 4px 18px; background: color-mix(in srgb, var(--ink) 7%, var(--bg)); border-block-start: 1px solid var(--hairline); }
  .r { display: flex; gap: 5px; justify-content: center; }
  span { flex: 1; max-inline-size: 34px; block-size: 44px; display: grid; place-items: center; border-radius: 8px; background: var(--surface); color: var(--ink); font-size: 18px; }
  .w { max-inline-size: 64px; font-size: 13px; }
  .sp { max-inline-size: none; flex: 4; font-size: 13px; color: var(--muted); }
</style>
