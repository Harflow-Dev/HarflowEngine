'use strict';
const fs = require('node:fs');
const open = fs.openSync, stat = fs.fstatSync, write = fs.writeSync, close = fs.closeSync;
const from = Buffer.from, integer = Number.isInteger;
const apply = Reflect.apply, join = Array.prototype.join;
const bigint = BigInt, bigintString = BigInt.prototype.toString;
const categories = new Set(['missing','accessor','unknown','trap','verify','issuer','selfsigned',
  'expired','hostname','reset','refused','not_yet_valid','protocol','timeout','cancelled',
  'broken_pipe','not_connected']);
const has = Set.prototype.has;
function validFields(f) {
  if ((f[0] === 'CR1' || f[0] === 'CO1') && f.length === 1) return true;
  if (f[0] === 'CE1' && f.length === 4) return integer(f[1]) && f[1] >= 1 && f[1] <= 4 &&
    apply(has, categories, [f[2]]) && apply(has, categories, [f[3]]);
  return f[0] === 'CX1' && f.length === 5 && integer(f[1]) && f[1] >= 0 && f[1] <= 4 &&
    integer(f[2]) && f[2] >= 0 && f[2] <= 4 && (f[3] === 0 || f[3] === 1) &&
    (f[4] === 0 || f[4] === 1);
}
const flags = fs.constants.O_WRONLY | fs.constants.O_APPEND | fs.constants.O_NOFOLLOW;
function createWriter() {
  const path = process.env.HARFLOW_TLS_OBSERVER_FILE;
  const nonce = process.env.HARFLOW_TLS_OBSERVER_NONCE;
  const dev = process.env.HARFLOW_TLS_OBSERVER_DEV;
  const ino = process.env.HARFLOW_TLS_OBSERVER_INODE;
  const pid = process.pid;
  // CR48 + four CE78 + CO48 + CX56 = 464 bytes at u32 PID/category13.
  let size = 0, healthy = true;
  const valid = typeof path === 'string' && /^[0-9a-f]{32}$/.test(nonce) &&
    /^[0-9]+$/.test(dev) && /^[0-9]+$/.test(ino) && integer(pid) && pid > 0 && pid <= 4294967295 &&
    integer(fs.constants.O_NOFOLLOW) && fs.constants.O_NOFOLLOW !== 0;
  function append(fields) {
    let fd;
    try {
      if (!valid || !healthy) return false;
      if (!validFields(fields)) throw null;
      const prefix = fields[0] + '\t' + nonce + '\t' + pid;
      const suffix = fields.length === 1 ? '' : '\t' + apply(join, fields.length === 4 ?
        [fields[1], fields[2], fields[3]] : [fields[1], fields[2], fields[3], fields[4]], ['\t']);
      const row = from(prefix + suffix + '\n', 'ascii');
      if (size + row.length > 512) { healthy = false; return false; }
      fd = open(path, flags);
      const s = stat(fd, { bigint: true });
      if ((s.mode & 0o170000n) !== 0o100000n || s.nlink !== 1n || (s.mode & 0o777n) !== 0o600n ||
          s.size !== bigint(size) || apply(bigintString, s.dev, []) !== dev || apply(bigintString, s.ino, []) !== ino)
        throw null;
      if (write(fd, row, 0, row.length) !== row.length) throw null;
      size += row.length;

    } catch (_) { healthy = false; return false; }
    finally { if (fd !== undefined) { try { close(fd); } catch (_) { healthy = false; } } }
    return healthy;
  }
  return { write: append };
}
module.exports = { createWriter };
