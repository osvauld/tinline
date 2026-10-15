import { mount } from 'svelte';
import '../../app.css';
import { applyMotionVars } from '../../lib/fx/motion';
import { setName } from '../../lib/model/actions/profile';
import { CARD_OWNER, DIRECTORY } from '../../lib/model/actions/people.svelte';
import RecoveryLab from './RecoveryLab.svelte';

// The app captures one user-chosen name, not first/last names. Seed only this isolated lab.
setName('maya');
const sampleContact = DIRECTORY[0];
CARD_OWNER.set(sampleContact.card, { id: sampleContact.id, name: 'tara', hue: sampleContact.hue });
applyMotionVars();
mount(RecoveryLab, { target: document.getElementById('app')! });
