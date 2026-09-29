import './app.css';
import { mount } from 'svelte';
import { loadLanguage, language } from './lib/i18n.svelte.js';
import AuthCheck from './AuthCheck.svelte';

// No crypto here: this page never sees a password or a key.
await loadLanguage();
document.documentElement.lang = language();
mount(AuthCheck, { target: document.getElementById('app') });
