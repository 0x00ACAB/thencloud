import './app.css';
import { mount } from 'svelte';
import { ready } from './lib/crypto.js';
import { loadLanguage } from './lib/i18n.svelte.js';
import SharePage from './SharePage.svelte';

// The page's language first, so nothing shows in English and then switches.
await Promise.all([ready, loadLanguage()]);
mount(SharePage, { target: document.getElementById('app') });
