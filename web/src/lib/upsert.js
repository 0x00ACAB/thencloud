// Map.prototype.getOrInsert / getOrInsertComputed (the "upsert" proposal),
// which pdf.js 6 calls but not every browser has yet.
for (const M of [Map, WeakMap]) {
  const define = (name, fn) => M.prototype[name] || Object.defineProperty(M.prototype, name, { value: fn, writable: true, configurable: true });
  define('getOrInsert', function (key, value) {
    if (!this.has(key)) this.set(key, value);
    return this.get(key);
  });
  define('getOrInsertComputed', function (key, compute) {
    if (!this.has(key)) this.set(key, compute(key));
    return this.get(key);
  });
}
