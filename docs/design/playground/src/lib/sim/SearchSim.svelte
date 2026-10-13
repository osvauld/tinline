<script lang="ts">
  /* Search simulation controls. Decided = in docs/search.md on branch `search`; Proposed = ours, not decided. */
  import Switch from '../ui/Switch.svelte';
  import { db, chat, newId } from '../model/store.svelte';
  import { clock, T } from '../model/clock.svelte';
  import { receiveText } from '../model/actions/chat';
  import { searchSim } from '../model/actions/search';
  import { ui } from '../../screens/search/state.svelte';

  let text = $state('Lunch on Thursday?');
  let conv = $state('arjun|arjun');
  const CONVS = [['arjun|arjun', 'Arjun'], ['meera|meera', 'Meera'], ['hike|sam', 'Weekend hike (Sam)'], ['family|amma', 'Family (Amma)']];
  const receive = () => { const [id, from] = conv.split('|'); if (text.trim()) receiveText(id, from, text.trim()); };

  const TEMPLATES = ['Lunch plans for today?', 'Running late, start lunch without me', 'Same lunch place as last week?', 'Can we move lunch to one?', 'Meeting notes are in the folder', 'Did you see the flat photos?'];
  function importHistory() {
    const c = chat('arjun');
    if (!c) return;
    const old = Array.from({ length: 60 }, (_, i) => ({
      id: newId('m'), from: i % 3 === 1 ? 'me' : 'arjun', at: clock.ago((20 + i) * T.DAY - (i % 5) * T.HOUR), text: TEMPLATES[i % TEMPLATES.length], ...(i % 3 === 1 ? { delivery: 'delivered' as const } : {}),
    }));
    c.messages.unshift(...old);
  }
  function deleteNewest() {
    const c = chat('arjun');
    const m = c?.messages.filter((x) => !x.deleted && !x.call).at(-1);
    if (m) { m.deleted = true; m.text = ''; }
  }
  function editNewestMine() {
    const m = chat('arjun')?.messages.filter((x) => x.from === 'me' && x.text && !x.deleted).at(-1);
    if (m) { m.text = 'Edited: lunch at one on Thursday'; m.edited = true; m.editedAt = clock.now; }
  }
</script>

<div class="rows">
  <div class="row"><span>Search position</span>
    <select bind:value={ui.placement} aria-label="Search position">
      <option value="default">Default for this form</option><option value="bottom">Bottom</option><option value="top">Top</option>
    </select>
  </div>
  <label class="row"><span>Index getting ready <i class="prop">Proposed</i></span><Switch label="Search index getting ready" bind:checked={searchSim.rebuilding} /></label>
  <label class="row"><span>Soft keyboard (mock, Android search)</span><Switch label="Soft keyboard mock" bind:checked={ui.kbd} /></label>

  <div class="blk">
    <input type="text" bind:value={text} aria-label="Message text" />
    <select bind:value={conv} aria-label="Conversation">{#each CONVS as [v, l] (v)}<option value={v}>{l}</option>{/each}</select>
    <div class="btns"><button type="button" onclick={receive}>Receive a matching message</button></div>
  </div>
  <div class="btns">
    <button type="button" onclick={importHistory}>Import 60 older messages (Arjun)</button>
    <button type="button" onclick={editNewestMine}>Edit my newest Arjun message</button>
    <button type="button" onclick={deleteNewest}>Delete newest Arjun message</button>
    <button type="button" onclick={() => ui.forgetAll()}>Clear recent searches</button>
  </div>

  <p class="note"><b>Decided</b> (docs/search.md): one local search over 1:1 chats, groups and names; People &amp; groups first, then Messages (newest first, 20 a page, cursor paging); words match by prefix, AND of terms, case and Latin accents ignored, Indic marks kept; messages need 2 characters, names 1; snippet about 80 characters around the first match, matches in bold; 150 ms debounce; deleted messages and system lines are never found; tapping a hit opens the conversation at that message, briefly highlighted; Ctrl+K focuses the desktop field.</p>
  <p class="note"><b>User decision:</b> Android search at the bottom (pill above the tab bar, new-chat button in the same row), desktop at the top; both placements here to compare. Nothing may move while results change.</p>
  <p class="note"><b>Proposed:</b> the position toggle itself; People &amp; groups as one fixed-height row of tiles (so the Messages header never moves); recent searches (local only); "Search is getting ready" while the index is rebuilt (the core reconciles in the background; here only the last 8 days are found meanwhile); find in this chat (Ctrl+F, n/m, up/down); "Left" and "Removed" next to a group's name in a hit; the new-chat button opening Contacts (no new-chat flow is designed); author prefix ("Sam:", "You:") before a snippet.</p>
</div>

<style>
  .rows { display: grid; gap: 4px; }
  .row { display: flex; align-items: center; justify-content: space-between; gap: 12px; min-height: 40px; font-size: var(--fs-sub); }
  .prop { font-style: normal; font-size: 11px; color: var(--gold-ink, var(--muted)); margin-inline-start: 4px; }
  .blk { display: grid; gap: 6px; margin-top: 6px; }
  .btns { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 6px; }
  button { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 10px; font-size: var(--fs-sub); cursor: pointer; }
  select, input { border: 1px solid var(--line); background: var(--surface); border-radius: 10px; padding: 6px 8px; font-size: var(--fs-sub); }
  .note { font-size: var(--fs-sub); color: var(--muted); margin: 8px 0 0; line-height: 18px; }
</style>
