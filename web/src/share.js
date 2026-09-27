import './app.css';
import { mount } from 'svelte';
import { ready } from './lib/crypto.js';
import SharePage from './SharePage.svelte';

await ready;
mount(SharePage, { target: document.getElementById('app') });
