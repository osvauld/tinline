<script lang="ts">
  /* Link a new device, the EXISTING device's side (decided model: the NEW device shows the QR and the code, this one scans or
     takes the code, both show six digits, a person confirms they match, then passphrase approval, then the phrase moves).
     Every failure says what was and was not shared. Nothing here is real: no camera, no phrase, no network. */
  import { onDestroy } from 'svelte';
  import { fade } from 'svelte/transition';
  import Icon from '../../lib/ui/Icon.svelte';
  import Btn from '../groups/Btn.svelte';
  import PassField from '../people/PassField.svelte';
  import { nav } from '../../lib/nav/router.svelte';
  import { dur } from '../../lib/fx/motion';
  import { motion } from '../../lib/fx/reduced.svelte';
  import { gateSim } from '../../lib/model/actions/gate.svelte';
  import { LINK_FAIL, TRANSFER_STEPS, link, pp } from '../../lib/model/actions/people.svelte';

  let text = $state(''), pass = $state(''), passErr = $state('');
  const ms = $derived(motion.reduced ? 0 : dur('crossfade'));
  link.begin();
  onDestroy(() => (link.step === 'transfer' ? link.cancel() : link.reset()));

  function approve() {
    if (gateSim.hasPassphrase && pass !== gateSim.passphrase) { passErr = 'That passphrase isn’t right.'; return; }
    link.authorized();
  }
  const again = () => { text = ''; pass = ''; passErr = ''; link.begin(); };
  const fail = $derived(LINK_FAIL[link.fail]);
  const cam = $derived(pp.perms.camera);
</script>

<div class="pf-scroll" data-link-device>
<div class="pf-col">
  {#key link.step}
  <div class="step" in:fade={{ duration: ms }}>
    {#if link.step === 'scan'}
      <h1>Scan the code on your new device</h1>
      <ol class="how">
        <li>On the new device, open Tinline and choose <b>Link to an existing device</b>.</li>
        <li>It shows a QR code and a short code. They work for 5 minutes, once.</li>
      </ol>
      {#if cam === 'granted'}
        <div class="finder" class:on={link.scanning} data-viewfinder>
          {#if link.scanning}<i class="c tl"></i><i class="c tr"></i><i class="c bl"></i><i class="c br"></i><i class="sweep"></i>
          {:else}<Icon name="camera" size={30} /><Btn variant="primary" data-scan onclick={() => link.scan()}>Scan QR code</Btn>{/if}
        </div>
        <p class="live" role="status">{link.scanning ? 'Looking for the code…' : ''}</p>
      {:else}
        <div class="denied" data-camera-denied><Icon name="camera" size={22} /><span><b>Camera access is off.</b> Allow it in system settings, or type the code below.</span>
          <Btn variant="outline" onclick={() => (pp.perms.camera = 'granted')}>Open settings</Btn></div>
      {/if}
      <form class="typed" onsubmit={(e) => { e.preventDefault(); link.typed(text); }}>
        <label for="lk-code">Or type the code</label>
        <div class="line"><input id="lk-code" bind:value={text} placeholder="OSVL1-7K2M-Q4RX" autocomplete="off" autocapitalize="characters" spellcheck="false" oninput={() => (link.error = '')}
          aria-invalid={link.error ? 'true' : undefined} aria-describedby="lk-err" /><Btn variant="outline" type="submit" disabled={link.scanning}>Continue</Btn></div>
        <p class="err" id="lk-err" role="alert" data-code-error>{link.error}</p>
      </form>

    {:else if link.step === 'verify'}
      <h1>Do the codes match?</h1>
      <p class="lead"><b>{link.name}</b> wants to join your account. It should be showing the same six digits as below.</p>
      <p class="digits" aria-label="Confirmation code {link.confirmCode.replace(' ', ' ')}" data-confirm-code>{link.confirmCode}</p>
      <p class="small">Compare them yourself. If they differ, someone else may have scanned the code first.</p>
      <div class="acts"><Btn variant="outline" onclick={() => link.verdict(false)}>They don’t match</Btn><Btn variant="primary" data-match onclick={() => link.verdict(true)}>They match</Btn></div>

    {:else if link.step === 'auth'}
      <h1>Approve {link.name}?</h1>
      <p class="warn" data-approve-warning><Icon name="warning" size={18} /><span>Approving sends your <b>recovery phrase</b> to {link.name}. It becomes a full copy of you: it can read your chats and act as you. Approve only a device you hold.</span></p>
      {#if gateSim.hasPassphrase}
        <form class="auth" onsubmit={(e) => { e.preventDefault(); approve(); }}>
          <PassField id="lk-pass" bind:value={pass} error={passErr} autofocus label="Your passphrase" oninput={() => (passErr = '')} />
          <div class="acts"><Btn variant="quiet" onclick={() => link.cancel()}>Don’t link</Btn><Btn variant="primary" type="submit" data-approve>Approve</Btn></div>
        </form>
      {:else}
        <div class="acts"><Btn variant="quiet" onclick={() => link.cancel()}>Don’t link</Btn><Btn variant="primary" data-approve onclick={approve}>Approve</Btn></div>
      {/if}

    {:else if link.step === 'transfer'}
      <h1>Linking {link.name}</h1>
      <ol class="steps" aria-live="polite" data-transfer>
        {#each TRANSFER_STEPS as s, i (s)}
          {@const st = i < link.tstep ? 'done' : i === link.tstep ? 'now' : 'wait'}
          <li class={st}><span class="mark">{#if st === 'done'}<Icon name="check" size={16} />{:else if st === 'now'}<i class="spin"></i>{/if}</span>
            <span class="tx">{s}</span><small>{st === 'done' ? 'Done' : st === 'now' ? 'Working' : 'Waiting'}</small></li>
        {/each}
      </ol>
      <p class="small">Keep both devices open until this finishes.</p>
      <div class="acts"><Btn variant="quiet" onclick={() => link.cancel()}>Cancel</Btn></div>

    {:else if link.step === 'done'}
      <div class="ok" data-link-done><span class="tick"><Icon name="check" size={28} /></span>
        <h1>{link.name} is linked</h1>
        <p class="lead">It syncs when both devices are online at the same time. Contacts you haven’t blocked get your updated device list when they’re next online.</p></div>
      <div class="acts"><Btn variant="primary" data-done onclick={() => nav.back()}>Done</Btn></div>

    {:else}
      <div class="bad" data-link-failed={link.fail}><span class="x"><Icon name="info" size={26} /></span>
        <h1>{fail.title}</h1><p class="lead">{fail.body}</p></div>
      <div class="acts"><Btn variant="quiet" onclick={() => nav.back()}>Close</Btn><Btn variant="primary" data-again onclick={again}>Try again</Btn></div>
    {/if}
  </div>
  {/key}
</div>
</div>

<style>
  .step { padding: 20px 20px 0; display: grid; grid-template-columns: minmax(0, 1fr); gap: 12px; justify-items: stretch; }
  h1 { margin: 0; font-size: 22px; line-height: 28px; font-weight: 650; text-wrap: balance; }
  .lead { margin: 0; line-height: 21px; }
  .small { margin: 0; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .how { margin: 0; padding-inline-start: 20px; display: grid; gap: 4px; color: var(--muted); font-size: var(--fs-sub); line-height: 19px; }
  .how b { color: var(--ink); font-weight: 600; }
  .finder { position: relative; justify-self: center; inline-size: 220px; block-size: 220px; border-radius: 20px; background: #0e1513; overflow: hidden; display: grid; place-content: center; justify-items: center; gap: 14px; color: #9db0aa; }
  .c { position: absolute; inline-size: 34px; block-size: 34px; border: 3px solid #fff; opacity: .9; }
  .tl { inset: 16px auto auto 16px; border-width: 3px 0 0 3px; border-top-left-radius: 10px; }
  .tr { inset: 16px 16px auto auto; border-width: 3px 3px 0 0; border-top-right-radius: 10px; }
  .bl { inset: auto auto 16px 16px; border-width: 0 0 3px 3px; border-bottom-left-radius: 10px; }
  .br { inset: auto 16px 16px auto; border-width: 0 3px 3px 0; border-bottom-right-radius: 10px; }
  .sweep { position: absolute; inset-inline: 22px; inset-block-start: 22px; block-size: 2px; border-radius: 1px; background: var(--accent); box-shadow: 0 0 12px var(--accent); animation: sweep 1.3s ease-in-out infinite alternate; }
  :global(:root[data-motion='reduce']) .sweep { animation: none; inset-block-start: 50%; }
  @keyframes sweep { to { inset-block-start: calc(100% - 24px); } }
  .live { margin: -4px 0 0; min-block-size: 19px; text-align: center; color: var(--muted); font-size: var(--fs-sub); }
  .denied { display: grid; grid-template-columns: auto 1fr; gap: 6px 10px; align-items: start; padding: 12px; border-radius: var(--r-ctl); background: var(--surface-2); font-size: var(--fs-sub); line-height: 19px; color: var(--muted); }
  .denied b { color: var(--ink); } .denied :global(.b) { grid-column: 1 / -1; justify-self: start; }
  .typed { display: grid; grid-template-columns: minmax(0, 1fr); gap: 6px; }
  .typed label { font-weight: 600; font-size: var(--fs-sub); }
  .line { display: flex; gap: 8px; }
  .line input { flex: 1; min-inline-size: 0; min-block-size: var(--btn); padding: 0 14px; border-radius: var(--r-ctl); border: 1px solid var(--line); background: var(--surface); font: 500 var(--fs-body) ui-monospace, Menlo, Consolas, monospace; letter-spacing: .04em; color: var(--ink); }
  .line input[aria-invalid='true'] { border-color: var(--danger); }
  .err { margin: 0; min-block-size: 38px; color: var(--danger); font-size: var(--fs-sub); line-height: 19px; }
  .digits { margin: 4px 0; text-align: center; font: 700 44px/52px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; letter-spacing: .12em; font-variant-numeric: tabular-nums; }
  .acts { display: flex; gap: 8px; justify-content: flex-end; margin-block-start: 6px; }
  .warn { display: flex; gap: 10px; align-items: flex-start; margin: 0; padding: 12px; border-radius: 12px; background: var(--warn-bg); color: var(--warn-ink); line-height: 20px; }
  .warn > :global(svg) { flex: none; margin-block-start: 1px; }
  .auth { display: grid; gap: 6px; }
  .steps { list-style: none; margin: 0; padding: 0; display: grid; gap: 2px; }
  .steps li { display: flex; align-items: center; gap: 12px; min-block-size: 48px; color: var(--muted); }
  .steps li.now, .steps li.done { color: var(--ink); }
  .steps .tx { flex: 1; } .steps small { font-size: var(--fs-sub); min-inline-size: 52px; text-align: end; color: var(--muted); }
  .mark { inline-size: 24px; block-size: 24px; border-radius: 50%; display: grid; place-items: center; flex: none; box-shadow: inset 0 0 0 1.5px var(--line); }
  .done .mark { background: var(--accent); color: var(--on-accent); box-shadow: none; }
  .spin { inline-size: 14px; block-size: 14px; border-radius: 50%; border: 2px solid var(--accent); border-inline-end-color: transparent; animation: rot .9s linear infinite; }
  :global(:root[data-motion='reduce']) .spin { animation: none; border-inline-end-color: var(--accent); opacity: .6; }
  @keyframes rot { to { transform: rotate(360deg); } }
  .ok, .bad { display: grid; justify-items: center; text-align: center; gap: 10px; padding-block-start: 24px; }
  .tick, .x { inline-size: 56px; block-size: 56px; border-radius: 50%; display: grid; place-items: center; background: var(--accent-soft); color: var(--accent-ink); }
  .x { background: var(--warn-bg); color: var(--warn-ink); }
  .ok .lead, .bad .lead { color: var(--muted); max-inline-size: 38ch; font-size: var(--fs-body); text-wrap: balance; }
</style>
