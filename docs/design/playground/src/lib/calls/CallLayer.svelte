<script lang="ts">
  /* Mounted once by each shell: the full call screen and the incoming alert, over the app. */
  import CallScreen from './CallScreen.svelte';
  import IncomingAlert from './IncomingAlert.svelte';
  import GroupCallLayer from './GroupCallLayer.svelte';
  import MiniVideo from './video/MiniVideo.svelte';
  import { call } from './call.svelte';

  const full = $derived(call.phase !== 'idle' && call.view === 'full' && !call.fading);
</script>

{#if full}<CallScreen />{/if}
<IncomingAlert />
<GroupCallLayer />
<MiniVideo />
