// Applies the saved theme, accent colour and tint before first paint (a
// separate file because the CSP forbids inline scripts). Only these display
// preferences are stored locally; never keys or tokens.
(function () {
  var pref = 'system';
  var accent = null;
  var tint = false;
  try {
    pref = localStorage.getItem('theme') || 'system';
    accent = localStorage.getItem('accent');
    tint = localStorage.getItem('tint') === '1';
  } catch (e) {}
  var root = document.documentElement;
  var dark = pref === 'dark' || (pref === 'system' && matchMedia('(prefers-color-scheme: dark)').matches);
  root.classList.toggle('dark', dark);
  root.classList.toggle('tinted', tint);
  if (accent && /^#[0-9a-f]{6}$/i.test(accent)) {
    // Same rule as accentForeground() in src/lib/ui.svelte.js.
    var lum = function (hex) {
      var c = [1, 3, 5].map(function (i) {
        var v = parseInt(hex.slice(i, i + 2), 16) / 255;
        return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
      });
      return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
    };
    var l = lum(accent);
    var onWhite = 1.05 / (l + 0.05);
    var onBlack = (l + 0.05) / 0.05;
    root.style.setProperty('--accent-base', accent);
    root.style.setProperty('--accent-fg', onWhite >= onBlack ? '#ffffff' : '#000000');
  }
})();
