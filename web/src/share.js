import './app.css';
import { mount } from 'svelte';
import { ready } from './lib/crypto.js';
import { loadLanguage, language, t } from './lib/i18n.svelte.js';
import SharePage from './SharePage.svelte';

// The page's language first, so nothing shows in English and then switches.
await Promise.all([ready, loadLanguage()]);
document.documentElement.lang = language();
document.title = `${t('Shared with you')} · thencloud`;
mount(SharePage, { target: document.getElementById('app') });
