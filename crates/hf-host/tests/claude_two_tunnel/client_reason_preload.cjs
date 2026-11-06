'use strict';
(() => {
  try {
    const { createWriter } = require('./client_reason_writer.cjs');
    const { install } = require('./client_reason_hook.cjs');
    const writer = createWriter();
    if (typeof globalThis.fetch !== 'function') return;
    const observer = install(globalThis.fetch, writer);
    globalThis.fetch = observer.proxy;
    process.once('exit', () => { try { observer.finish(); } catch (_) {} });
    writer.write(['CR2']);
  } catch (_) { /* Missing completion refuses qualification. */ }
})();
