<script lang="ts">
  /* Links in a profile are data. Following one is a deliberate step, and it opens outside Tinline: nothing is fetched or embedded. */
  import Layer from '../chat/Layer.svelte';
  import Btn from '../groups/Btn.svelte';
  let { url, onclose }: { url: string; onclose: () => void } = $props();
  function open() { window.open(url, '_blank', 'noopener,noreferrer'); onclose(); }
</script>

<Layer variant="dialog" label="Open link" {onclose}>
  <div class="pf-dlg" data-open-link>
    <h2>Open this link?</h2>
    <p class="url" dir="ltr">{url}</p>
    <p>It opens in your browser, outside Tinline. Tinline doesn’t load it for you, and can’t vouch for what’s there.</p>
    <div class="pf-acts"><Btn variant="quiet" data-autofocus onclick={onclose}>Cancel</Btn><Btn variant="primary" onclick={open}>Open link</Btn></div>
  </div>
</Layer>
