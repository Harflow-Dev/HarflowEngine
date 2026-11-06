'use strict';
// Fixed inert reporter. No request hooks or CLI output.
(() => {
  let fd;
  try {
    const fs = require('node:fs');
    const path = process.env.HARFLOW_TLS_OBSERVER_FILE;
    const nonce = process.env.HARFLOW_TLS_OBSERVER_NONCE;
    const dev = process.env.HARFLOW_TLS_OBSERVER_DEV;
    const ino = process.env.HARFLOW_TLS_OBSERVER_INODE;
    const nofollow = fs.constants.O_NOFOLLOW;
    if (!Number.isInteger(nofollow) || nofollow === 0 ||
        typeof path !== 'string' || !/^[0-9a-f]{32}$/.test(nonce) ||
        !/^[0-9]+$/.test(dev) || !/^[0-9]+$/.test(ino)) return;
    fd = fs.openSync(path, fs.constants.O_WRONLY | fs.constants.O_APPEND | nofollow);
    const stamp = fs.fstatSync(fd, { bigint: true });
    if (!stamp.isFile() || stamp.nlink !== 1n || stamp.size !== 0n ||
        stamp.dev.toString() !== dev || stamp.ino.toString() !== ino) return;
    const row = Buffer.from(`CLIENT_PRELOAD_READY_V1\t${nonce}\t${process.pid}\n`, 'ascii');
    if (row.length <= 512) fs.writeSync(fd, row, 0, row.length);
  } catch (_) {
    // Absence is unavailable, never evidence of a particular compile feature.
  } finally {
    if (fd !== undefined) { try { require('node:fs').closeSync(fd); } catch (_) {} }
  }
})();
