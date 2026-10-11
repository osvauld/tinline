<script lang="ts">
  import Btn from './Btn.svelte';
  import Field from './Field.svelte';
  import StepShell from './StepShell.svelte';
  import { gate, CONFIRM_AT } from '../../lib/model/actions/gate.svelte';
  let answers = $state(['', '', '']);
  let tried = $state(false);
  import { DEMO_PHRASE } from '../../lib/model/actions/gate.svelte';
  const wrong = (i: number) => tried && answers[i].trim().toLowerCase() !== DEMO_PHRASE[CONFIRM_AT[i] - 1];
  const submit = () => { tried = true; if (gate.confirmOk(answers)) gate.back(); };
</script>

<StepShell title="Check my copy" lead="Optional. Type the words you wrote down, to be sure the paper is right." form onsubmit={submit}>
  <div class="col">
    {#each CONFIRM_AT as p, i (p)}
      <Field label="Word {p}" mono bind:value={answers[i]} autofocus={i === 0} error={wrong(i) ? 'That isn’t word ' + p + '. Check your written copy.' : null} />
    {/each}
  </div>
  {#snippet foot()}
    <Btn type="submit" disabled={answers.some((a) => !a.trim())}>Done</Btn>
    <Btn kind="text" onclick={() => gate.back()}>Back to the words</Btn>
  {/snippet}
</StepShell>

<style>
  .col { display: grid; gap: 14px; }
</style>
