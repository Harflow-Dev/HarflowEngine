'use strict';
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { createWriter } = require('./client_reason_writer.cjs');

function prepare(file, nonce) {
  const fd = fs.openSync(file, 'wx', 0o600);
  fs.closeSync(fd);
  const stamp = fs.statSync(file, { bigint: true });
  process.env.HARFLOW_TLS_OBSERVER_FILE = file;
  process.env.HARFLOW_TLS_OBSERVER_NONCE = nonce;
  process.env.HARFLOW_TLS_OBSERVER_DEV = stamp.dev.toString();
  process.env.HARFLOW_TLS_OBSERVER_INODE = stamp.ino.toString();
}

function captureEnvironment() {
  const keys = [
    'HARFLOW_TLS_OBSERVER_FILE', 'HARFLOW_TLS_OBSERVER_NONCE',
    'HARFLOW_TLS_OBSERVER_DEV', 'HARFLOW_TLS_OBSERVER_INODE',
  ];
  return keys.map(key => [key, process.env[key]]);
}

function restoreEnvironment(saved) {
  for (const [key, value] of saved) {
    if (value === undefined) delete process.env[key];
    else process.env[key] = value;
  }
}

function testBoundedAppend(root) {
  const file = path.join(root, 'metadata');
  const nonce = '0123456789abcdef0123456789abcdef';
  prepare(file, nonce);
  const writer = createWriter();
  assert.equal(writer.write(['CR1']), true);
  for (let call = 1; call <= 4; call++)
    assert.equal(writer.write(['CE1', call, 'unknown', 'missing']), true);
  assert.equal(writer.write(['CO1']), true);
  assert.equal(writer.write(['CX1', 4, 4, 1, 1]), true);
  const data = fs.readFileSync(file);
  assert(data.length <= 512);
  assert.equal(data.toString('ascii').split('\n').length, 8);
  const stamp = fs.statSync(file, { bigint: true });
  assert.equal(stamp.nlink, 1n);
  assert.equal(stamp.mode & 0o777n, 0o600n);
  assert.equal(stamp.dev.toString(), process.env.HARFLOW_TLS_OBSERVER_DEV);
  assert.equal(stamp.ino.toString(), process.env.HARFLOW_TLS_OBSERVER_INODE);
}

function testRefusals(root) {
  const symlinkTarget = path.join(root, 'target');
  fs.writeFileSync(symlinkTarget, 'untouched', { mode: 0o600 });
  const symlink = path.join(root, 'symlink');
  fs.symlinkSync(symlinkTarget, symlink);
  let before = fs.readFileSync(symlinkTarget);
  prepareIdentity(symlink);
  assert.equal(createWriter().write(['CR1']), false);
  assert.deepEqual(fs.readFileSync(symlinkTarget), before);

  const linked = path.join(root, 'linked');
  const alias = path.join(root, 'alias');
  fs.writeFileSync(linked, '', { mode: 0o600 });
  fs.linkSync(linked, alias);
  before = fs.readFileSync(linked);
  prepareIdentity(linked);
  assert.equal(createWriter().write(['CR1']), false);
  assert.deepEqual(fs.readFileSync(linked), before);

  const wrongMode = path.join(root, 'wrong-mode');
  fs.writeFileSync(wrongMode, '', { mode: 0o644 });
  prepareIdentity(wrongMode);
  assert.equal(createWriter().write(['CR1']), false);
  assert.equal(fs.statSync(wrongMode).size, 0);

  const wrongIdentity = path.join(root, 'wrong-identity');
  fs.writeFileSync(wrongIdentity, '', { mode: 0o600 });
  prepareIdentity(wrongIdentity);
  process.env.HARFLOW_TLS_OBSERVER_INODE = '0';
  assert.equal(createWriter().write(['CR1']), false);
  assert.equal(fs.statSync(wrongIdentity).size, 0);

  const bounded = path.join(root, 'bounded');
  fs.writeFileSync(bounded, 'x'.repeat(500), { mode: 0o600 });
  prepareIdentity(bounded);
  assert.equal(createWriter().write(['CR1']), false);
  assert.equal(fs.statSync(bounded).size, 500);
}

function prepareIdentity(file) {
  const stamp = fs.lstatSync(file, { bigint: true });
  process.env.HARFLOW_TLS_OBSERVER_FILE = file;
  process.env.HARFLOW_TLS_OBSERVER_NONCE = '0123456789abcdef0123456789abcdef';
  process.env.HARFLOW_TLS_OBSERVER_DEV = stamp.dev.toString();
  process.env.HARFLOW_TLS_OBSERVER_INODE = stamp.ino.toString();
}

function main() {
  const saved = captureEnvironment();
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'harflow-client-reason-'));
  try {
    testBoundedAppend(root);
    testRefusals(root);
  } finally {
    restoreEnvironment(saved);
    fs.rmSync(root, { recursive: true, force: true });
  }
}

main();
