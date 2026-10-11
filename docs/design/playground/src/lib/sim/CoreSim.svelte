<script lang="ts">
  import Switch from '../ui/Switch.svelte';
  import { db, resetDb } from '../model/store.svelte';
  import { clock, T } from '../model/clock.svelte';
  import { motion } from '../fx/reduced.svelte';
  import { setTimeScale } from '../fx/motion';
  import { audio } from '../fx/audio';
  import { nav } from '../nav/router.svelte';

  let sound = $state(!audio.quiet);
  let slow = $state(false);
</script>

<div class="rows">
  <label class="row"><span>This device online</span><Switch label="This device online" bind:checked={db.env.networkOnline} /></label>
  <label class="row"><span>Group hosts online</span><Switch label="Group hosts online" bind:checked={db.env.hostsOnline} /></label>
  <label class="row"><span>Next send fails</span><Switch label="Next send fails" bind:checked={db.env.failNextSend} /></label>
  <label class="row"><span>Sound (off = Quiet)</span><Switch label="Sound" bind:checked={sound} onchange={(v) => audio.setQuiet(!v)} /></label>
  <label class="row"><span>Slow motion ×4</span><Switch label="Slow motion" bind:checked={slow} onchange={(v) => setTimeScale(v ? 4 : 1)} /></label>
  <div class="row"><span>Motion</span>
    <select bind:value={motion.pref} aria-label="Motion preference">
      <option value="system">System</option><option value="full">Full</option><option value="reduce">Reduce</option>
    </select>
  </div>
  <div class="row"><span>Clock</span>
    <span class="btns">
      <button type="button" onclick={() => clock.jump(T.HOUR)}>+1 h</button>
      <button type="button" onclick={() => clock.jump(T.DAY)}>+1 day</button>
    </span>
  </div>
  <button type="button" class="reset" onclick={() => { resetDb(); nav.reset(); }}>Reset all data</button>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; font-size: var(--fs-sub); }
  .btns { display: flex; gap: 6px; }
  button:not([role]) { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  select { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); }
  .reset { margin-top: 8px; }
</style>
