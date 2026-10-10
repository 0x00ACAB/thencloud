// The end of linking a storage account (Google Drive): the server sends the
// popup here with how it went after the #. Tell the app's page, which is
// waiting on the same channel, and close; say so if the window stays open.
(() => {
  const outcome = location.hash.slice(1) || 'failed';
  try {
    const ch = new BroadcastChannel('thencloud-storage-link');
    ch.postMessage(outcome);
    ch.close();
  } catch {
    // The app also notices by asking the server.
  }
  const lang = (() => {
    try {
      const f = JSON.parse(localStorage.getItem('format') || '{}');
      if (f.language && f.language !== 'auto') return f.language;
    } catch {
      // Private window: fall back to the browser's language.
    }
    return (navigator.language || 'en').slice(0, 2);
  })();
  const text = {
    en: { linked: 'Google Drive is linked. You can close this window.', other: "Google Drive wasn't linked. You can close this window and try again." },
    pl: { linked: 'Dysk Google jest połączony. Możesz zamknąć to okno.', other: 'Nie udało się połączyć Dysku Google. Możesz zamknąć to okno i spróbować ponownie.' },
    de: { linked: 'Google Drive ist verbunden. Du kannst dieses Fenster schließen.', other: 'Google Drive wurde nicht verbunden. Du kannst dieses Fenster schließen und es noch einmal versuchen.' },
  }[lang] || null;
  const t = text || { linked: 'Google Drive is linked. You can close this window.', other: "Google Drive wasn't linked. You can close this window and try again." };
  document.documentElement.lang = text ? lang : 'en';
  document.getElementById('msg').textContent = outcome === 'linked' ? t.linked : t.other;
  window.close();
})();
