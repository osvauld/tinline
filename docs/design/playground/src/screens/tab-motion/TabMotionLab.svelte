<script lang="ts">
  import type { Tab } from '../../lib/nav/router.svelte';
  import { TABS } from '../shell/TabIcon';
  import { TAB_LISTS } from '../registry';
  import { prefs } from '../../lib/model/prefs.svelte';
  import { motion, syncMotionAttr } from '../../lib/fx/reduced.svelte';
  import { dur, setTimeScale } from '../../lib/fx/motion';
  import TabMotionPhone from './TabMotionPhone.svelte';
  import type { Treatment } from './transitions';

  const candidates: { id: Treatment; title: string; description: string }[] = [
    { id: 'through', title: 'A · Fade through', description: 'Current playground. Old list fades out before the next fades in from 98% scale.' },
    { id: 'crossfade', title: 'B · Quiet crossfade · chosen', description: 'Chosen for main tabs: no travel, no scale, no empty beat. Both lists blend over the same short interval.' },
    { id: 'slide', title: 'C · Short lateral', description: '16 px travel in tab order, with a fade. Compare whether direction helps or feels like opening a page.' },
  ];
  let tabs = $state<Record<Treatment, Tab>>({ through: 'chats', crossfade: 'chats', slide: 'chats' });
  let directions = $state<Record<Treatment, number>>({ through: 1, crossfade: 1, slide: 1 });
  let linked = $state(true);
  let speed = $state(1);
  let rtl = $state(false);
  let chosen = $state<Treatment | ''>('');
  let touring = $state(false);
  let tourId = 0;
  const durations = $derived.by(() => {
    speed;
    return { through: dur('tabOut') + dur('tabIn'), crossfade: dur('crossfade'), slide: dur('tabIn') };
  });
  $effect(() => { prefs.form = 'android'; });
  $effect(() => {
    if (prefs.theme === 'system') delete document.documentElement.dataset.theme;
    else document.documentElement.dataset.theme = prefs.theme;
  });
  $effect(() => syncMotionAttr());

  function change(ids: Treatment[], next: Tab) {
    for (const id of ids) {
      if (tabs[id] === next) continue;
      directions[id] = Math.sign(TABS.indexOf(next) - TABS.indexOf(tabs[id]));
      tabs[id] = next;
    }
  }
  function stop() { tourId++; touring = false; }
  function select(id: Treatment, next: Tab) {
    stop();
    change(linked ? candidates.map((c) => c.id) : [id], next);
  }
  function all(next: Tab) { stop(); change(candidates.map((c) => c.id), next); }
  function slow(next: number) { speed = next; setTimeScale(next); }
  async function walk() {
    const id = ++tourId;
    touring = true;
    // One round trip, including a non-adjacent jump. RAF time is only a viewing pause, never an animation clock.
    for (const next of ['chats', 'groups', 'calls', 'contacts', 'chats', 'contacts', 'calls', 'groups', 'chats'] as Tab[]) {
      if (id !== tourId) return;
      change(candidates.map((c) => c.id), next);
      await new Promise<void>((resolve) => {
        const end = performance.now() + dur('tabOut') + dur('tabIn') + dur('enter') * 4;
        function frame(now: number) {
          if (id !== tourId || now >= end) resolve(); else requestAnimationFrame(frame);
        }
        requestAnimationFrame(frame);
      });
    }
    if (id === tourId) touring = false;
  }
</script>

<div class="lab">
  <header class="intro">
    <p class="eyebrow">Tinline interaction lab · Quiet crossfade chosen</p>
    <h1>Moving between main tabs</h1>
    <p>Same data, three treatments. The app bar, search dock and navigation stay still; only the title and list change.</p>
    <div class="controls">
      <label><input type="checkbox" bind:checked={linked} /> Switch together</label>
      <label>Theme <select aria-label="Theme" bind:value={prefs.theme}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label>
      <label>Motion <select aria-label="Motion" bind:value={motion.pref}><option value="system">System</option><option value="reduce">Reduced</option><option value="full">Full</option></select></label>
      <div class="speed" role="group" aria-label="Playback speed"><button type="button" aria-pressed={speed === 1} onclick={() => slow(1)}>1×</button><button type="button" aria-pressed={speed === 4} onclick={() => slow(4)}>¼ speed</button></div>
      <label><input type="checkbox" bind:checked={rtl} /> RTL</label>
      <button type="button" onclick={() => touring ? stop() : void walk()}>{touring ? 'Stop walkthrough' : 'Walk all tabs'}</button>
    </div>
    <div class="jump" role="group" aria-label="Switch every preview">
      <span>Compare</span>{#each TABS as tab}<button type="button" data-compare={tab} onclick={() => all(tab)}>{TAB_LISTS[tab].title}</button>{/each}
    </div>
  </header>
  <main class="comparisons">
    {#each candidates as candidate (candidate.id)}
      <article data-candidate={candidate.id}>
        <div class="caption"><h2>{candidate.title}</h2><span>{motion.reduced ? 'Instant' : `${durations[candidate.id]} ms`}</span></div>
        <p class="description">{candidate.description}</p>
        <TabMotionPhone treatment={candidate.id} tab={tabs[candidate.id]} direction={directions[candidate.id]} onselect={(tab) => select(candidate.id, tab)} {rtl} />
        <button class="pick" type="button" aria-pressed={chosen === candidate.id} onclick={() => chosen = chosen === candidate.id ? '' : candidate.id}>{chosen === candidate.id ? 'Preferred in this session' : 'Prefer this treatment'}</button>
      </article>
    {/each}
  </main>
  <aside class="notes">
    <h2>What to judge</h2>
    <p>Try adjacent tabs, Chats → Contacts, rapid back-and-forth, and tapping the selected tab again. Scroll a list, leave it, then return. Check whether the new title is readable immediately and whether the fixed controls feel settled.</p>
    <p><b>Chosen: B · Quiet crossfade.</b> Main tabs blend over 120 ms, with no delay, travel or scale. Reduced motion is instant. A and C remain here for comparison; the preference buttons are only temporary lab notes. Native Android smoothness still needs device validation.</p>
    <p><b>Proposed search layout:</b> Contacts uses bottom search instead of its top filter. This study reserves the same dock height on Calls too, with no extra action, so changing tabs cannot make the list jump. That Calls placement is a separate decision.</p>
    <p>List contents and search/action shapes are read-only previews. Only tab navigation and lab controls are active. Chats, Groups and Calls reuse existing list components; Contacts reads the same shared people store without the top filter. The chosen motion is implemented in Android’s tab panel. This lab itself does not mutate production navigation or data; the search-layout proposals remain separate.</p>
  </aside>
</div>

<style>
  :global(body) { overflow: auto; }
  .lab { max-inline-size: 1320px; margin-inline: auto; padding: 24px; }
  .eyebrow { color: var(--accent-ink); font-size: var(--fs-sub); font-weight: 650; margin: 0 0 8px; }
  h1 { margin: 0; font-size: clamp(26px, 3vw, 36px); letter-spacing: -.025em; }
  .intro > p:not(.eyebrow), .notes p { color: var(--muted); line-height: 1.55; }
  .controls, .jump, .speed { display: flex; flex-wrap: wrap; align-items: center; gap: 8px; }
  .controls { gap: 8px 16px; }
  label { display: inline-flex; align-items: center; gap: 6px; min-block-size: 44px; }
  input[type='checkbox'] { accent-color: var(--accent); inline-size: 18px; block-size: 18px; }
  button, select { min-block-size: 44px; border: 1px solid var(--line); border-radius: 12px; color: var(--ink); background: var(--surface); padding-inline: 12px; font: inherit; cursor: pointer; }
  button[aria-pressed='true'] { background: var(--accent-soft); color: var(--accent-ink); border-color: var(--accent); }
  .jump { margin-block: 12px 20px; }
  .jump > span { color: var(--muted); margin-inline-end: 8px; }
  .comparisons { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 24px; }
  article { min-inline-size: 0; }
  .caption { display: flex; justify-content: space-between; align-items: baseline; gap: 8px; }
  .caption h2 { font-size: 18px; margin: 0; }
  .caption span { color: var(--muted); font-size: var(--fs-sub); white-space: nowrap; }
  .description { color: var(--muted); font-size: var(--fs-sub); line-height: 1.5; min-block-size: 60px; margin-block: 8px 14px; }
  .pick { inline-size: 100%; margin-block-start: 12px; }
  .notes { margin-block-start: 28px; border-block-start: 1px solid var(--line); padding-block-start: 16px; }
  .notes h2 { font-size: 18px; }
  @media (max-width: 1000px) { .comparisons { grid-template-columns: minmax(0, 390px); justify-content: center; } .description { min-block-size: 0; } }
  @media (max-width: 440px) { .lab { padding: 16px; } }
</style>
