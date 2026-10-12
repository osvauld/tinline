<script lang="ts">
  import Btn from './Btn.svelte';
  import Notice from './Notice.svelte';
  import Qr from './Qr.svelte';
  import StepShell from './StepShell.svelte';
  import { gate, LINK_FAIL } from '../../lib/model/actions/gate.svelte';
  const st = $derived(gate.linkState);
  const left = $derived(Math.ceil(gate.linkLeftMs / 1000));
  const mmss = $derived(Math.floor(left / 60) + ':' + String(left % 60).padStart(2, '0'));
  const title = $derived(st.phase === 'scanned' ? 'Same code on both?' : st.phase === 'approved' ? 'Linking…' : st.phase === 'failed' ? 'Couldn’t link' : 'Link to your account');
  const again = () => gate.beginLinkSession();
  const toRestore = () => { gate.flow = 'restore'; gate.history = ['welcome', 'choose']; gate.go('restore'); };
</script>

<StepShell {title}
  lead={st.phase === 'showing' ? 'On a device that already has your account, open Settings › Link a device and scan this.'
    : st.phase === 'scanned' ? 'Check that ' + (gate.peer || 'your other device') + ' shows this code, then approve there.'
    : st.phase === 'approved' ? 'Receiving your account. Keep both devices open.' : undefined}
  center anchor>
  {#if st.phase === 'showing'}
    <div class="qr"><Qr seed={gate.link.nonce} size={208} /></div>
    <p class="exp" role="timer">Expires in {mmss} · works once</p>
    <small class="hint">Anyone who can see this code in the next few minutes could try to use it. Show it only to your own device.</small>
  {:else if st.phase === 'scanned'}
    <p class="code" aria-label="Link code {gate.linkCode.replace(' ', ' ')}">{gate.linkCode}</p>
    <Notice kind="warn">Different code, or you didn’t start this? Cancel. Someone else may have scanned the QR.</Notice>
    <p class="wait"><span class="dot" aria-hidden="true"></span>Waiting for approval…</p>
  {:else if st.phase === 'approved'}
    <div class="busy" aria-hidden="true"><span class="spin"></span></div>
  {:else}
    <div class="fail"><Notice kind="error">{LINK_FAIL[st.reason ?? 'expired']}</Notice></div>
  {/if}
  {#snippet foot()}
    {#if st.phase === 'failed'}
      <Btn onclick={again}>Try again</Btn>
      <Btn kind="text" onclick={toRestore}>Restore with recovery phrase instead</Btn>
    {:else if st.phase === 'scanned'}
      <Btn kind="secondary" onclick={() => gate.simCancel()}>Cancel</Btn>
    {:else if st.phase === 'showing'}
      <Btn kind="text" onclick={again}>New code</Btn>
    {/if}
  {/snippet}
</StepShell>

<style>
  .qr { padding: 12px; background: #fff; border-radius: 20px; border: 1px solid var(--line); }
  .exp { margin: 0; font-variant-numeric: tabular-nums; color: var(--muted); font-size: var(--fs-sub); }
  .hint { color: var(--muted); font-size: var(--fs-sub); line-height: 18px; max-inline-size: 36ch; }
  .code { margin: 8px 0; font: 600 44px/52px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; letter-spacing: .12em; }
  .wait { display: inline-flex; align-items: center; gap: 10px; margin: 0; color: var(--muted); font-size: var(--fs-sub); }
  .dot { inline-size: 8px; block-size: 8px; border-radius: 50%; background: var(--accent); animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: .25; } }
  .busy { display: grid; place-items: center; padding-block: 40px; }
  .spin { inline-size: 40px; block-size: 40px; border-radius: 50%; border: 4px solid var(--line); border-block-start-color: var(--accent); animation: spin 0.9s linear infinite; }
  @keyframes spin { to { rotate: 360deg; } }
  .fail { inline-size: 100%; text-align: start; }
</style>
