<script lang="ts">
  import Switch from '../ui/Switch.svelte';
  import { db } from '../model/store.svelte';
  import { gate, gateSim, LINK_FAIL, DEMO_PHRASE, type Step, type LinkReason } from '../model/actions/gate.svelte';

  const STEPS: [Step, string][] = [
    ['welcome', 'Welcome'], ['choose', 'I already have an account'],
    ['name', 'Create: name'], ['pass', 'Create: passphrase'], ['phrase', 'Create: recovery phrase'], ['confirm', 'Create: confirm words'],
    ['device', 'Create: device name'], ['terms', 'Terms'], ['perms', 'Permissions'], ['done', 'Done'],
    ['linkName', 'Link: device name'], ['link', 'Link: QR + code'], ['linkPass', 'Link: protect'],
    ['restore', 'Restore: words'], ['restoreName', 'Restore: name'], ['restorePass', 'Restore: passphrase'], ['forgotPass', 'Forgot: new passphrase'],
    ['unlock', 'Unlock'], ['keylost', 'Key lost'], ['savefailed', 'Save failed'], ['savepass', 'Save failed: add passphrase'],
  ];
  let reason = $state<LinkReason>('timeout');
  const link = $derived(gate.linkState);
  const onLink = $derived(gate.step === 'link');
</script>

<div class="rows">
  <div class="row"><span>Jump to step</span>
    <select aria-label="Jump to step" value={gate.step} onchange={(e) => gate.jump(e.currentTarget.value as Step)}>
      {#each STEPS as [s, l] (s)}<option value={s}>{l}</option>{/each}
    </select>
  </div>
  <p class="note">Now: <code>{gate.step}</code> · flow <code>{gate.flow}</code> · app state <code>{db.account.gate}</code></p>

  <h4>Link (this is the new device)</h4>
  <p class="note">Link state: <code>{link.phase}{link.reason ? ' / ' + link.reason : ''}</code>{onLink ? '' : ' (open the Link step)'}</p>
  <div class="btns">
    <button type="button" disabled={!onLink || link.phase !== 'showing'} onclick={() => gate.simScan()}>Other device scans</button>
    <button type="button" disabled={!onLink || link.phase !== 'scanned'} onclick={() => gate.simApprove()}>Other device approves</button>
    <button type="button" disabled={!onLink} onclick={() => gate.simExpire()}>Code expires</button>
    <button type="button" disabled={!onLink} onclick={() => gate.simCancel()}>Other device cancels</button>
  </div>
  <div class="row"><span>Fail with</span>
    <span class="btns"><select aria-label="Failure reason" bind:value={reason}>{#each Object.keys(LINK_FAIL).filter((k) => k !== 'expired' && k !== 'cancelled') as k (k)}<option value={k}>{k}</option>{/each}</select>
    <button type="button" disabled={!onLink} onclick={() => gate.simFail(reason)}>Fail</button></span>
  </div>

  <h4>Create / restore</h4>
  <label class="row"><span>Storage refuses the key (save fails)</span><Switch label="Save fails" bind:checked={gateSim.failSave} /></label>
  <label class="row"><span>Phrase restore fails</span><Switch label="Phrase restore fails" bind:checked={gateSim.failRestore} /></label>
  <div class="row"><span>OS permission prompt</span>
    <select aria-label="Permission result" bind:value={gateSim.permResult}><option value="grant">Allow</option><option value="deny">Deny</option></select>
  </div>
  <div class="btns"><button type="button" onclick={() => (gate.words = [...DEMO_PHRASE])}>Fill demo phrase (restore)</button></div>
  <p class="note">Demo recovery phrase: <code>{DEMO_PHRASE.slice(0, 4).join(' ')} …</code> Create always shows this phrase; confirm asks words 4, 11, 19.</p>

  <h4>Unlock</h4>
  <label class="row"><span>Account has a passphrase (off = key lost)</span><Switch label="Has passphrase" bind:checked={gateSim.hasPassphrase}
      onchange={() => { if (gate.step === 'unlock' || gate.step === 'keylost') gate.step = gateSim.hasPassphrase ? 'unlock' : 'keylost'; }} /></label>
  <div class="row"><span>Passphrase that unlocks</span><input aria-label="Accepted passphrase" bind:value={gateSim.passphrase} /></div>
  <div class="row"><span>Wrong tries before lockout (N)</span><input type="number" min="1" max="20" aria-label="Tries before lockout" bind:value={gateSim.maxTries} /></div>
  <div class="row"><span>Lockout wait (seconds)</span><input type="number" min="1" max="600" aria-label="Lockout seconds" bind:value={gateSim.lockSeconds} /></div>
  <div class="row"><span>Wrong tries so far</span><span>{gate.tries}</span></div>
  <div class="btns"><button type="button" onclick={() => { gate.lockedUntil = 0; gate.tries = 0; }}>Clear lockout</button></div>
  <div class="row"><span>Simulated Argon2 time (ms)</span><input type="number" min="0" max="5000" step="100" aria-label="Key derivation time" bind:value={gateSim.kdfMs} /></div>
  <p class="note">Remembered on this phone: <code>{String(gateSim.remembered)}</code></p>

  <h4>Proposed / undecided (not app copy)</h4>
  <ul class="notes">
    <li><b>Proposed:</b> lockout after N wrong passphrases (default N={gateSim.maxTries}, wait {gateSim.lockSeconds}s). The real app never locks out; it is only deliberately slow (Argon2).</li>
    <li><b>Proposed:</b> an explicit "Remember on this phone" switch on Unlock (Android). Real app remembers via the Keystore without asking.</li>
    <li><b>Proposed:</b> "Check your copy" step (3 typed words). Real onboarding has no confirm step.</li>
    <li><b>Proposed:</b> passphrase confirm field and strength bars. Real has one field and a "why" line.</li>
    <li><b>Proposed:</b> blurred phrase until revealed; real shows the words at once (screenshots blocked on Android). Copy kept but discouraged.</li>
    <li><b>Proposed:</b> permissions step also lists Camera (optional) and has "Not now"; real skip is "Skip for now".</li>
    <li><b>Proposed:</b> restore/link end on one shared "Done" step instead of the real Restored / Syncing screens.</li>
    <li><b>Undecided:</b> restoring a phrase when that account is already on this device (real: "Already on this phone" dialog; not drawn). Restore-replaces vs adds is still open (audit item 9).</li>
    <li><b>Undecided:</b> a QR the NEW device shows is the decided model; the existing-device scan/approve screens belong to Devices (step 8).</li>
    <li>Terms/Privacy links are placeholders. The 6-digit code and QR are fake.</li>
  </ul>
</div>

<style>
  .rows { display: grid; gap: 6px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; font-size: var(--fs-sub); }
  h4 { margin: 12px 0 0; font-size: var(--fs-sub); }
  .note { margin: 0; font-size: var(--fs-sub); color: var(--muted); }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  button:not([role]) { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  button:disabled { opacity: .45; cursor: default; }
  select, input { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); max-width: 170px; }
  input[type='number'] { width: 80px; }
  .notes { margin: 0; padding-inline-start: 18px; display: grid; gap: 4px; font-size: var(--fs-sub); color: var(--muted); }
  code { font-size: 12px; }
</style>
