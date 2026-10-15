import { mount } from 'svelte';
import '../../app.css';
import { applyMotionVars } from '../../lib/fx/motion';
import TabMotionLab from './TabMotionLab.svelte';

applyMotionVars();
mount(TabMotionLab, { target: document.getElementById('app')! });
