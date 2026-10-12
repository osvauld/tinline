<script lang="ts">
  /* Dialogs that belong to People and open from several places (the + button, Contacts, the new-chat picker). Mounted once
     in each shell, inside the app frame like every other layer. */
  import '../profile/profile.css';
  import Layer from '../chat/Layer.svelte';
  import NewChat from './NewChat.svelte';
  import AddContact from './AddContact.svelte';
  import { pp, closeDialog } from '../../lib/model/actions/people.svelte';
</script>

{#if pp.dialog === 'newChat'}
  <Layer variant="dialog" label="New chat" onclose={closeDialog}><NewChat /></Layer>
{:else if pp.dialog === 'addContact'}
  <Layer variant="dialog" label="Add contact" onclose={closeDialog}><AddContact /></Layer>
{/if}
