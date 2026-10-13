/* Find in this chat (desktop Ctrl+F). Proposed: the core design only has the global search. State of the find bar only; the matches are derived
   from the conversation in Conversation.svelte (findInChat) so they follow edits, deletes and arrivals live. */
class Find {
  open = $state(false);
  q = $state('');
  /** Index of the current match among the matches, oldest first. A big number = the newest (clamped by the reader). */
  cur = $state(Number.MAX_SAFE_INTEGER);
  focusReq = $state(0);
  show() { this.open = true; this.focusReq++; }
  close() { this.open = false; this.q = ''; this.cur = Number.MAX_SAFE_INTEGER; }
  setQ(v: string) { this.q = v; this.cur = Number.MAX_SAFE_INTEGER; }
}
export const find = new Find();
