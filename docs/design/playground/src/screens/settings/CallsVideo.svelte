<script lang="ts">
  /* Settings > Video (2026-10-15). The video ladder is automatic; "Sharper video" lets it climb to HD when the
     connection and the device allow (phone up to 540p, computer 720p). The computer picks a camera, USB ones included.
     A choice applies the next time the camera starts, and the page says so once. */
  import Switch from '../../lib/ui/Switch.svelte';
  import Icon from '../../lib/ui/Icon.svelte';
  import { device } from '../../lib/model/device.svelte';
  import { prefs } from '../../lib/model/prefs.svelte';
  const android = $derived(prefs.form === 'android');
</script>

<div class="pf-scroll" data-calls-video>
<div class="pf-col">
  <section class="pf-sec first" aria-labelledby="vq-h">
    <h2 id="vq-h">Video quality</h2>
    <div class="pf-card">
      <label class="pf-act sw"><Icon name="video" size={20} />
        <span class="t"><b>Sharper video</b><small>{device.hd ? `Up to ${android ? '540p' : '720p'} when the connection allows. Uses more data and battery.` : 'Adjusts by itself for smooth calls.'}</small></span>
        <Switch label="Sharper video" bind:checked={device.hd} /></label>
    </div>

    {#if android}<p class="pf-note">Changes apply the next time your camera starts.</p>{/if}
  </section>

  {#if !android}
    <section class="pf-sec" aria-labelledby="cam-h">
      <h2 id="cam-h">Camera</h2>
      <div class="pf-card" role="radiogroup" aria-label="Camera">
        {#each device.cameras as c (c.id)}
          <div class="pf-act opt">
            <button type="button" role="radio" class="pick press" aria-checked={device.camera === c.id} onclick={() => (device.camera = c.id)}>
              <span class="radio" aria-hidden="true"></span>
              <span class="t"><b>{c.label}</b>{#if c.usb}<small>USB</small>{/if}</span>
            </button>
          </div>
        {/each}
      </div>
      <p class="pf-note">Changes apply the next time your camera starts.</p>
    </section>
  {/if}
</div>
</div>

<style>
  .first { margin-block-start: 16px; }
  .sw { cursor: default; }
  .opt { padding: 0; }
  .pick { display: flex; align-items: center; gap: 14px; inline-size: 100%; min-block-size: 56px; padding: 8px 16px; border: 0; background: none; color: inherit; font: inherit; text-align: start; cursor: pointer; }
  .pick:hover { background: var(--surface-2); }
  .radio { flex: none; inline-size: 22px; block-size: 22px; border-radius: 50%; box-shadow: inset 0 0 0 2px var(--line); display: grid; place-items: center; transition: box-shadow var(--dur-switch) var(--ease-out); }
  .radio::after { content: ''; inline-size: 12px; block-size: 12px; border-radius: 50%; background: var(--accent); transform: scale(0); transition: transform var(--dur-switch) var(--ease-out); }
  [aria-checked='true'] .radio { box-shadow: inset 0 0 0 2px var(--accent); }
  [aria-checked='true'] .radio::after { transform: none; }
</style>
