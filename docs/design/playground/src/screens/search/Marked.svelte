<script lang="ts">
  /* Text with highlighted ranges. `ranges` are character (code point) offsets, as the core returns them; `utf16` for offsets already in JS units.
     No whitespace between the parts: the bubble is white-space:pre-wrap and the meta floats right after the text. */
  import { parts, type HitRange } from '../../lib/model/actions/search';
  let { text, ranges, utf16 = false, variant = 'bold', cur = false, lead = 0 }: { text: string; ranges: HitRange[]; utf16?: boolean; variant?: 'bold' | 'mark'; cur?: boolean; lead?: number } = $props();
  const ps = $derived(trimLead(utf16 ? plain(text, ranges) : parts(text, ranges), lead));
  /* One-line snippets: keep at most `lead` characters before the first match (from a word start, after "…"),
     so the match is always visible instead of falling past the ellipsis at the end. */
  function trimLead(ps: { t: string; hit: boolean }[], n: number) {
    if (!n || !ps.length || ps[0].hit) return ps;
    const head = Array.from(ps[0].t);
    if (head.length <= n) return ps;
    let tail = head.slice(-n).join('');
    const sp = tail.search(/\s/);
    if (sp >= 0 && sp < n - 4) tail = tail.slice(sp + 1);
    return [{ t: '…' + tail, hit: false }, ...ps.slice(1)];
  }
  function plain(t: string, rs: HitRange[]) {
    const out: { t: string; hit: boolean }[] = [];
    let p = 0;
    for (const r of rs) { if (r.start > p) out.push({ t: t.slice(p, r.start), hit: false }); if (r.end > r.start) out.push({ t: t.slice(r.start, r.end), hit: true }); p = Math.max(p, r.end); }
    if (p < t.length) out.push({ t: t.slice(p), hit: false });
    return out;
  }
</script>

{#each ps as p, i (i)}{#if p.hit}{#if variant === 'mark'}<mark class="m" class:cur>{p.t}</mark>{:else}<b class="b">{p.t}</b>{/if}{:else}{p.t}{/if}{/each}

<style>
  .b { font-weight: 700; color: var(--ink); }
  /* Find in chat: a wash of gold behind the text, stronger on the current match. Text colour is never changed (contrast stays the bubble's own). */
  .m { color: inherit; background: color-mix(in srgb, var(--gold) 38%, transparent); border-radius: 3px; padding: 0; box-decoration-break: clone; -webkit-box-decoration-break: clone; transition: background-color var(--dur-switch) var(--ease-out), box-shadow var(--dur-switch) var(--ease-out); }
  .m.cur { background: color-mix(in srgb, var(--gold) 70%, transparent); box-shadow: 0 0 0 1.5px color-mix(in srgb, var(--gold) 70%, transparent); }
</style>
