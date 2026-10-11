<script lang="ts">
  import { tick } from 'svelte';
  import Btn from './Btn.svelte';
  import Notice from './Notice.svelte';
  import StepShell from './StepShell.svelte';
  import { gate } from '../../lib/model/actions/gate.svelte';
  import { WORDSET, suggest } from './wordlist';

  let active = $state(-1);
  let tried = $state(false);
  let seen = $state<boolean[]>(Array(24).fill(false)); // field was left once
  let inputs: HTMLInputElement[] = [];

  const w = (i: number) => gate.words[i];
  const prefixOk = (v: string) => suggest(v, 1).length > 0;
  const bad = (i: number) => {
    const v = w(i);
    if (!v || WORDSET.has(v)) return false;
    return seen[i] || tried || !prefixOk(v);
  };
  const badList = $derived(gate.words.map((_, i) => i).filter(bad));
  const filled = $derived(gate.words.filter(Boolean).length);
  const sugg = $derived(active >= 0 && !WORDSET.has(w(active)) ? suggest(w(active), 4) : []);

  function setWord(i: number, raw: string) {
    const toks = raw.toLowerCase().split(/[^a-z]+/).filter(Boolean);
    if (toks.length > 1) {                       // pasted a run of words: spread them from this field
      toks.forEach((t, k) => { if (i + k < 24) gate.words[i + k] = t; });
      const last = Math.min(23, i + toks.length - 1);
      tick().then(() => inputs[last]?.focus());
    } else gate.words[i] = (toks[0] ?? '').slice(0, 12);
    gate.error = null;
  }
  function key(e: KeyboardEvent, i: number) {
    if (e.key === ' ' || e.key === 'Enter') {
      e.preventDefault();
      if (w(i) && !WORDSET.has(w(i)) && suggest(w(i), 1).length === 1) gate.words[i] = suggest(w(i), 1)[0];
      if (i < 23) inputs[i + 1]?.focus(); else submit();
    } else if (e.key === 'Backspace' && !w(i) && i > 0) { e.preventDefault(); inputs[i - 1]?.focus(); }
  }
  function pick(s: string) { if (active < 0) return; gate.words[active] = s; const n = active; if (n < 23) inputs[n + 1]?.focus(); }
  async function paste() {
    try { setWord(0, await navigator.clipboard.readText()); } catch { gate.error = 'Couldn’t read the clipboard. Paste into the first box instead.'; }
  }
  function submit() {
    tried = true;
    if (badList.length || filled < 24) {
      const first = badList[0] ?? gate.words.findIndex((x) => !x);
      inputs[first]?.focus();
      return;
    }
    gate.go(gate.flow === 'forgot' ? 'forgotPass' : 'restoreName');
  }
  const msg = $derived(badList.length
    ? `Word ${badList[0] + 1}${badList.length > 1 ? ' and ' + (badList.length - 1) + ' more' : ''} ${badList.length > 1 ? 'aren’t' : 'isn’t'} in the recovery word list. Check the spelling.`
    : tried && filled < 24 ? `${24 - filled} words are still missing.` : (gate.error ?? ''));
</script>

<StepShell title="Restore account" lead="Type your 24 words in order, or paste them all into the first box. They stay on this {gate.noun}." form onsubmit={submit}>
  <Notice icon="info" kind="warn">{gate.flow === 'forgot' ? 'Restoring keeps the same identity: use the recovery phrase of this account. You’ll set a new passphrase.' : 'Next, you’ll choose a passphrase for this account.'}</Notice>
  <div class="tools"><Btn kind="text" onclick={paste}>Paste from clipboard</Btn><span class="cnt" aria-live="polite">{filled} of 24</span></div>
  <div class="grid" role="group" aria-label="Recovery phrase words">
    {#each gate.words as _, i (i)}
      <label class="cell" class:bad={bad(i)} class:ok={w(i) && WORDSET.has(w(i))}>
        <i aria-hidden="true">{i + 1}</i>
        <input aria-label="Word {i + 1}" bind:this={inputs[i]} value={w(i)} data-autofocus={i === 0 || undefined} autocomplete="off" autocapitalize="off" spellcheck="false" inputmode="text"
          aria-invalid={bad(i) ? 'true' : undefined} aria-describedby="rmsg"
          oninput={(e) => setWord(i, e.currentTarget.value)} onfocus={() => (active = i)}
          onblur={() => { seen[i] = true; if (active === i) active = -1; }} onkeydown={(e) => key(e, i)} />
      </label>
    {/each}
  </div>
  <div class="dock">
    <div class="sugg" aria-label="Suggestions" role="group">
      {#if sugg.length}{#each sugg as s (s)}<button type="button" class="chip press" onmousedown={(e) => e.preventDefault()} onclick={() => pick(s)}>{s}</button>{/each}{/if}
    </div>
    <p id="rmsg" class="err" role="alert">{msg}</p>
  </div>
  {#snippet foot()}
    <Btn type="submit" disabled={filled === 0}>Continue</Btn>
  {/snippet}
</StepShell>

<style>
  .tools { display: flex; align-items: center; justify-content: space-between; margin-block: -8px -4px; }
  .cnt { color: var(--muted); font-size: var(--fs-sub); font-variant-numeric: tabular-nums; }
  .grid { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; }
  :global([data-form='desktop']) .grid { grid-template-columns: repeat(4, minmax(0, 1fr)); }
  .cell { display: flex; align-items: center; gap: 6px; min-block-size: 44px; padding-inline: 10px 6px; border: 1px solid var(--line); border-radius: 10px; background: var(--surface); }
  .cell:focus-within { outline: 2px solid var(--accent-ink); outline-offset: 1px; border-color: transparent; }
  .cell.bad { border-color: var(--danger); background: var(--danger-bg); }
  .cell.ok { border-color: var(--accent-soft); }
  .cell i { inline-size: 20px; text-align: end; font-style: normal; font-size: var(--fs-meta); color: var(--muted); font-variant-numeric: tabular-nums; flex: none; }
  .cell input { flex: 1; min-inline-size: 0; border: 0; background: transparent; outline: none; min-block-size: 40px; font: 15px/20px ui-monospace, 'SF Mono', Menlo, Consolas, monospace; }
  .dock { position: sticky; inset-block-end: -16px; margin-block-end: -16px; padding-block: 8px 16px; background: var(--bg); display: grid; gap: 6px; }
  :global([data-form='desktop']) .dock { background: var(--surface); }
  .sugg { display: flex; flex-wrap: wrap; gap: 8px; min-block-size: 44px; align-items: center; }
  .chip { min-block-size: 44px; padding-inline: 16px; border-radius: 22px; border: 0; background: var(--accent-soft); color: var(--accent-ink); font: 600 14px ui-monospace, Menlo, Consolas, monospace; cursor: pointer; }
  .err { margin: 0; color: var(--danger); font-weight: 600; font-size: var(--fs-sub); min-block-size: 18px; }
</style>
