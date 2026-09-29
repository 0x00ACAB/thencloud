import './app.css';
import { mount } from 'svelte';
import { ready } from './lib/crypto.js';
import { loadLanguage } from './lib/i18n.svelte.js';
import App from './App.svelte';

// The page's language first, so nothing shows in English and then switches.
await Promise.all([ready, loadLanguage()]);
mount(App, { target: document.getElementById('app') });
